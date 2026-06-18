use aws_credential_types::{Credentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Region},
    presigning::PresigningConfig,
};
use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, procedure,
    rand::RngCore, view,
};
use std::time::Duration;

use crate::config::s3_config as _;
use crate::user::{require_registered_user, session as _, session__view as _};

const UPLOAD_URL_TTL_SECS: u64 = 900;
const DOWNLOAD_URL_TTL_SECS: u64 = 300;
const INLINE_CONTENT_MAX_BYTES: usize = 256 * 1024;

#[spacetimedb::table(accessor = user_file, public)]
pub struct UserFile {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    #[index(btree)]
    pub name: String,
    pub path: Option<String>,
    pub hash: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub inline_content: Option<String>,
    pub is_directory: bool,
    pub s3_key: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct FileMetadata {
    pub id: u64,
    pub name: String,
    pub path: Option<String>,
    pub hash: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub inline_content: Option<String>,
    pub is_directory: bool,
    pub s3_key: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl From<UserFile> for FileMetadata {
    fn from(f: UserFile) -> Self {
        Self {
            id: f.id,
            name: f.name,
            path: f.path,
            hash: f.hash,
            size_bytes: f.size_bytes,
            content_type: f.content_type,
            inline_content: f.inline_content,
            is_directory: f.is_directory,
            s3_key: f.s3_key,
            created_at: f.created_at,
            updated_at: f.updated_at,
        }
    }
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct UploadTicket {
    pub file_id: u64,
    pub upload_url: String,
    pub s3_key: String,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct ReplaceTicket {
    pub file_id: u64,
    pub upload_url: String,
    pub s3_key: String,
}

fn load_s3_config(ctx: &ReducerContext) -> Result<crate::config::S3Config, String> {
    ctx.db
        .s3_config()
        .id()
        .find(crate::config::S3_CONFIG_ID)
        .filter(|c| {
            !c.bucket.is_empty()
                && !c.region.is_empty()
                && !c.access_key_id.is_empty()
                && !c.secret_access_key.is_empty()
        })
        .ok_or_else(|| "s3 is not configured".to_string())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn validate_file_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > 256 {
        return Err("name must be 256 characters or fewer".to_string());
    }
    if name.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("name cannot contain control characters".to_string());
    }
    Ok(name)
}

fn validate_file_path(path: Option<String>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim().to_string();
    if path.is_empty() {
        return Ok(None);
    }
    if path.len() > 1024 {
        return Err("path must be 1024 characters or fewer".to_string());
    }
    if path.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("path cannot contain control characters".to_string());
    }
    Ok(Some(path))
}

fn path_conflict(
    ctx: &ReducerContext,
    owner: Identity,
    path: &Option<String>,
    except_id: Option<u64>,
) -> bool {
    let Some(path) = path.as_deref() else {
        return false;
    };
    ctx.db
        .user_file()
        .owner()
        .filter(owner)
        .any(|f| f.path.as_deref() == Some(path) && Some(f.id) != except_id)
}

fn s3_key_for(ctx: &ReducerContext, owner: Identity, name: &str) -> String {
    let prefix = ctx
        .db
        .s3_config()
        .id()
        .find(crate::config::S3_CONFIG_ID)
        .and_then(|c| c.path_prefix)
        .unwrap_or_default();
    let nonce: u32 = ctx.rng().next_u32();
    let safe = sanitize(name);
    if prefix.is_empty() {
        format!("users/{owner}/{nonce:08x}-{safe}")
    } else {
        format!(
            "{}/users/{owner}/{nonce:08x}-{safe}",
            prefix.trim_end_matches('/')
        )
    }
}

#[spacetimedb::reducer]
pub fn register_file(
    ctx: &ReducerContext,
    name: String,
    path: Option<String>,
    hash: String,
    size_bytes: u64,
    content_type: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_file_name(name)?;
    let path = validate_file_path(path)?;
    if hash.is_empty() {
        return Err("hash cannot be empty".to_string());
    }
    load_s3_config(ctx)?;

    let owner = user.identity;
    if path_conflict(ctx, owner, &path, None) {
        return Err("a file or folder already uses this path".to_string());
    }
    let s3_key = s3_key_for(ctx, owner, &name);
    ctx.db.user_file().insert(UserFile {
        id: 0,
        owner,
        name,
        path,
        hash,
        size_bytes,
        content_type,
        inline_content: None,
        is_directory: false,
        s3_key,
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn create_folder(
    ctx: &ReducerContext,
    name: String,
    path: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_file_name(name)?;
    let path = validate_file_path(path)?;
    let owner = user.identity;
    if path_conflict(ctx, owner, &path, None) {
        return Err("a file or folder already uses this path".to_string());
    }
    ctx.db.user_file().insert(UserFile {
        id: 0,
        owner,
        name,
        path,
        hash: String::new(),
        size_bytes: 0,
        content_type: None,
        inline_content: None,
        is_directory: true,
        s3_key: String::new(),
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_file_content(
    ctx: &ReducerContext,
    file_id: Option<u64>,
    name: String,
    path: Option<String>,
    content: String,
    content_type: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_file_name(name)?;
    let path = validate_file_path(path)?;
    if content.len() > INLINE_CONTENT_MAX_BYTES {
        return Err("file content too large (max 256 KiB)".to_string());
    }

    let owner = user.identity;
    let existing = if let Some(id) = file_id {
        Some(
            ctx.db
                .user_file()
                .id()
                .find(id)
                .ok_or_else(|| "file not found".to_string())?,
        )
    } else if let Some(target_path) = path.as_deref() {
        ctx.db
            .user_file()
            .owner()
            .filter(owner)
            .find(|f| f.path.as_deref() == Some(target_path))
    } else {
        None
    };

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());
    let size_bytes = content.len() as u64;
    if let Some(mut file) = existing {
        if file.owner != owner {
            return Err("not your file".to_string());
        }
        if file.is_directory {
            return Err("folders cannot have file content".to_string());
        }
        if path_conflict(ctx, owner, &path, Some(file.id)) {
            return Err("a file or folder already uses this path".to_string());
        }
        file.name = name;
        file.path = path;
        file.hash = hash;
        file.size_bytes = size_bytes;
        file.content_type = content_type;
        file.inline_content = Some(content);
        file.updated_at = ctx.timestamp;
        ctx.db.user_file().id().update(file);
    } else {
        if path_conflict(ctx, owner, &path, None) {
            return Err("a file or folder already uses this path".to_string());
        }
        ctx.db.user_file().insert(UserFile {
            id: 0,
            owner,
            name,
            path,
            hash,
            size_bytes,
            content_type,
            inline_content: Some(content),
            is_directory: false,
            s3_key: String::new(),
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
        });
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_file(ctx: &ReducerContext, file_id: u64) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let file = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if file.owner != user.identity {
        return Err("not your file".to_string());
    }
    ctx.db.user_file().id().delete(file_id);
    Ok(())
}

#[spacetimedb::reducer]
pub fn finalize_upload(
    ctx: &ReducerContext,
    file_id: u64,
    hash: String,
    size_bytes: u64,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let mut file = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if file.owner != user.identity {
        return Err("not your file".to_string());
    }
    if file.is_directory {
        return Err("folders cannot be finalized as uploads".to_string());
    }
    if hash.is_empty() {
        return Err("hash cannot be empty".to_string());
    }
    file.hash = hash;
    file.size_bytes = size_bytes;
    file.inline_content = None;
    file.updated_at = ctx.timestamp;
    ctx.db.user_file().id().update(file);
    Ok(())
}

#[spacetimedb::reducer]
pub fn rename_file(
    ctx: &ReducerContext,
    file_id: u64,
    name: String,
    path: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let mut file = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if file.owner != user.identity {
        return Err("not your file".to_string());
    }
    let name = validate_file_name(name)?;
    let path = validate_file_path(path)?;
    if path_conflict(ctx, user.identity, &path, Some(file_id)) {
        return Err("a file or folder already uses this path".to_string());
    }
    file.name = name;
    file.path = path;
    file.updated_at = ctx.timestamp;
    ctx.db.user_file().id().update(file);
    Ok(())
}

#[view(accessor = my_files, public)]
fn my_files(ctx: &ViewContext) -> Vec<FileMetadata> {
    let Some(user) = ctx.db.session().connection().find(ctx.sender()).map(|s| s.user) else {
        return Vec::new();
    };
    ctx.db
        .user_file()
        .owner()
        .filter(user)
        .map(FileMetadata::from)
        .collect()
}

fn build_s3_client(cfg: &crate::config::S3Config) -> Result<Client, String> {
    let creds = Credentials::new(
        cfg.access_key_id.clone(),
        cfg.secret_access_key.clone(),
        None,
        None,
        "spacenix-static",
    );
    let mut builder = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(cfg.region.clone()))
        .credentials_provider(SharedCredentialsProvider::new(creds));
    if let Some(ep) = cfg.endpoint.as_deref() {
        if !ep.is_empty() {
            builder = builder.endpoint_url(ep);
        }
    }
    Ok(Client::from_conf(builder.build()))
}

#[procedure]
pub fn get_file(
    ctx: &mut spacetimedb::ProcedureContext,
    file_id: u64,
) -> Result<Option<FileMetadata>, String> {
    let sender = ctx.sender();
    ctx.try_with_tx(|tx| -> Result<Option<FileMetadata>, String> {
        let user = tx
            .db
            .session()
            .connection()
            .find(sender)
            .map(|s| s.user)
            .ok_or_else(|| "sign in first".to_string())?;
        Ok(tx
            .db
            .user_file()
            .id()
            .find(file_id)
            .filter(|f| f.owner == user)
            .map(FileMetadata::from))
    })
}

#[procedure]
pub fn search_files(
    ctx: &mut spacetimedb::ProcedureContext,
    query: String,
) -> Result<Vec<FileMetadata>, String> {
    let sender = ctx.sender();
    let q = query.to_lowercase();
    ctx.try_with_tx(|tx| -> Result<Vec<FileMetadata>, String> {
        let user = tx
            .db
            .session()
            .connection()
            .find(sender)
            .map(|s| s.user)
            .ok_or_else(|| "sign in first".to_string())?;
        Ok(tx
            .db
            .user_file()
            .owner()
            .filter(user)
            .filter(|f| {
                f.name.to_lowercase().contains(&q)
                    || f.path
                        .as_deref()
                        .is_some_and(|path| path.to_lowercase().contains(&q))
            })
            .map(FileMetadata::from)
            .collect::<Vec<_>>())
    })
}

#[procedure]
pub fn request_upload_url(
    ctx: &mut spacetimedb::ProcedureContext,
    name: String,
    path: Option<String>,
    content_type: Option<String>,
) -> Result<UploadTicket, String> {
    let name = validate_file_name(name)?;
    let path = validate_file_path(path)?;

    let sender = ctx.sender();
    let nonce: u32 = ctx.rng().next_u32();
    let safe = sanitize(&name);
    let timestamp = ctx.timestamp;

    let prepared = ctx.try_with_tx(
        |tx| -> Result<(crate::config::S3Config, u64, String, Option<String>), String> {
            let user = tx
                .db
                .session()
                .connection()
                .find(sender)
                .map(|s| s.user)
                .ok_or_else(|| "sign in first".to_string())?;
            if let Some(target_path) = path.as_deref() {
                if tx
                    .db
                    .user_file()
                    .owner()
                    .filter(user)
                    .any(|f| f.path.as_deref() == Some(target_path))
                {
                    return Err("a file or folder already uses this path".to_string());
                }
            }
            let cfg = tx
                .db
                .s3_config()
                .id()
                .find(crate::config::S3_CONFIG_ID)
                .filter(|c| {
                    !c.bucket.is_empty()
                        && !c.region.is_empty()
                        && !c.access_key_id.is_empty()
                        && !c.secret_access_key.is_empty()
                })
                .ok_or_else(|| "s3 is not configured".to_string())?;
            let prefix = cfg
                .path_prefix
                .as_deref()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_string();
            let s3_key = if prefix.is_empty() {
                format!("users/{user}/{nonce:08x}-{safe}")
            } else {
                format!("{prefix}/users/{user}/{nonce:08x}-{safe}")
            };
            let inserted = tx.db.user_file().insert(UserFile {
                id: 0,
                owner: user,
                name: name.clone(),
                path: path.clone(),
                hash: String::new(),
                size_bytes: 0,
                content_type: content_type.clone(),
                inline_content: None,
                is_directory: false,
                s3_key: s3_key.clone(),
                created_at: timestamp,
                updated_at: timestamp,
            });
            Ok((cfg, inserted.id, s3_key, inserted.content_type))
        },
    )?;

    let (cfg, file_id, s3_key, ct) = prepared;

    let client = build_s3_client(&cfg)?;

    let presign = PresigningConfig::expires_in(Duration::from_secs(UPLOAD_URL_TTL_SECS))
        .map_err(|e| format!("presign config: {e}"))?;
    let mut req = client.put_object().bucket(cfg.bucket).key(s3_key.clone());
    if let Some(c) = ct.as_deref() {
        req = req.content_type(c);
    }
    let presigned = futures_lite::future::block_on(req.presigned(presign))
        .map_err(|e| format!("presign: {e}"))?;

    Ok(UploadTicket {
        file_id,
        upload_url: presigned.uri().to_string(),
        s3_key,
    })
}

#[procedure]
pub fn request_download_url(
    ctx: &mut spacetimedb::ProcedureContext,
    file_id: u64,
) -> Result<String, String> {
    let sender = ctx.sender();
    let lookup = ctx.try_with_tx(
        |tx| -> Result<Option<(String, String, bool, bool)>, String> {
            let user = tx
                .db
                .session()
                .connection()
                .find(sender)
                .map(|s| s.user)
                .ok_or_else(|| "sign in first".to_string())?;
            Ok(tx
                .db
                .user_file()
                .id()
                .find(file_id)
                .filter(|f| f.owner == user)
                .map(|f| (f.s3_key, f.hash, f.inline_content.is_some(), f.is_directory)))
        },
    )?;
    let (s3_key, hash, is_inline, is_directory) =
        lookup.ok_or_else(|| "file not found".to_string())?;
    if is_directory {
        return Err("folders do not have download URLs".to_string());
    }
    if is_inline {
        return Err("file is stored inline and does not have a download URL".to_string());
    }
    if hash.is_empty() {
        return Err("file has no recorded hash; finalize the upload first".to_string());
    }

    let cfg = ctx.try_with_tx(|tx| -> Result<Option<crate::config::S3Config>, String> {
        Ok(tx.db.s3_config().id().find(crate::config::S3_CONFIG_ID))
    })?;
    let cfg = cfg
        .filter(|c| {
            !c.bucket.is_empty()
                && !c.region.is_empty()
                && !c.access_key_id.is_empty()
                && !c.secret_access_key.is_empty()
        })
        .ok_or_else(|| "s3 is not configured".to_string())?;

    let client = build_s3_client(&cfg)?;
    let presign = PresigningConfig::expires_in(Duration::from_secs(DOWNLOAD_URL_TTL_SECS))
        .map_err(|e| format!("presign config: {e}"))?;
    let presigned = futures_lite::future::block_on(
        client
            .get_object()
            .bucket(cfg.bucket)
            .key(s3_key)
            .presigned(presign),
    )
    .map_err(|e| format!("presign: {e}"))?;

    Ok(presigned.uri().to_string())
}

#[procedure]
pub fn replace_file_content(
    ctx: &mut spacetimedb::ProcedureContext,
    file_id: u64,
    content_type: Option<String>,
) -> Result<ReplaceTicket, String> {
    let sender = ctx.sender();
    let timestamp = ctx.timestamp;

    let lookup = ctx.try_with_tx(
        |tx| -> Result<Option<(String, crate::config::S3Config)>, String> {
            let user = tx
                .db
                .session()
                .connection()
                .find(sender)
                .map(|s| s.user)
                .ok_or_else(|| "sign in first".to_string())?;
            let file = tx
                .db
                .user_file()
                .id()
                .find(file_id)
                .filter(|f| f.owner == user)
                .ok_or_else(|| "file not found".to_string())?;
            if file.is_directory {
                return Err("folders cannot have file content".to_string());
            }
            let cfg = tx
                .db
                .s3_config()
                .id()
                .find(crate::config::S3_CONFIG_ID)
                .filter(|c| {
                    !c.bucket.is_empty()
                        && !c.region.is_empty()
                        && !c.access_key_id.is_empty()
                        && !c.secret_access_key.is_empty()
                })
                .ok_or_else(|| "s3 is not configured".to_string())?;
            let _ = timestamp;
            Ok(Some((file.s3_key, cfg)))
        },
    )?;
    let (s3_key, cfg) = lookup.ok_or_else(|| "file not found".to_string())?;

    let client = build_s3_client(&cfg)?;

    let presign = PresigningConfig::expires_in(Duration::from_secs(UPLOAD_URL_TTL_SECS))
        .map_err(|e| format!("presign config: {e}"))?;
    let mut req = client.put_object().bucket(cfg.bucket).key(s3_key.clone());
    if let Some(c) = content_type.as_deref() {
        if !c.is_empty() {
            req = req.content_type(c);
        }
    }
    let presigned = futures_lite::future::block_on(req.presigned(presign))
        .map_err(|e| format!("presign: {e}"))?;

    Ok(ReplaceTicket {
        file_id,
        upload_url: presigned.uri().to_string(),
        s3_key,
    })
}
