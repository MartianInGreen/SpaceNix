use spacetimedb::{procedure, ReducerContext, SpacetimeType, Table, ViewContext, view};

use crate::user::{password as _, require_admin, user as _, user__view as _};

pub const S3_CONFIG_ID: u32 = 1;

#[spacetimedb::table(accessor = s3_config)]
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

#[derive(SpacetimeType, Clone, Debug)]
pub struct S3ConfigStatus {
    pub id: u32,
    pub configured: bool,
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub path_prefix: Option<String>,
    pub public_base_url: Option<String>,
    pub has_access_key_id: bool,
    pub has_secret_access_key: bool,
}

#[spacetimedb::reducer]
pub fn update_s3_config(
    ctx: &ReducerContext,
    bucket: String,
    region: String,
    endpoint: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    path_prefix: Option<String>,
    public_base_url: Option<String>,
) -> Result<(), String> {
    require_admin(ctx)?;

    let previous = ctx.db.s3_config().id().find(S3_CONFIG_ID);
    if bucket.is_empty() {
        return Err("bucket cannot be empty".to_string());
    }
    if region.is_empty() {
        return Err("region cannot be empty".to_string());
    }
    let access_key_id = match access_key_id {
        Some(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err("access_key_id cannot be empty".to_string());
            }
            value
        }
        None => previous
            .as_ref()
            .map(|cfg| cfg.access_key_id.clone())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "access_key_id cannot be empty".to_string())?,
    };
    let secret_access_key = match secret_access_key {
        Some(value) => {
            if value.is_empty() {
                return Err("secret_access_key cannot be empty".to_string());
            }
            value
        }
        None => previous
            .as_ref()
            .map(|cfg| cfg.secret_access_key.clone())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "secret_access_key cannot be empty".to_string())?,
    };

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

    if previous.is_some() {
        ctx.db.s3_config().id().update(updated);
    } else {
        ctx.db.s3_config().insert(updated);
    }
    Ok(())
}

#[procedure]
pub fn update_s3_config_with_credentials(
    ctx: &mut spacetimedb::ProcedureContext,
    email: String,
    password: String,
    bucket: String,
    region: String,
    endpoint: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    path_prefix: Option<String>,
    public_base_url: Option<String>,
) -> Result<(), String> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use argon2::Argon2;

    if bucket.is_empty() {
        return Err("bucket cannot be empty".to_string());
    }
    if region.is_empty() {
        return Err("region cannot be empty".to_string());
    }

    let email = email.trim().to_lowercase();
    let now = ctx.timestamp;

    let access_key_id = access_key_id.and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    });
    let secret_access_key = secret_access_key.filter(|v| !v.is_empty());

    let access_key_id = access_key_id.ok_or_else(|| "access_key_id cannot be empty".to_string())?;
    if secret_access_key.is_none() {
        return Err("secret_access_key cannot be empty".to_string());
    }
    let secret_access_key = secret_access_key.unwrap();

    ctx.try_with_tx(|tx| -> Result<(), String> {
        let Some(user) = tx.db.user().email().find(&email) else {
            let _ = hash_throwaway(tx, &password);
            return Err("invalid email or password".to_string());
        };
        let Some(cred) = tx.db.password().user().find(user.identity) else {
            return Err("invalid email or password".to_string());
        };
        let parsed = PasswordHash::new(&cred.password_hash)
            .map_err(|e| format!("stored password hash is invalid: {e}"))?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Err("invalid email or password".to_string());
        }
        let mut user = tx
            .db
            .user()
            .identity()
            .find(user.identity)
            .ok_or_else(|| "invalid email or password".to_string())?;
        if user.role != crate::user::ROLE_ADMIN {
            return Err("admin access required".to_string());
        }
        user.last_login_at = now;
        tx.db.user().identity().update(user);

        let updated = S3Config {
            id: S3_CONFIG_ID,
            bucket: bucket.clone(),
            region: region.clone(),
            endpoint: endpoint.clone(),
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            path_prefix: path_prefix.clone(),
            public_base_url: public_base_url.clone(),
        };
        if tx.db.s3_config().id().find(S3_CONFIG_ID).is_some() {
            tx.db.s3_config().id().update(updated);
        } else {
            tx.db.s3_config().insert(updated);
        }
        Ok(())
    })
}

fn hash_throwaway(_tx: &spacetimedb::TxContext, _password: &str) -> Result<(), String> {
    Ok(())
}

#[view(accessor = s3_config_status, public)]
fn s3_config_status(ctx: &ViewContext) -> Option<S3ConfigStatus> {
    let cfg = ctx.db.s3_config().id().find(S3_CONFIG_ID)?;
    let has_access_key_id = !cfg.access_key_id.is_empty();
    let has_secret_access_key = !cfg.secret_access_key.is_empty();
    let is_admin = ctx
        .db
        .user()
        .identity()
        .find(ctx.sender())
        .is_some_and(|user| user.role == crate::user::ROLE_ADMIN);

    Some(S3ConfigStatus {
        id: cfg.id,
        configured: !cfg.bucket.is_empty()
            && !cfg.region.is_empty()
            && has_access_key_id
            && has_secret_access_key,
        bucket: if is_admin { cfg.bucket } else { String::new() },
        region: if is_admin { cfg.region } else { String::new() },
        endpoint: if is_admin { cfg.endpoint } else { None },
        path_prefix: if is_admin { cfg.path_prefix } else { None },
        public_base_url: if is_admin { cfg.public_base_url } else { None },
        has_access_key_id,
        has_secret_access_key,
    })
}
