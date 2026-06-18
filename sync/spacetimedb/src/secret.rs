use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, procedure, view,
};

#[spacetimedb::table(accessor = user_secret, public)]
pub struct UserSecret {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    #[index(btree)]
    pub env: String,
    pub value: String,
    pub device_ids: Vec<String>,
    pub permissions: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SecretMetadata {
    pub id: u64,
    pub env: String,
    pub device_ids: Vec<String>,
    pub permissions: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct SecretValue {
    pub id: u64,
    pub env: String,
    pub value: String,
    pub device_ids: Vec<String>,
    pub permissions: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl From<UserSecret> for SecretMetadata {
    fn from(s: UserSecret) -> Self {
        Self {
            id: s.id,
            env: s.env,
            device_ids: s.device_ids,
            permissions: s.permissions,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

fn is_valid_env(env: &str) -> bool {
    !env.is_empty()
        && env
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn validate_device_ids(device_ids: &[String]) -> Result<(), String> {
    if device_ids.len() > 64 {
        return Err("too many device_ids (max 64)".to_string());
    }
    for d in device_ids {
        if d.is_empty() || d.len() > 128 {
            return Err("device_id must be 1..=128 chars".to_string());
        }
    }
    Ok(())
}

fn normalize_permissions(mut permissions: Vec<String>) -> Result<Vec<String>, String> {
    if permissions.len() > 64 {
        return Err("too many permissions (max 64)".to_string());
    }
    for p in &mut permissions {
        *p = p.trim().to_string();
        if p.is_empty() {
            return Err("permission cannot be empty".to_string());
        }
        if p.len() > 128 {
            return Err("permission must be 128 characters or fewer".to_string());
        }
        if !p
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-' | '*'))
        {
            return Err("permission contains invalid characters".to_string());
        }
    }
    permissions.sort();
    permissions.dedup();
    Ok(permissions)
}

#[spacetimedb::reducer]
pub fn set_secret(
    ctx: &ReducerContext,
    env: String,
    value: String,
    device_ids: Vec<String>,
    permissions: Vec<String>,
) -> Result<(), String> {
    if !is_valid_env(&env) {
        return Err("env must be non-empty and contain only [A-Za-z0-9_.]".to_string());
    }
    if value.is_empty() {
        return Err("value cannot be empty".to_string());
    }
    validate_device_ids(&device_ids)?;
    let permissions = normalize_permissions(permissions)?;

    let owner = ctx.sender();
    let existing = ctx
        .db
        .user_secret()
        .owner()
        .filter(owner)
        .find(|s| s.env == env);

    if let Some(prev) = existing {
        let updated = UserSecret {
            value,
            device_ids,
            permissions,
            updated_at: ctx.timestamp,
            ..prev
        };
        ctx.db.user_secret().id().update(updated);
    } else {
        ctx.db.user_secret().insert(UserSecret {
            id: 0,
            owner,
            env,
            value,
            device_ids,
            permissions,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
        });
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_secret_value(ctx: &ReducerContext, id: u64, value: String) -> Result<(), String> {
    if value.is_empty() {
        return Err("value cannot be empty".to_string());
    }
    let mut secret = ctx
        .db
        .user_secret()
        .id()
        .find(id)
        .ok_or_else(|| "secret not found".to_string())?;
    if secret.owner != ctx.sender() {
        return Err("not your secret".to_string());
    }
    secret.value = value;
    secret.updated_at = ctx.timestamp;
    ctx.db.user_secret().id().update(secret);
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_secret_devices(
    ctx: &ReducerContext,
    id: u64,
    device_ids: Vec<String>,
) -> Result<(), String> {
    validate_device_ids(&device_ids)?;
    let mut secret = ctx
        .db
        .user_secret()
        .id()
        .find(id)
        .ok_or_else(|| "secret not found".to_string())?;
    if secret.owner != ctx.sender() {
        return Err("not your secret".to_string());
    }
    secret.device_ids = device_ids;
    secret.updated_at = ctx.timestamp;
    ctx.db.user_secret().id().update(secret);
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_secret_permissions(
    ctx: &ReducerContext,
    id: u64,
    permissions: Vec<String>,
) -> Result<(), String> {
    let permissions = normalize_permissions(permissions)?;
    let mut secret = ctx
        .db
        .user_secret()
        .id()
        .find(id)
        .ok_or_else(|| "secret not found".to_string())?;
    if secret.owner != ctx.sender() {
        return Err("not your secret".to_string());
    }
    secret.permissions = permissions;
    secret.updated_at = ctx.timestamp;
    ctx.db.user_secret().id().update(secret);
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_secret(ctx: &ReducerContext, id: u64) -> Result<(), String> {
    let row = ctx
        .db
        .user_secret()
        .id()
        .find(id)
        .ok_or_else(|| "secret not found".to_string())?;
    if row.owner != ctx.sender() {
        return Err("not your secret".to_string());
    }
    ctx.db.user_secret().id().delete(id);
    Ok(())
}

#[view(accessor = my_secrets, public)]
fn my_secrets(ctx: &ViewContext) -> Vec<SecretMetadata> {
    ctx.db
        .user_secret()
        .owner()
        .filter(ctx.sender())
        .map(SecretMetadata::from)
        .collect()
}

#[procedure]
pub fn get_secret(
    ctx: &mut spacetimedb::ProcedureContext,
    id: u64,
) -> Result<Option<SecretMetadata>, String> {
    let sender = ctx.sender();
    ctx.try_with_tx(|tx| -> Result<Option<SecretMetadata>, String> {
        Ok(tx
            .db
            .user_secret()
            .id()
            .find(id)
            .filter(|s| s.owner == sender)
            .map(SecretMetadata::from))
    })
}

#[procedure]
pub fn reveal_secret(
    ctx: &mut spacetimedb::ProcedureContext,
    id: u64,
) -> Result<Option<SecretValue>, String> {
    let sender = ctx.sender();
    ctx.try_with_tx(|tx| -> Result<Option<SecretValue>, String> {
        Ok(tx
            .db
            .user_secret()
            .id()
            .find(id)
            .filter(|s| s.owner == sender)
            .map(|s| SecretValue {
                id: s.id,
                env: s.env,
                value: s.value,
                device_ids: s.device_ids,
                permissions: s.permissions,
                created_at: s.created_at,
                updated_at: s.updated_at,
            }))
    })
}
