use aws_credential_types::{Credentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{Client, config::Region, presigning::PresigningConfig};
use aws_types::sdk_config::SdkConfig;
use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, procedure,
    rand::RngCore, view,
};
use std::time::Duration;

use crate::config::s3_config as _;

const UPLOAD_URL_TTL_SECS: u64 = 900;
const DOWNLOAD_URL_TTL_SECS: u64 = 300;

#[spacetimedb::table(accessor = user_file, public)]
pub struct UserFile {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    #[index(btree)]
    pub name: String,
    pub hash: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub s3_key: String,
    pub created_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct FileMetadata {
    pub id: u64,
    pub name: String,
    pub hash: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub s3_key: String,
    pub created_at: Timestamp,
}

impl From<UserFile> for FileMetadata {
    fn from(f: UserFile) -> Self {
        Self {
            id: f.id,
            name: f.name,
            hash: f.hash,
            size_bytes: f.size_bytes,
            content_type: f.content_type,
            s3_key: f.s3_key,
            created_at: f.created_at,
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
        .filter(|c| !c.bucket.is_empty() && !c.region.is_empty())
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
    hash: String,
    size_bytes: u64,
    content_type: Option<String>,
) -> Result<(), String> {
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if hash.is_empty() {
        return Err("hash cannot be empty".to_string());
    }
    load_s3_config(ctx)?;

    let owner = ctx.sender();
    let s3_key = s3_key_for(ctx, owner, &name);
    ctx.db.user_file().insert(UserFile {
        id: 0,
        owner,
        name,
        hash,
        size_bytes,
        content_type,
        s3_key,
        created_at: ctx.timestamp,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_file(ctx: &ReducerContext, file_id: u64) -> Result<(), String> {
    let file = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if file.owner != ctx.sender() {
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
    let mut file = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if file.owner != ctx.sender() {
        return Err("not your file".to_string());
    }
    if hash.is_empty() {
        return Err("hash cannot be empty".to_string());
    }
    file.hash = hash;
    file.size_bytes = size_bytes;
    ctx.db.user_file().id().update(file);
    Ok(())
}

#[spacetimedb::reducer]
pub fn rename_file(ctx: &ReducerContext, file_id: u64, name: String) -> Result<(), String> {
    let mut file = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if file.owner != ctx.sender() {
        return Err("not your file".to_string());
    }
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > 256 {
        return Err("name must be 256 characters or fewer".to_string());
    }
    file.name = name;
    ctx.db.user_file().id().update(file);
    Ok(())
}

#[view(accessor = my_files, public)]
fn my_files(ctx: &ViewContext) -> Vec<FileMetadata> {
    ctx.db
        .user_file()
        .owner()
        .filter(ctx.sender())
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
    let mut builder = SdkConfig::builder()
        .region(Region::new(cfg.region.clone()))
        .credentials_provider(SharedCredentialsProvider::new(creds));
    if let Some(ep) = cfg.endpoint.as_deref() {
        builder = builder.endpoint_url(ep);
    }
    Ok(Client::new(&builder.build()))
}

#[procedure]
pub fn get_file(
    ctx: &mut spacetimedb::ProcedureContext,
    file_id: u64,
) -> Result<Option<FileMetadata>, String> {
    let sender = ctx.sender();
    ctx.try_with_tx(|tx| -> Result<Option<FileMetadata>, String> {
        Ok(tx
            .db
            .user_file()
            .id()
            .find(file_id)
            .filter(|f| f.owner == sender)
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
        Ok(tx
            .db
            .user_file()
            .owner()
            .filter(sender)
            .filter(|f| f.name.to_lowercase().contains(&q))
            .map(FileMetadata::from)
            .collect::<Vec<_>>())
    })
}

#[procedure]
pub fn request_upload_url(
    ctx: &mut spacetimedb::ProcedureContext,
    name: String,
    content_type: Option<String>,
) -> Result<UploadTicket, String> {
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }

    let sender = ctx.sender();
    let nonce: u32 = ctx.rng().next_u32();
    let safe = sanitize(&name);
    let timestamp = ctx.timestamp;

    let prepared = ctx.try_with_tx(
        |tx| -> Result<(crate::config::S3Config, u64, String, Option<String>), String> {
            let cfg = tx
                .db
                .s3_config()
                .id()
                .find(crate::config::S3_CONFIG_ID)
                .filter(|c| !c.bucket.is_empty() && !c.region.is_empty())
                .ok_or_else(|| "s3 is not configured".to_string())?;
            let prefix = cfg
                .path_prefix
                .as_deref()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_string();
            let s3_key = if prefix.is_empty() {
                format!("users/{sender}/{nonce:08x}-{safe}")
            } else {
                format!("{prefix}/users/{sender}/{nonce:08x}-{safe}")
            };
            let inserted = tx.db.user_file().insert(UserFile {
                id: 0,
                owner: sender,
                name: name.clone(),
                hash: String::new(),
                size_bytes: 0,
                content_type: content_type.clone(),
                s3_key: s3_key.clone(),
                created_at: timestamp,
            });
            Ok((cfg, inserted.id, s3_key, inserted.content_type))
        },
    )?;

    let (cfg, file_id, s3_key, ct) = prepared;

    let creds = Credentials::new(
        cfg.access_key_id,
        cfg.secret_access_key,
        None,
        None,
        "spacenix-static",
    );
    let mut builder = SdkConfig::builder()
        .region(Region::new(cfg.region))
        .credentials_provider(SharedCredentialsProvider::new(creds));
    if let Some(ep) = cfg.endpoint {
        if !ep.is_empty() {
            builder = builder.endpoint_url(ep);
        }
    }
    let client = Client::new(&builder.build());

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
    let lookup = ctx.try_with_tx(|tx| -> Result<Option<(String, String)>, String> {
        Ok(tx
            .db
            .user_file()
            .id()
            .find(file_id)
            .filter(|f| f.owner == sender)
            .map(|f| (f.s3_key, f.hash)))
    })?;
    let (s3_key, hash) = lookup.ok_or_else(|| "file not found".to_string())?;
    if hash.is_empty() {
        return Err("file has no recorded hash; finalize the upload first".to_string());
    }

    let cfg = ctx.try_with_tx(|tx| -> Result<Option<crate::config::S3Config>, String> {
        Ok(tx.db.s3_config().id().find(crate::config::S3_CONFIG_ID))
    })?;
    let cfg = cfg
        .filter(|c| !c.bucket.is_empty() && !c.region.is_empty())
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

    let lookup = ctx.try_with_tx(|tx| -> Result<Option<(String, crate::config::S3Config)>, String> {
        let file = tx
            .db
            .user_file()
            .id()
            .find(file_id)
            .filter(|f| f.owner == sender)
            .ok_or_else(|| "file not found".to_string())?;
        let cfg = tx
            .db
            .s3_config()
            .id()
            .find(crate::config::S3_CONFIG_ID)
            .filter(|c| !c.bucket.is_empty() && !c.region.is_empty())
            .ok_or_else(|| "s3 is not configured".to_string())?;
        let _ = timestamp;
        Ok(Some((file.s3_key, cfg)))
    })?;
    let (s3_key, cfg) = lookup.ok_or_else(|| "file not found".to_string())?;

    let creds = Credentials::new(
        cfg.access_key_id,
        cfg.secret_access_key,
        None,
        None,
        "spacenix-static",
    );
    let mut builder = SdkConfig::builder()
        .region(Region::new(cfg.region))
        .credentials_provider(SharedCredentialsProvider::new(creds));
    if let Some(ep) = cfg.endpoint {
        if !ep.is_empty() {
            builder = builder.endpoint_url(ep);
        }
    }
    let client = Client::new(&builder.build());

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
