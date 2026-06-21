use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, procedure,
    rand::RngCore, view,
};
use std::time::Duration;

use crate::config::s3_config as _;
use crate::user::{require_registered_user, session as _, session__view as _};

type HmacSha256 = Hmac<Sha256>;

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
    /// Virtual SpaceNix path used to nest this row inside folders in the
    /// tree view. Relative segments joined with `/`; `None` or `""` means
    /// the row lives at the root of the user's tree.
    pub tree_path: Option<String>,
    /// Absolute path on the local filesystem that this row (or its parent
    /// folder, for files inside) syncs to. `None` means the sync agent has
    /// not been told a destination yet.
    pub local_path: Option<String>,
    pub hash: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub is_directory: bool,
    pub s3_key: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct FileMetadata {
    pub id: u64,
    pub name: String,
    pub tree_path: Option<String>,
    pub local_path: Option<String>,
    pub hash: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
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
            tree_path: f.tree_path,
            local_path: f.local_path,
            hash: f.hash,
            size_bytes: f.size_bytes,
            content_type: f.content_type,
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

fn validate_tree_path(path: Option<String>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim().trim_end_matches('/').to_string();
    if path.is_empty() {
        return Ok(None);
    }
    if path.len() > 1024 {
        return Err("tree path must be 1024 characters or fewer".to_string());
    }
    if path.starts_with('/') {
        return Err("tree path must be relative (no leading slash)".to_string());
    }
    if path.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("tree path cannot contain control characters".to_string());
    }
    Ok(Some(path))
}

/// Validates a `local_path`: the absolute path on the user's local
/// filesystem that a row (or its containing folder) syncs to. `None` or
/// empty means the sync agent has not been told a destination yet.
fn validate_local_path(path: Option<String>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim().to_string();
    if path.is_empty() {
        return Ok(None);
    }
    if path.len() > 4096 {
        return Err("local path must be 4096 characters or fewer".to_string());
    }
    if !path.starts_with('/') {
        return Err("local path must be absolute (start with '/')".to_string());
    }
    if path.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("local path cannot contain control characters".to_string());
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
        .any(|f| f.tree_path.as_deref() == Some(path) && Some(f.id) != except_id)
}

/// Returns true if `prefix` is a path prefix of `path` (i.e. `path == prefix`
/// or `path` starts with `prefix + "/"`). Used to detect a folder being moved
/// into its own subtree.
fn has_path_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Detects moves that would create a containment conflict — i.e. moving a
/// folder `old_path` to `new_path` where `new_path` is already inside
/// `old_path`. Returns `Err` with a user-facing message in that case.
fn check_move_containment(
    ctx: &ReducerContext,
    owner: Identity,
    file_id: u64,
    old_path: Option<&str>,
    new_path: Option<&str>,
) -> Result<(), String> {
    let (Some(old), Some(new)) = (old_path, new_path) else {
        return Ok(());
    };
    if old == new {
        return Ok(());
    }
    if has_path_prefix(new, old) {
        return Err("cannot move a folder into itself".to_string());
    }
    // If the moved row is a directory, every row currently at a path that is
    // a descendant of `old` will be rewritten to live under `new`. None of
    // those descendants can already be at `new` (or under it) in a way that
    // would collide after the rewrite.
    let moved = ctx
        .db
        .user_file()
        .id()
        .find(file_id)
        .ok_or_else(|| "file not found".to_string())?;
    if !moved.is_directory {
        return Ok(());
    }
    let old_prefix = format!("{old}/");
    let new_prefix = format!("{new}/");
    for descendant in ctx
        .db
        .user_file()
        .owner()
        .filter(owner)
        .filter(|f| Some(f.id) != Some(file_id))
    {
        let Some(child_path) = descendant.tree_path.as_deref() else {
            continue;
        };
        if !child_path.starts_with(&old_prefix) {
            continue;
        }
        // The descendant's new path will be `new/<rest>`.
        let rewritten = format!("{new}{}", &child_path[old.len()..]);
        if has_path_prefix(&rewritten, &new_prefix) {
            // Only possible if `new` is itself under the descendant — covered
            // by the earlier `has_path_prefix(new, old)` check, but keep
            // for safety.
            return Err("cannot move a folder into itself".to_string());
        }
        if path_conflict(ctx, owner, &Some(rewritten.clone()), Some(descendant.id)) {
            return Err("a file or folder already uses this path".to_string());
        }
    }
    Ok(())
}

/// Rewrites the `path` of every descendant of `old_prefix` (the directory
/// being moved) to its new location under `new_prefix`. Returns the number of
/// rows updated.
fn rewrite_descendant_paths(
    ctx: &ReducerContext,
    owner: Identity,
    old_prefix: &str,
    new_prefix: &str,
) -> usize {
    let old_with_slash = format!("{old_prefix}/");
    let mut updated = 0;
    for row in ctx.db.user_file().owner().filter(owner) {
        let Some(child_path) = row.tree_path.as_deref() else {
            continue;
        };
        if !child_path.starts_with(&old_with_slash) {
            continue;
        }
        let suffix = &child_path[old_prefix.len()..];
        let new_path = format!("{new_prefix}{suffix}");
        let mut next = row;
        next.tree_path = Some(new_path);
        next.updated_at = ctx.timestamp;
        ctx.db.user_file().id().update(next);
        updated += 1;
    }
    updated
}

/// Deletes every descendant of `prefix` (rows whose `path` starts with
/// `prefix + "/"`). Does not touch the row at `prefix` itself. Returns the
/// number of rows deleted.
fn delete_descendants(
    ctx: &ReducerContext,
    owner: Identity,
    prefix: &str,
) -> usize {
    let with_slash = format!("{prefix}/");
    let to_delete: Vec<u64> = ctx
        .db
        .user_file()
        .owner()
        .filter(owner)
        .filter(|f| {
            f.tree_path
                .as_deref()
                .is_some_and(|p| p.starts_with(&with_slash))
        })
        .map(|f| f.id)
        .collect();
    let n = to_delete.len();
    for id in to_delete {
        ctx.db.user_file().id().delete(id);
    }
    n
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
    tree_path: Option<String>,
    local_path: Option<String>,
    hash: String,
    size_bytes: u64,
    content_type: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_file_name(name)?;
    let tree_path = validate_tree_path(tree_path)?;
    let local_path = validate_local_path(local_path)?;
    if hash.is_empty() {
        return Err("hash cannot be empty".to_string());
    }
    load_s3_config(ctx)?;

    let owner = user.identity;
    if path_conflict(ctx, owner, &tree_path, None) {
        return Err("a file or folder already uses this path".to_string());
    }
    let s3_key = s3_key_for(ctx, owner, &name);
    ctx.db.user_file().insert(UserFile {
        id: 0,
        owner,
        name,
        tree_path,
        local_path,
        hash,
        size_bytes,
        content_type,
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
    tree_path: Option<String>,
    local_path: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_file_name(name)?;
    let tree_path = validate_tree_path(tree_path)?;
    let local_path = validate_local_path(local_path)?;
    let owner = user.identity;
    if path_conflict(ctx, owner, &tree_path, None) {
        return Err("a file or folder already uses this path".to_string());
    }
    ctx.db.user_file().insert(UserFile {
        id: 0,
        owner,
        name,
        tree_path,
        local_path,
        hash: String::new(),
        size_bytes: 0,
        content_type: None,
        is_directory: true,
        s3_key: String::new(),
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
    });
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
    // Cascade: if the row is a directory, remove every descendant as well so
    // the tree never ends up with ghost children pointing at a missing
    // parent. The descendant rows are looked up by `tree_path` prefix after
    // removing the directory itself, so the deletion is symmetric and order
    // does not matter.
    let owner = user.identity;
    let prefix = file.tree_path.clone();
    ctx.db.user_file().id().delete(file_id);
    if file.is_directory {
        if let Some(p) = prefix.as_deref() {
            delete_descendants(ctx, owner, p);
        }
    }
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
    file.updated_at = ctx.timestamp;
    ctx.db.user_file().id().update(file);
    Ok(())
}

#[spacetimedb::reducer]
pub fn rename_file(
    ctx: &ReducerContext,
    file_id: u64,
    name: String,
    tree_path: Option<String>,
    local_path: Option<String>,
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
    let tree_path = validate_tree_path(tree_path)?;
    let local_path = validate_local_path(local_path)?;
    if path_conflict(ctx, user.identity, &tree_path, Some(file_id)) {
        return Err("a file or folder already uses this path".to_string());
    }
    // Reject moving a folder into itself or any of its descendants, and
    // verify that no descendant, once rewritten, would collide with an
    // existing row. This prevents the "ghost folder" pattern that arises
    // when descendants are left at their old paths after a folder move.
    if file.is_directory {
        check_move_containment(
            ctx,
            user.identity,
            file_id,
            file.tree_path.as_deref(),
            tree_path.as_deref(),
        )?;
    }
    let is_directory = file.is_directory;
    let old_path = file.tree_path.clone();
    file.name = name;
    file.tree_path = tree_path.clone();
    file.local_path = local_path;
    file.updated_at = ctx.timestamp;
    ctx.db.user_file().id().update(file);
    if is_directory {
        if let (Some(old), Some(new)) = (old_path.as_deref(), tree_path.as_deref()) {
            if old != new {
                rewrite_descendant_paths(ctx, user.identity, old, new);
            }
        }
    }
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

fn presign_url(
    method: &str,
    cfg: &crate::config::S3Config,
    key: &str,
    _content_type: Option<&str>,
    expires_in: Duration,
    now_micros: i64,
) -> Result<String, String> {
    let (date_stamp, amz_date) = format_amz_date(now_micros);

    let (scheme, host, path_prefix) = match cfg.endpoint.as_deref() {
        Some(ep) if !ep.is_empty() => {
            let (scheme, rest) = ep
                .strip_prefix("https://")
                .map(|r| ("https", r))
                .or_else(|| ep.strip_prefix("http://").map(|r| ("http", r)))
                .unwrap_or(("https", ep));
            (
                scheme.to_string(),
                rest.trim_end_matches('/').to_string(),
                format!("/{}", cfg.bucket),
            )
        }
        _ => {
            let host = format!("{}.s3.{}.amazonaws.com", cfg.bucket, cfg.region);
            ("https".to_string(), host, String::new())
        }
    };

    let canonical_uri = if path_prefix.is_empty() {
        format!("/{}", uri_encode(key, false))
    } else {
        format!("{}/{}", path_prefix, uri_encode(key, false))
    };

    let canonical_headers = format!("host:{}\n", host);
    let signed_headers = "host";

    let credential = format!(
        "{}/{}/{}/s3/aws4_request",
        cfg.access_key_id, date_stamp, cfg.region
    );
    let expires_secs = expires_in.as_secs().to_string();

    let mut query_pairs: Vec<(String, String)> = vec![
        ("X-Amz-Algorithm".to_string(), "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential".to_string(), credential),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), expires_secs),
        ("X-Amz-SignedHeaders".to_string(), signed_headers.to_string()),
    ];
    query_pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_query_string = query_pairs
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\nUNSIGNED-PAYLOAD",
        method,
        canonical_uri,
        canonical_query_string,
        canonical_headers,
        signed_headers
    );

    let hashed_canonical = sha256_hex(canonical_request.as_bytes());
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}/{}/s3/aws4_request\n{}",
        amz_date, date_stamp, cfg.region, hashed_canonical
    );

    let signing_key = derive_signing_key(&cfg.secret_access_key, &date_stamp, &cfg.region);
    let signature_bytes = hmac_sha256(&signing_key, string_to_sign.as_bytes());
    let signature_hex: String = signature_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    Ok(format!(
        "{}://{}{}?{}&X-Amz-Signature={}",
        scheme, host, canonical_uri, canonical_query_string, signature_hex
    ))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn derive_signing_key(secret: &str, date_stamp: &str, region: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{}", secret).as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

fn uri_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b'/' if !encode_slash => out.push('/'),
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
    out
}

fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + n - 10) as char,
        _ => unreachable!(),
    }
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn format_amz_date(micros: i64) -> (String, String) {
    let secs = micros / 1_000_000;
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let h = tod / 3600;
    let min = (tod % 3600) / 60;
    let s = tod % 60;
    let date_stamp = format!("{:04}{:02}{:02}", y, m, d);
    let amz_date = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        y, m, d, h, min, s
    );
    (date_stamp, amz_date)
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
                    || f.tree_path
                        .as_deref()
                        .is_some_and(|path| path.to_lowercase().contains(&q))
                    || f.local_path
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
    tree_path: Option<String>,
    local_path: Option<String>,
    content_type: Option<String>,
) -> Result<UploadTicket, String> {
    let name = validate_file_name(name)?;
    let tree_path = validate_tree_path(tree_path)?;
    let local_path = validate_local_path(local_path)?;

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
            if let Some(target_path) = tree_path.as_deref() {
                if tx
                    .db
                    .user_file()
                    .owner()
                    .filter(user)
                    .any(|f| f.tree_path.as_deref() == Some(target_path))
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
                tree_path: tree_path.clone(),
                local_path: local_path.clone(),
                hash: String::new(),
                size_bytes: 0,
                content_type: content_type.clone(),
                is_directory: false,
                s3_key: s3_key.clone(),
                created_at: timestamp,
                updated_at: timestamp,
            });
            Ok((cfg, inserted.id, s3_key, inserted.content_type))
        },
    )?;

    let (cfg, file_id, s3_key, ct) = prepared;

    let upload_url = presign_url(
        "PUT",
        &cfg,
        &s3_key,
        ct.as_deref(),
        Duration::from_secs(UPLOAD_URL_TTL_SECS),
        ctx.timestamp.to_micros_since_unix_epoch(),
    )?;

    Ok(UploadTicket {
        file_id,
        upload_url,
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
        |tx| -> Result<Option<(String, String, bool)>, String> {
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
                .map(|f| (f.s3_key, f.hash, f.is_directory)))
        },
    )?;
    let (s3_key, hash, is_directory) =
        lookup.ok_or_else(|| "file not found".to_string())?;
    if is_directory {
        return Err("folders do not have download URLs".to_string());
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

    let url = presign_url(
        "GET",
        &cfg,
        &s3_key,
        None,
        Duration::from_secs(DOWNLOAD_URL_TTL_SECS),
        ctx.timestamp.to_micros_since_unix_epoch(),
    )?;

    Ok(url)
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

    let upload_url = presign_url(
        "PUT",
        &cfg,
        &s3_key,
        content_type.as_deref(),
        Duration::from_secs(UPLOAD_URL_TTL_SECS),
        ctx.timestamp.to_micros_since_unix_epoch(),
    )?;

    Ok(ReplaceTicket {
        file_id,
        upload_url,
        s3_key,
    })
}
