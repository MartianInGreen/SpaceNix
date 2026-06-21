use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, TimeDuration, Timestamp, ViewContext,
    procedure, view,
};

use crate::user::{require_registered_user, session as _, session__view as _};
use crate::device::device as _;

const NAME_MAX_LEN: usize = 128;
const FINGERPRINT_MAX_LEN: usize = 128;
const HOST_MAX_LEN: usize = 256;
const USERNAME_MAX_LEN: usize = 128;
const PUBLIC_KEY_MAX_LEN: usize = 16384;
const PRIVATE_KEY_MAX_LEN: usize = 16384;
const TAG_MAX_LEN: usize = 128;
const TAG_MAX_COUNT: usize = 64;
const DEVICE_ID_MAX_COUNT: usize = 64;
const AUTH_TOKEN_MAX_LEN: usize = 128;
const SESSION_MAX_LIFETIME_MICROS: i64 = 60 * 60 * 1_000_000;

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

// ---------------------------------------------------------------------------
// Browser SSH relay
// ---------------------------------------------------------------------------
//
// Lets a signed-in browser open an interactive SSH session to a registered
// endpoint by going through one of the user's devices running
// `spacenix service start` (the "relay"). The browser opens a
// WebSocket to the relay's local HTTP service, the service validates
// a per-session token that was minted by the relay and stored in
// `SshRelaySession.auth_token`, then spawns `ssh(1)` in a pty and
// bridges bytes.
//
// The relay device and the browser both connect to SpacetimeDB as
// the same user identity; STDB acts purely as the coordination
// surface (which device is the relay, which session is open, what
// the per-session auth token is). The actual SSH traffic never
// touches the database.

#[derive(SpacetimeType, Clone, Debug, PartialEq, Eq)]
pub enum SshRelaySessionStatus {
    /// Session row exists, no token attached yet, no ssh running.
    Pending,
    /// Relay has minted a token and the browser is connecting.
    Active,
    /// Closed by either side. Row kept briefly to drop late WS
    /// connections, cleaned up by the close reducer.
    Closed,
}

/// Designates one of the caller's devices as the SSH relay for browser
/// sessions. Singleton per owner (primary key = owner).
#[spacetimedb::table(accessor = ssh_relay_device, public)]
pub struct SshRelayDevice {
    #[primary_key]
    pub owner: Identity,
    pub device_id: u64,
    pub updated_at: Timestamp,
    /// Address the browser can use to reach the relay's HTTP/WS
    /// service. Set via `set_ssh_relay_device_url` from the Devices
    /// page. Examples: `ws://laptop.lan:7770`,
    /// `wss://my-laptop.tail-net.ts.net:7770`.
    pub listen_url: Option<String>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SshRelayDeviceMetadata {
    pub owner: Identity,
    pub device_id: u64,
    pub updated_at: Timestamp,
    pub listen_url: Option<String>,
}

impl From<SshRelayDevice> for SshRelayDeviceMetadata {
    fn from(d: SshRelayDevice) -> Self {
        Self {
            owner: d.owner,
            device_id: d.device_id,
            updated_at: d.updated_at,
            listen_url: d.listen_url,
        }
    }
}

#[spacetimedb::table(accessor = ssh_relay_session, public)]
pub struct SshRelaySession {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    /// The device the browser is running on. Optional because the
    /// browser doesn't always know its `device.id`; the relay uses
    /// it only for diagnostics.
    pub requester_device_id: Option<u64>,
    /// The device that will spawn `ssh(1)`. Must reference a device
    /// owned by the caller and currently set as the relay via
    /// `ssh_relay_device`.
    #[index(btree)]
    pub relay_device_id: u64,
    /// The endpoint the user wants to connect to. Must be owned by
    /// the caller, must be enabled.
    #[index(btree)]
    pub endpoint_id: u64,
    pub status: SshRelaySessionStatus,
    pub created_at: Timestamp,
    /// Hard cap on the session's lifetime. Relay and browser both
    /// tear down at or after this point.
    pub expires_at: Timestamp,
    /// Opaque bearer token the browser must present when opening the
    /// WebSocket. Minted by the relay device (see
    /// `attach_ssh_relay_session_token`) so the session is bound to
    /// the very same token both sides agreed on, and not forged by
    /// the row's creator.
    pub auth_token: Option<String>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SshRelaySessionMetadata {
    pub id: u64,
    pub owner: Identity,
    pub requester_device_id: Option<u64>,
    pub relay_device_id: u64,
    pub endpoint_id: u64,
    pub status: SshRelaySessionStatus,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub auth_token: Option<String>,
}

impl From<SshRelaySession> for SshRelaySessionMetadata {
    fn from(s: SshRelaySession) -> Self {
        Self {
            id: s.id,
            owner: s.owner,
            requester_device_id: s.requester_device_id,
            relay_device_id: s.relay_device_id,
            endpoint_id: s.endpoint_id,
            status: s.status,
            created_at: s.created_at,
            expires_at: s.expires_at,
            auth_token: s.auth_token,
        }
    }
}

fn require_owned_device(ctx: &ReducerContext, id: u64) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let device = ctx
        .db
        .device()
        .id()
        .find(id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    Ok(())
}

fn require_owned_endpoint_visible(ctx: &ReducerContext, id: u64) -> Result<SshEndpoint, String> {
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

fn validate_auth_token(token: String) -> Result<String, String> {
    if token.len() < 16 {
        return Err("auth_token must be at least 16 characters".to_string());
    }
    if token.len() > AUTH_TOKEN_MAX_LEN {
        return Err(format!(
            "auth_token must be {AUTH_TOKEN_MAX_LEN} characters or fewer"
        ));
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("auth_token must be ASCII alphanumeric, '-' or '_'".to_string());
    }
    Ok(token)
}

#[spacetimedb::reducer]
pub fn set_ssh_relay_device(
    ctx: &ReducerContext,
    device_id: u64,
) -> Result<(), String> {
    require_registered_user(ctx)?;
    require_owned_device(ctx, device_id)?;
    if ctx
        .db
        .ssh_relay_device()
        .owner()
        .find(ctx.sender())
        .is_some()
    {
        let existing = ctx.db.ssh_relay_device().owner().find(ctx.sender()).unwrap();
        ctx.db.ssh_relay_device().owner().update(SshRelayDevice {
            device_id,
            updated_at: ctx.timestamp,
            ..existing
        });
    } else {
        ctx.db.ssh_relay_device().insert(SshRelayDevice {
            owner: ctx.sender(),
            device_id,
            updated_at: ctx.timestamp,
            listen_url: None,
        });
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn clear_ssh_relay_device(ctx: &ReducerContext) -> Result<(), String> {
    require_registered_user(ctx)?;
    if ctx
        .db
        .ssh_relay_device()
        .owner()
        .find(ctx.sender())
        .is_some()
    {
        ctx.db.ssh_relay_device().owner().delete(ctx.sender());
    }
    Ok(())
}

const LISTEN_URL_MAX_LEN: usize = 512;

fn validate_listen_url(url: String) -> Result<String, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("listen url cannot be empty".to_string());
    }
    if url.len() > LISTEN_URL_MAX_LEN {
        return Err(format!(
            "listen url must be {LISTEN_URL_MAX_LEN} characters or fewer"
        ));
    }
    // We accept ws://, wss://, http://, https://, or a bare
    // host:port. We don't try to resolve anything; the URL is
    // passed straight to the browser.
    if !(url.starts_with("ws://")
        || url.starts_with("wss://")
        || url.starts_with("http://")
        || url.starts_with("https://"))
    {
        return Err(
            "listen url must start with ws://, wss://, http://, or https://".to_string(),
        );
    }
    Ok(url)
}

/// Set or clear the address the browser can use to reach the
/// relay's HTTP/WS service. Pass an empty string to clear.
#[spacetimedb::reducer]
pub fn set_ssh_relay_device_url(
    ctx: &ReducerContext,
    url: String,
) -> Result<(), String> {
    require_registered_user(ctx)?;
    let existing = ctx
        .db
        .ssh_relay_device()
        .owner()
        .find(ctx.sender())
        .ok_or_else(|| "no ssh relay device is set".to_string())?;
    let new_url = if url.trim().is_empty() {
        None
    } else {
        Some(validate_listen_url(url)?)
    };
    ctx.db.ssh_relay_device().owner().update(SshRelayDevice {
        listen_url: new_url,
        updated_at: ctx.timestamp,
        ..existing
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn open_ssh_relay_session(
    ctx: &ReducerContext,
    relay_device_id: u64,
    endpoint_id: u64,
    requester_device_id: Option<u64>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    require_owned_device(ctx, relay_device_id)?;
    let ep = require_owned_endpoint_visible(ctx, endpoint_id)?;
    if !ep.enabled {
        return Err("ssh endpoint is disabled".to_string());
    }
    let relay = ctx
        .db
        .ssh_relay_device()
        .owner()
        .find(user.identity)
        .ok_or_else(|| "no ssh relay device set for this account".to_string())?;
    if relay.device_id != relay_device_id {
        return Err(format!(
            "device #{relay_device_id} is not the configured relay; \
             current relay is device #{}",
            relay.device_id
        ));
    }
    if let Some(rid) = requester_device_id {
        require_owned_device(ctx, rid)?;
    }
    ctx.db.ssh_relay_session().insert(SshRelaySession {
        id: 0,
        owner: user.identity,
        requester_device_id,
        relay_device_id,
        endpoint_id,
        status: SshRelaySessionStatus::Pending,
        created_at: ctx.timestamp,
        expires_at: ctx.timestamp + TimeDuration::from_micros(SESSION_MAX_LIFETIME_MICROS),
        auth_token: None,
    });
    Ok(())
}

/// Called by the relay device once it has minted a session-specific
/// token (e.g. via `rand`). The browser learns the token by reading
/// `my_ssh_relay_sessions.auth_token` and uses it as a bearer token
/// when opening the WebSocket. The relay device verifies the same
/// token on the WS upgrade.
#[spacetimedb::reducer]
pub fn attach_ssh_relay_session_token(
    ctx: &ReducerContext,
    session_id: u64,
    token: String,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let token = validate_auth_token(token)?;
    let session = ctx
        .db
        .ssh_relay_session()
        .id()
        .find(session_id)
        .ok_or_else(|| "ssh relay session not found".to_string())?;
    if session.owner != user.identity {
        return Err("not your ssh relay session".to_string());
    }
    if session.status == SshRelaySessionStatus::Closed {
        return Err("ssh relay session is closed".to_string());
    }
    // Authorise: the caller must own `relay_device_id` (which they
    // always do, since the session is theirs) AND the device row
    // must still exist.
    let _ = ctx
        .db
        .device()
        .id()
        .find(session.relay_device_id)
        .ok_or_else(|| "relay device no longer exists".to_string())?;
    ctx.db.ssh_relay_session().id().update(SshRelaySession {
        status: SshRelaySessionStatus::Active,
        auth_token: Some(token),
        ..session
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn close_ssh_relay_session(ctx: &ReducerContext, session_id: u64) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let session = ctx
        .db
        .ssh_relay_session()
        .id()
        .find(session_id)
        .ok_or_else(|| "ssh relay session not found".to_string())?;
    if session.owner != user.identity {
        return Err("not your ssh relay session".to_string());
    }
    ctx.db.ssh_relay_session().id().delete(session_id);
    Ok(())
}

#[view(accessor = my_ssh_relay_device, public)]
fn my_ssh_relay_device(ctx: &ViewContext) -> Option<SshRelayDeviceMetadata> {
    ctx.db
        .ssh_relay_device()
        .owner()
        .find(ctx.sender())
        .map(SshRelayDeviceMetadata::from)
}

#[view(accessor = my_ssh_relay_sessions, public)]
fn my_ssh_relay_sessions(ctx: &ViewContext) -> Vec<SshRelaySessionMetadata> {
    let Some(user) = ctx.db.session().connection().find(ctx.sender()).map(|s| s.user) else {
        return Vec::new();
    };
    ctx.db
        .ssh_relay_session()
        .owner()
        .filter(user)
        .map(SshRelaySessionMetadata::from)
        .collect()
}
