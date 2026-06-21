use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, procedure, view,
};

use crate::user::{require_registered_user, session as _, session__view as _};

const NAME_MAX_LEN: usize = 128;
const FINGERPRINT_MAX_LEN: usize = 128;
const HOST_MAX_LEN: usize = 256;
const USERNAME_MAX_LEN: usize = 128;
const PUBLIC_KEY_MAX_LEN: usize = 16384;
const PRIVATE_KEY_MAX_LEN: usize = 16384;
const TAG_MAX_LEN: usize = 128;
const TAG_MAX_COUNT: usize = 64;
const DEVICE_ID_MAX_COUNT: usize = 64;

#[spacetimedb::table(accessor = ssh_key)]
pub struct SshKey {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    pub name: String,
    pub public_key: String,
    pub private_key: String,
    pub fingerprint: String,
    pub device_ids: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SshKeyMetadata {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub device_ids: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SshKeyValue {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub public_key: String,
    pub private_key: String,
    pub fingerprint: String,
    pub device_ids: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl From<SshKey> for SshKeyMetadata {
    fn from(k: SshKey) -> Self {
        Self {
            id: k.id,
            owner: k.owner,
            name: k.name,
            public_key: k.public_key,
            fingerprint: k.fingerprint,
            device_ids: k.device_ids,
            tags: k.tags,
            created_at: k.created_at,
            updated_at: k.updated_at,
        }
    }
}

#[spacetimedb::table(accessor = ssh_endpoint)]
pub struct SshEndpoint {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[index(btree)]
    pub key_id: u64,
    pub device_ids: Vec<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SshEndpointMetadata {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub key_id: u64,
    pub device_ids: Vec<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl From<SshEndpoint> for SshEndpointMetadata {
    fn from(e: SshEndpoint) -> Self {
        Self {
            id: e.id,
            owner: e.owner,
            name: e.name,
            host: e.host,
            port: e.port,
            username: e.username,
            key_id: e.key_id,
            device_ids: e.device_ids,
            tags: e.tags,
            enabled: e.enabled,
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

fn validate_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > NAME_MAX_LEN {
        return Err(format!("name must be {NAME_MAX_LEN} characters or fewer"));
    }
    Ok(name)
}

fn validate_host(host: String) -> Result<String, String> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err("host cannot be empty".to_string());
    }
    if host.len() > HOST_MAX_LEN {
        return Err(format!("host must be {HOST_MAX_LEN} characters or fewer"));
    }
    Ok(host)
}

fn validate_username(username: String) -> Result<String, String> {
    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("username cannot be empty".to_string());
    }
    if username.len() > USERNAME_MAX_LEN {
        return Err(format!(
            "username must be {USERNAME_MAX_LEN} characters or fewer"
        ));
    }
    Ok(username)
}

fn validate_port(port: u16) -> Result<u16, String> {
    if port == 0 {
        return Err("port must be 1..=65535".to_string());
    }
    Ok(port)
}

fn validate_key_material(public_key: &str, private_key: &str) -> Result<(String, String), String> {
    let public_key = public_key.trim().to_string();
    if public_key.is_empty() {
        return Err("public_key cannot be empty".to_string());
    }
    if public_key.len() > PUBLIC_KEY_MAX_LEN {
        return Err(format!(
            "public_key must be {PUBLIC_KEY_MAX_LEN} characters or fewer"
        ));
    }
    let private_key = private_key.trim().to_string();
    if private_key.is_empty() {
        return Err("private_key cannot be empty".to_string());
    }
    if private_key.len() > PRIVATE_KEY_MAX_LEN {
        return Err(format!(
            "private_key must be {PRIVATE_KEY_MAX_LEN} characters or fewer"
        ));
    }
    Ok((public_key, private_key))
}

fn validate_device_ids(device_ids: &[String]) -> Result<(), String> {
    if device_ids.len() > DEVICE_ID_MAX_COUNT {
        return Err(format!(
            "too many device_ids (max {DEVICE_ID_MAX_COUNT})"
        ));
    }
    for d in device_ids {
        if d.is_empty() || d.len() > TAG_MAX_LEN {
            return Err("device_id must be 1..=128 chars".to_string());
        }
    }
    Ok(())
}

fn normalize_tags(mut tags: Vec<String>) -> Result<Vec<String>, String> {
    if tags.len() > TAG_MAX_COUNT {
        return Err(format!("too many tags (max {TAG_MAX_COUNT})"));
    }
    for t in &mut tags {
        *t = t.trim().to_string();
        if t.is_empty() {
            return Err("tag cannot be empty".to_string());
        }
        if t.len() > TAG_MAX_LEN {
            return Err(format!("tag must be {TAG_MAX_LEN} characters or fewer"));
        }
        if !t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-' | '*'))
        {
            return Err("tag contains invalid characters".to_string());
        }
    }
    tags.sort();
    tags.dedup();
    Ok(tags)
}

fn compute_fingerprint(public_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(public_key.as_bytes());
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2 + 4);
    out.push_str("SHA256:");
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    if out.len() > FINGERPRINT_MAX_LEN {
        out.truncate(FINGERPRINT_MAX_LEN);
    }
    out
}

fn require_owned_key(ctx: &ReducerContext, id: u64) -> Result<SshKey, String> {
    let user = require_registered_user(ctx)?;
    let key = ctx
        .db
        .ssh_key()
        .id()
        .find(id)
        .ok_or_else(|| "ssh key not found".to_string())?;
    if key.owner != user.identity {
        return Err("not your ssh key".to_string());
    }
    Ok(key)
}

fn require_owned_endpoint(ctx: &ReducerContext, id: u64) -> Result<SshEndpoint, String> {
    let user = require_registered_user(ctx)?;
    let ep = ctx
        .db
        .ssh_endpoint()
        .id()
        .find(id)
        .ok_or_else(|| "ssh endpoint not found".to_string())?;
    if ep.owner != user.identity {
        return Err("not your ssh endpoint".to_string());
    }
    Ok(ep)
}

#[spacetimedb::reducer]
pub fn set_ssh_key(
    ctx: &ReducerContext,
    name: String,
    public_key: String,
    private_key: String,
    device_ids: Vec<String>,
    tags: Vec<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_name(name)?;
    let (public_key, private_key) = validate_key_material(&public_key, &private_key)?;
    validate_device_ids(&device_ids)?;
    let tags = normalize_tags(tags)?;
    let fingerprint = compute_fingerprint(&public_key);
    let owner = user.identity;

    if let Some(prev) = ctx
        .db
        .ssh_key()
        .owner()
        .filter(owner)
        .find(|k| k.name == name)
    {
        let updated = SshKey {
            public_key,
            private_key,
            fingerprint,
            device_ids,
            tags,
            updated_at: ctx.timestamp,
            ..prev
        };
        ctx.db.ssh_key().id().update(updated);
    } else {
        ctx.db.ssh_key().insert(SshKey {
            id: 0,
            owner,
            name,
            public_key,
            private_key,
            fingerprint,
            device_ids,
            tags,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
        });
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_key_value(
    ctx: &ReducerContext,
    id: u64,
    public_key: String,
    private_key: String,
) -> Result<(), String> {
    let key = require_owned_key(ctx, id)?;
    let (public_key, private_key) = validate_key_material(&public_key, &private_key)?;
    let fingerprint = compute_fingerprint(&public_key);
    ctx.db.ssh_key().id().update(SshKey {
        public_key,
        private_key,
        fingerprint,
        updated_at: ctx.timestamp,
        ..key
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_key_devices(
    ctx: &ReducerContext,
    id: u64,
    device_ids: Vec<String>,
) -> Result<(), String> {
    validate_device_ids(&device_ids)?;
    let key = require_owned_key(ctx, id)?;
    ctx.db.ssh_key().id().update(SshKey {
        device_ids,
        updated_at: ctx.timestamp,
        ..key
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_key_tags(ctx: &ReducerContext, id: u64, tags: Vec<String>) -> Result<(), String> {
    let tags = normalize_tags(tags)?;
    let key = require_owned_key(ctx, id)?;
    ctx.db.ssh_key().id().update(SshKey {
        tags,
        updated_at: ctx.timestamp,
        ..key
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_ssh_key(ctx: &ReducerContext, id: u64) -> Result<(), String> {
    let key = require_owned_key(ctx, id)?;
    let referencing: Vec<u64> = ctx
        .db
        .ssh_endpoint()
        .owner()
        .filter(key.owner)
        .filter(|e| e.key_id == key.id)
        .map(|e| e.id)
        .collect();
    if !referencing.is_empty() {
        return Err(format!(
            "cannot delete: still referenced by {} ssh endpoint(s)",
            referencing.len()
        ));
    }
    ctx.db.ssh_key().id().delete(id);
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_endpoint(
    ctx: &ReducerContext,
    name: String,
    host: String,
    port: u16,
    username: String,
    key_id: u64,
    device_ids: Vec<String>,
    tags: Vec<String>,
    enabled: bool,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_name(name)?;
    let host = validate_host(host)?;
    let port = validate_port(port)?;
    let username = validate_username(username)?;
    validate_device_ids(&device_ids)?;
    let tags = normalize_tags(tags)?;

    let key = ctx
        .db
        .ssh_key()
        .id()
        .find(key_id)
        .ok_or_else(|| "ssh key not found".to_string())?;
    if key.owner != user.identity {
        return Err("not your ssh key".to_string());
    }

    let owner = user.identity;
    if let Some(prev) = ctx
        .db
        .ssh_endpoint()
        .owner()
        .filter(owner)
        .find(|e| e.name == name)
    {
        let updated = SshEndpoint {
            host,
            port,
            username,
            key_id,
            device_ids,
            tags,
            enabled,
            updated_at: ctx.timestamp,
            ..prev
        };
        ctx.db.ssh_endpoint().id().update(updated);
    } else {
        ctx.db.ssh_endpoint().insert(SshEndpoint {
            id: 0,
            owner,
            name,
            host,
            port,
            username,
            key_id,
            device_ids,
            tags,
            enabled,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
        });
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn update_ssh_endpoint(
    ctx: &ReducerContext,
    id: u64,
    host: String,
    port: u16,
    username: String,
    key_id: u64,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let host = validate_host(host)?;
    let port = validate_port(port)?;
    let username = validate_username(username)?;
    let key = ctx
        .db
        .ssh_key()
        .id()
        .find(key_id)
        .ok_or_else(|| "ssh key not found".to_string())?;
    if key.owner != user.identity {
        return Err("not your ssh key".to_string());
    }
    let ep = require_owned_endpoint(ctx, id)?;
    ctx.db.ssh_endpoint().id().update(SshEndpoint {
        host,
        port,
        username,
        key_id,
        updated_at: ctx.timestamp,
        ..ep
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_endpoint_devices(
    ctx: &ReducerContext,
    id: u64,
    device_ids: Vec<String>,
) -> Result<(), String> {
    validate_device_ids(&device_ids)?;
    let ep = require_owned_endpoint(ctx, id)?;
    ctx.db.ssh_endpoint().id().update(SshEndpoint {
        device_ids,
        updated_at: ctx.timestamp,
        ..ep
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_endpoint_tags(
    ctx: &ReducerContext,
    id: u64,
    tags: Vec<String>,
) -> Result<(), String> {
    let tags = normalize_tags(tags)?;
    let ep = require_owned_endpoint(ctx, id)?;
    ctx.db.ssh_endpoint().id().update(SshEndpoint {
        tags,
        updated_at: ctx.timestamp,
        ..ep
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_ssh_endpoint_enabled(
    ctx: &ReducerContext,
    id: u64,
    enabled: bool,
) -> Result<(), String> {
    let ep = require_owned_endpoint(ctx, id)?;
    ctx.db.ssh_endpoint().id().update(SshEndpoint {
        enabled,
        updated_at: ctx.timestamp,
        ..ep
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_ssh_endpoint(ctx: &ReducerContext, id: u64) -> Result<(), String> {
    require_owned_endpoint(ctx, id)?;
    ctx.db.ssh_endpoint().id().delete(id);
    Ok(())
}

#[view(accessor = my_ssh_keys, public)]
fn my_ssh_keys(ctx: &ViewContext) -> Vec<SshKeyMetadata> {
    let Some(user) = ctx.db.session().connection().find(ctx.sender()).map(|s| s.user) else {
        return Vec::new();
    };
    ctx.db
        .ssh_key()
        .owner()
        .filter(user)
        .map(SshKeyMetadata::from)
        .collect()
}

#[view(accessor = my_ssh_endpoints, public)]
fn my_ssh_endpoints(ctx: &ViewContext) -> Vec<SshEndpointMetadata> {
    let Some(user) = ctx.db.session().connection().find(ctx.sender()).map(|s| s.user) else {
        return Vec::new();
    };
    ctx.db
        .ssh_endpoint()
        .owner()
        .filter(user)
        .map(SshEndpointMetadata::from)
        .collect()
}

#[procedure]
pub fn reveal_ssh_key(
    ctx: &mut spacetimedb::ProcedureContext,
    id: u64,
) -> Result<Option<SshKeyValue>, String> {
    let sender = ctx.sender();
    ctx.try_with_tx(|tx| -> Result<Option<SshKeyValue>, String> {
        let user = tx
            .db
            .session()
            .connection()
            .find(sender)
            .map(|s| s.user)
            .ok_or_else(|| "sign in first".to_string())?;
        Ok(tx
            .db
            .ssh_key()
            .id()
            .find(id)
            .filter(|k| k.owner == user)
            .map(|k| SshKeyValue {
                id: k.id,
                owner: k.owner,
                name: k.name,
                public_key: k.public_key,
                private_key: k.private_key,
                fingerprint: k.fingerprint,
                device_ids: k.device_ids,
                tags: k.tags,
                created_at: k.created_at,
                updated_at: k.updated_at,
            }))
    })
}
