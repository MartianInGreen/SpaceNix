use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, procedure,
    rand::RngCore, view,
};

use crate::user::user as _;

const API_KEY_PREFIX: &str = "snx_";
const TOKEN_BYTES: usize = 32;

#[spacetimedb::table(accessor = api_key)]
pub struct ApiKey {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    pub name: String,
    #[unique]
    pub token_hash: String,
    pub permissions: Vec<String>,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct ApiKeyMetadata {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub permissions: Vec<String>,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct CreatedApiKey {
    pub token: String,
    pub metadata: ApiKeyMetadata,
}

impl From<ApiKey> for ApiKeyMetadata {
    fn from(key: ApiKey) -> Self {
        Self {
            id: key.id,
            owner: key.owner,
            name: key.name,
            permissions: key.permissions,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
            revoked_at: key.revoked_at,
        }
    }
}

#[view(accessor = my_api_keys, public)]
fn my_api_keys(ctx: &ViewContext) -> Vec<ApiKeyMetadata> {
    ctx.db
        .api_key()
        .owner()
        .filter(ctx.sender())
        .map(ApiKeyMetadata::from)
        .collect()
}

#[procedure]
pub fn create_api_key(
    ctx: &mut spacetimedb::ProcedureContext,
    name: String,
    permissions: Vec<String>,
) -> Result<CreatedApiKey, String> {
    let owner = ctx.sender();
    let name = validate_name(name)?;
    let permissions = normalize_permissions(permissions)?;

    let mut token_bytes = [0u8; TOKEN_BYTES];
    ctx.rng().fill_bytes(&mut token_bytes);
    let token = format!("{API_KEY_PREFIX}{}", encode_hex(&token_bytes));
    let token_hash = hash_token(&token);
    let now = ctx.timestamp;

    let inserted = ctx.try_with_tx(|tx| -> Result<ApiKey, String> {
        if tx.db.user().identity().find(owner).is_none() {
            return Err("identity is not registered".to_string());
        }
        Ok(tx.db.api_key().insert(ApiKey {
            id: 0,
            owner,
            name: name.clone(),
            token_hash: token_hash.clone(),
            permissions: permissions.clone(),
            created_at: now,
            last_used_at: None,
            revoked_at: None,
        }))
    })?;

    Ok(CreatedApiKey {
        token,
        metadata: inserted.into(),
    })
}

#[spacetimedb::reducer]
pub fn revoke_api_key(ctx: &ReducerContext, api_key_id: u64) -> Result<(), String> {
    let key = ctx
        .db
        .api_key()
        .id()
        .find(api_key_id)
        .ok_or_else(|| "api key not found".to_string())?;
    if key.owner != ctx.sender() {
        return Err("not your api key".to_string());
    }
    ctx.db.api_key().id().update(ApiKey {
        revoked_at: Some(ctx.timestamp),
        ..key
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn update_api_key_permissions(
    ctx: &ReducerContext,
    api_key_id: u64,
    permissions: Vec<String>,
) -> Result<(), String> {
    let key = ctx
        .db
        .api_key()
        .id()
        .find(api_key_id)
        .ok_or_else(|| "api key not found".to_string())?;
    if key.owner != ctx.sender() {
        return Err("not your api key".to_string());
    }
    ctx.db.api_key().id().update(ApiKey {
        permissions: normalize_permissions(permissions)?,
        ..key
    });
    Ok(())
}

#[procedure]
pub fn api_key_has_permission(
    ctx: &mut spacetimedb::ProcedureContext,
    token: String,
    permission: String,
) -> Result<bool, String> {
    validate_permission(&permission)?;
    let token_hash = hash_token(token.trim());
    let now = ctx.timestamp;

    ctx.try_with_tx(|tx| -> Result<bool, String> {
        let Some(key) = tx.db.api_key().token_hash().find(&token_hash) else {
            return Ok(false);
        };
        let allowed =
            key.revoked_at.is_none() && permission_is_granted(&key.permissions, &permission);
        if allowed {
            tx.db.api_key().id().update(ApiKey {
                last_used_at: Some(now),
                ..key
            });
        }
        Ok(allowed)
    })
}

pub fn api_key_owner_with_permission(
    ctx: &ReducerContext,
    token: &str,
    permission: &str,
) -> Result<Identity, String> {
    validate_permission(permission)?;
    let token_hash = hash_token(token.trim());
    let key = ctx
        .db
        .api_key()
        .token_hash()
        .find(&token_hash)
        .ok_or_else(|| "invalid api key".to_string())?;
    if key.revoked_at.is_some() {
        return Err("api key is revoked".to_string());
    }
    if !permission_is_granted(&key.permissions, permission) {
        return Err("api key lacks permission".to_string());
    }
    let owner = key.owner;
    ctx.db.api_key().id().update(ApiKey {
        last_used_at: Some(ctx.timestamp),
        ..key
    });
    Ok(owner)
}

fn validate_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > 128 {
        return Err("name must be 128 characters or fewer".to_string());
    }
    Ok(name)
}

fn normalize_permissions(mut permissions: Vec<String>) -> Result<Vec<String>, String> {
    if permissions.is_empty() {
        return Err("at least one permission is required".to_string());
    }
    if permissions.len() > 64 {
        return Err("too many permissions (max 64)".to_string());
    }

    for permission in &mut permissions {
        *permission = permission.trim().to_string();
        validate_permission(permission)?;
    }
    permissions.sort();
    permissions.dedup();
    Ok(permissions)
}

fn validate_permission(permission: &str) -> Result<(), String> {
    if permission.is_empty() {
        return Err("permission cannot be empty".to_string());
    }
    if permission.len() > 128 {
        return Err("permission must be 128 characters or fewer".to_string());
    }
    if !permission
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-' | '*'))
    {
        return Err("permission contains invalid characters".to_string());
    }
    Ok(())
}

fn permission_is_granted(grants: &[String], permission: &str) -> bool {
    grants.iter().any(|grant| {
        grant == "*"
            || grant == permission
            || grant
                .strip_suffix(":*")
                .is_some_and(|prefix| permission.starts_with(&format!("{prefix}:")))
    })
}

fn hash_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
