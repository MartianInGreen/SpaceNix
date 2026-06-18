use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view,
};

pub const S3_CONFIG_ID: u32 = 1;

#[spacetimedb::table(accessor = s3_config, public)]
pub struct S3Config {
    #[primary_key]
    pub id: u32,
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub path_prefix: Option<String>,
    pub public_base_url: Option<String>,
}

#[spacetimedb::reducer]
pub fn update_s3_config(
    ctx: &ReducerContext,
    bucket: String,
    region: String,
    endpoint: Option<String>,
    access_key_id: String,
    secret_access_key: String,
    path_prefix: Option<String>,
    public_base_url: Option<String>,
) -> Result<(), String> {
    if bucket.is_empty() {
        return Err("bucket cannot be empty".to_string());
    }
    if region.is_empty() {
        return Err("region cannot be empty".to_string());
    }
    if access_key_id.is_empty() {
        return Err("access_key_id cannot be empty".to_string());
    }
    if secret_access_key.is_empty() {
        return Err("secret_access_key cannot be empty".to_string());
    }

    let updated = S3Config {
        id: S3_CONFIG_ID,
        bucket,
        region,
        endpoint,
        access_key_id,
        secret_access_key,
        path_prefix,
        public_base_url,
    };

    if ctx.db.s3_config().id().find(S3_CONFIG_ID).is_some() {
        ctx.db.s3_config().id().update(updated);
    } else {
        ctx.db.s3_config().insert(updated);
    }
    Ok(())
}

#[spacetimedb::table(accessor = user_config, public)]
pub struct UserConfig {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    #[index(btree)]
    pub name: String,
    #[unique]
    pub owner_name_key: String,
    pub content: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct UserConfigMetadata {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub content: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl From<UserConfig> for UserConfigMetadata {
    fn from(c: UserConfig) -> Self {
        Self {
            id: c.id,
            owner: c.owner,
            name: c.name,
            content: c.content,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

fn validate_config_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > 256 {
        return Err("name must be 256 characters or fewer".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
    {
        return Err("name may only contain [A-Za-z0-9._-/]".to_string());
    }
    Ok(name)
}

fn owner_name_key(owner: Identity, name: &str) -> String {
    format!("{owner}/{name}")
}

#[spacetimedb::reducer]
pub fn set_config(ctx: &ReducerContext, name: String, content: String) -> Result<(), String> {
    let name = validate_config_name(name)?;
    if content.len() > 256 * 1024 {
        return Err("config content too large (max 256 KiB)".to_string());
    }
    let owner = ctx.sender();
    let key = owner_name_key(owner, &name);
    let existing = ctx.db.user_config().owner_name_key().find(&key);

    if let Some(prev) = existing {
        ctx.db.user_config().id().update(UserConfig {
            content,
            updated_at: ctx.timestamp,
            ..prev
        });
    } else {
        ctx.db.user_config().insert(UserConfig {
            id: 0,
            owner,
            name,
            owner_name_key: key,
            content,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
        });
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_config(ctx: &ReducerContext, config_id: u64) -> Result<(), String> {
    let config = ctx
        .db
        .user_config()
        .id()
        .find(config_id)
        .ok_or_else(|| "config not found".to_string())?;
    if config.owner != ctx.sender() {
        return Err("not your config".to_string());
    }
    ctx.db.user_config().id().delete(config_id);
    Ok(())
}

#[view(accessor = my_configs, public)]
fn my_configs(ctx: &ViewContext) -> Vec<UserConfigMetadata> {
    ctx.db
        .user_config()
        .owner()
        .filter(ctx.sender())
        .map(UserConfigMetadata::from)
        .collect()
}
