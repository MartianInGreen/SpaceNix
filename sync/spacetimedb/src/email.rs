use spacetimedb::rand::Rng;
use spacetimedb::{procedure, Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view};

use crate::user::{
    normalize_email, password as _, require_admin, require_registered_user, session as _,
    user as _, user__view as _,
};

pub const EMAIL_CONFIG_ID: u32 = 1;

const RESEND_COOLDOWN_MICROS: i64 = 120_000_000; // 60 seconds
const CODE_TTL_MICROS: i64 = 900_000_000; // 15 minutes
const MAX_ATTEMPTS: u32 = 5;

#[spacetimedb::table(accessor = scaleway_email_config)]
pub struct ScalewayEmailConfig {
    #[primary_key]
    pub id: u32,
    pub region: String,
    pub secret_key: String,
    pub project_id: String,
    pub from_email: String,
    pub from_name: String,
    pub enabled: bool,
}

#[spacetimedb::table(accessor = email_verification)]
pub struct EmailVerification {
    #[primary_key]
    pub email: String,
    #[index(btree)]
    pub identity: Identity,
    pub code: String,
    /// "signup" or "change".
    pub purpose: String,
    pub attempts: u32,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub verified_at: Option<Timestamp>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct ScalewayEmailConfigStatus {
    pub id: u32,
    pub configured: bool,
    pub enabled: bool,
    pub region: String,
    pub has_secret_key: bool,
    pub has_project_id: bool,
    pub from_email: String,
    pub from_name: String,
}

#[spacetimedb::reducer]
pub fn update_scaleway_email_config(
    ctx: &ReducerContext,
    region: String,
    secret_key: Option<String>,
    project_id: Option<String>,
    from_email: String,
    from_name: Option<String>,
    enabled: Option<bool>,
) -> Result<(), String> {
    require_admin(ctx)?;

    let region = region.trim().to_string();
    if region.is_empty() {
        return Err("region cannot be empty".to_string());
    }
    let from_email = normalize_email(&from_email)?;
    let from_name = from_name.unwrap_or_else(|| "SpaceNix".to_string());
    let from_name = if from_name.trim().is_empty() {
        "SpaceNix".to_string()
    } else {
        from_name.trim().to_string()
    };

    let previous = ctx.db.scaleway_email_config().id().find(EMAIL_CONFIG_ID);
    let secret_key = match secret_key {
        Some(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err("secret_key cannot be empty".to_string());
            }
            value
        }
        None => previous
            .as_ref()
            .map(|cfg| cfg.secret_key.clone())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "secret_key cannot be empty".to_string())?,
    };
    let project_id = match project_id {
        Some(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err("project_id cannot be empty".to_string());
            }
            value
        }
        None => previous
            .as_ref()
            .map(|cfg| cfg.project_id.clone())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "project_id cannot be empty".to_string())?,
    };

    let updated = ScalewayEmailConfig {
        id: EMAIL_CONFIG_ID,
        region,
        secret_key,
        project_id,
        from_email,
        from_name,
        enabled: enabled.unwrap_or(true),
    };

    if previous.is_some() {
        ctx.db.scaleway_email_config().id().update(updated);
    } else {
        ctx.db.scaleway_email_config().insert(updated);
    }
    Ok(())
}

#[procedure]
pub fn update_scaleway_email_config_with_credentials(
    ctx: &mut spacetimedb::ProcedureContext,
    email: String,
    password: String,
    region: String,
    secret_key: Option<String>,
    project_id: Option<String>,
    from_email: String,
    from_name: Option<String>,
    enabled: Option<bool>,
) -> Result<(), String> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use argon2::Argon2;

    let region = region.trim().to_string();
    if region.is_empty() {
        return Err("region cannot be empty".to_string());
    }
    let from_email = normalize_email(&from_email)?;
    let from_name = from_name.unwrap_or_else(|| "SpaceNix".to_string());
    let from_name = if from_name.trim().is_empty() {
        "SpaceNix".to_string()
    } else {
        from_name.trim().to_string()
    };

    let email = email.trim().to_lowercase();
    let now = ctx.timestamp;

    let secret_key = secret_key.and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() { None } else { Some(v) }
    });
    let project_id = project_id.and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() { None } else { Some(v) }
    });

    let secret_key = secret_key.ok_or_else(|| "secret_key cannot be empty".to_string())?;
    let project_id = project_id.ok_or_else(|| "project_id cannot be empty".to_string())?;

    ctx.try_with_tx(|tx| -> Result<(), String> {
        let Some(user) = tx.db.user().email().find(&email) else {
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

        let updated = ScalewayEmailConfig {
            id: EMAIL_CONFIG_ID,
            region: region.clone(),
            secret_key: secret_key.clone(),
            project_id: project_id.clone(),
            from_email: from_email.clone(),
            from_name: from_name.clone(),
            enabled: enabled.unwrap_or(true),
        };
        if tx.db.scaleway_email_config().id().find(EMAIL_CONFIG_ID).is_some() {
            tx.db.scaleway_email_config().id().update(updated);
        } else {
            tx.db.scaleway_email_config().insert(updated);
        }
        Ok(())
    })
}

#[view(accessor = scaleway_email_config_status, public)]
fn scaleway_email_config_status(ctx: &ViewContext) -> Option<ScalewayEmailConfigStatus> {
    let cfg = ctx.db.scaleway_email_config().id().find(EMAIL_CONFIG_ID)?;
    let has_secret_key = !cfg.secret_key.is_empty();
    let has_project_id = !cfg.project_id.is_empty();
    let is_admin = ctx
        .db
        .user()
        .identity()
        .find(ctx.sender())
        .is_some_and(|user| user.role == crate::user::ROLE_ADMIN);

    Some(ScalewayEmailConfigStatus {
        id: cfg.id,
        configured: has_secret_key && has_project_id && !cfg.region.is_empty() && !cfg.from_email.is_empty(),
        enabled: cfg.enabled,
        region: if is_admin { cfg.region.clone() } else { String::new() },
        has_secret_key,
        has_project_id,
        from_email: if is_admin { cfg.from_email.clone() } else { String::new() },
        from_name: if is_admin { cfg.from_name.clone() } else { String::new() },
    })
}

#[spacetimedb::reducer]
pub fn request_email_verification(ctx: &ReducerContext) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    if user.email_verified {
        return Err("email is already verified".to_string());
    }
    create_or_refresh_verification(ctx, user.email, user.identity, "signup")
}

#[spacetimedb::reducer]
pub fn request_email_change_verification(
    ctx: &ReducerContext,
    new_email: String,
    current_password: String,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    if !user.email_verified {
        return Err("verify your current email before changing it".to_string());
    }
    crate::user::verify_current_password(ctx, &user.identity, &current_password)?;
    let new_email = normalize_email(&new_email)?;
    if new_email == user.email {
        return Err("new email is the same as the current email".to_string());
    }
    if ctx.db.user().email().find(&new_email).is_some() {
        return Err("an account with that email already exists".to_string());
    }
    // Only one pending email change is allowed per user. Remove any stale one.
    let stale: Vec<String> = ctx
        .db
        .email_verification()
        .identity()
        .filter(user.identity)
        .filter(|v| v.purpose == "change")
        .map(|v| v.email)
        .collect();
    for email in stale {
        ctx.db.email_verification().email().delete(&email);
    }
    create_or_refresh_verification(ctx, new_email, user.identity, "change")
}

#[spacetimedb::reducer]
pub fn verify_email(ctx: &ReducerContext, code: String) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let Some(verification) = ctx.db.email_verification().email().find(&user.email) else {
        return Err("no pending verification for this email".to_string());
    };
    if verification.verified_at.is_some() {
        return Err("email is already verified".to_string());
    }
    if ctx.timestamp > verification.expires_at {
        ctx.db.email_verification().email().delete(&user.email);
        return Err("verification code has expired; request a new one".to_string());
    }
    if verification.attempts >= MAX_ATTEMPTS {
        ctx.db.email_verification().email().delete(&user.email);
        return Err("too many failed attempts; request a new code".to_string());
    }
    if verification.code != code.trim() {
        let mut verification = verification;
        verification.attempts += 1;
        ctx.db.email_verification().email().update(verification);
        return Err("invalid verification code".to_string());
    }

    ctx.db.email_verification().email().delete(&user.email);
    let mut user = ctx
        .db
        .user()
        .identity()
        .find(user.identity)
        .ok_or_else(|| "user not found".to_string())?;
    user.email_verified = true;
    ctx.db.user().identity().update(user);
    Ok(())
}

#[spacetimedb::reducer]
pub fn confirm_email_change(ctx: &ReducerContext, code: String) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let Some(pending) = ctx
        .db
        .email_verification()
        .identity()
        .filter(user.identity)
        .find(|v| v.purpose == "change")
    else {
        return Err("no pending email change".to_string());
    };
    if pending.verified_at.is_some() {
        return Err("email change is already confirmed".to_string());
    }
    if ctx.timestamp > pending.expires_at {
        ctx.db.email_verification().email().delete(&pending.email);
        return Err("verification code has expired; request a new one".to_string());
    }
    if pending.attempts >= MAX_ATTEMPTS {
        ctx.db.email_verification().email().delete(&pending.email);
        return Err("too many failed attempts; request a new code".to_string());
    }
    if pending.code != code.trim() {
        let mut pending = pending;
        pending.attempts += 1;
        ctx.db.email_verification().email().update(pending);
        return Err("invalid verification code".to_string());
    }

    ctx.db.email_verification().email().delete(&pending.email);
    let mut user = ctx
        .db
        .user()
        .identity()
        .find(user.identity)
        .ok_or_else(|| "user not found".to_string())?;
    user.email = pending.email;
    user.email_verified = true;
    ctx.db.user().identity().update(user);
    Ok(())
}

#[procedure]
pub fn send_verification_email(ctx: &mut spacetimedb::ProcedureContext) -> Result<(), String> {
    let sender = ctx.sender();
    let now = ctx.timestamp;
    let (to_email, code, config) = ctx.try_with_tx(
        |tx| -> Result<(String, String, ScalewayEmailConfig), String> {
            let user = tx
                .db
                .session()
                .connection()
                .find(sender)
                .map(|s| s.user)
                .ok_or_else(|| "sign in first".to_string())?;
            let user = tx
                .db
                .user()
                .identity()
                .find(user)
                .ok_or_else(|| "user not found".to_string())?;
            let verification = tx
                .db
                .email_verification()
                .email()
                .find(&user.email)
                .ok_or_else(|| "no pending verification for this email".to_string())?;
            if verification.verified_at.is_some() {
                return Err("email is already verified".to_string());
            }
            if now > verification.expires_at {
                return Err("verification code has expired; request a new one".to_string());
            }
            let config = tx
                .db
                .scaleway_email_config()
                .id()
                .find(EMAIL_CONFIG_ID)
                .ok_or_else(|| "email service is not configured".to_string())?;
            if !config.enabled {
                return Err("email service is disabled".to_string());
            }
            if config.secret_key.is_empty()
                || config.project_id.is_empty()
                || config.region.is_empty()
                || config.from_email.is_empty()
            {
                return Err("email service is not fully configured".to_string());
            }
            Ok((verification.email.clone(), verification.code.clone(), config))
        },
    )?;

    send_scaleway_email(ctx, &config, &to_email, &code)
}

fn create_or_refresh_verification(
    ctx: &ReducerContext,
    email: String,
    identity: Identity,
    purpose: &str,
) -> Result<(), String> {
    let now = ctx.timestamp;
    let cooldown = spacetimedb::TimeDuration::from_micros(RESEND_COOLDOWN_MICROS);
    let ttl = spacetimedb::TimeDuration::from_micros(CODE_TTL_MICROS);

    if let Some(existing) = ctx.db.email_verification().email().find(&email) {
        if existing.identity != identity {
            return Err("verification belongs to another user".to_string());
        }
        if existing.verified_at.is_some() {
            return Err("email is already verified".to_string());
        }
        if now < existing.created_at + cooldown {
            return Err("please wait before requesting a new code".to_string());
        }
        let code = generate_code(ctx);
        ctx.db.email_verification().email().update(EmailVerification {
            email: email.clone(),
            identity,
            code,
            purpose: purpose.to_string(),
            attempts: 0,
            created_at: now,
            expires_at: now + ttl,
            verified_at: None,
        });
    } else {
        let code = generate_code(ctx);
        ctx.db.email_verification().insert(EmailVerification {
            email,
            identity,
            code,
            purpose: purpose.to_string(),
            attempts: 0,
            created_at: now,
            expires_at: now + ttl,
            verified_at: None,
        });
    }
    Ok(())
}

fn generate_code(ctx: &ReducerContext) -> String {
    format!("{:06}", ctx.rng().gen_range(0..=999999))
}

fn send_scaleway_email(
    ctx: &mut spacetimedb::ProcedureContext,
    config: &ScalewayEmailConfig,
    to_email: &str,
    code: &str,
) -> Result<(), String> {
    let url = format!(
        "https://api.scaleway.com/transactional-email/v1alpha1/regions/{}/emails",
        config.region
    );

    let subject = "Verify your SpaceNix email";
    let text = format!("Your SpaceNix verification code is: {code}");
    let html = format!(
        "<html><body><p>Your SpaceNix verification code is: <strong>{code}</strong></p><br/>Do not share this code with anyone.</body></html>"
    );

    let body = format!(
        r#"{{"from":{{"name":{},"email":{}}},"to":[{{"email":{}}}],"subject":{},"text":{},"html":{},"project_id":{}}}"#,
        json_string(&config.from_name),
        json_string(&config.from_email),
        json_string(to_email),
        json_string(subject),
        json_string(&text),
        json_string(&html),
        json_string(&config.project_id)
    );

    let request = spacetimedb::http::Request::builder()
        .method("POST")
        .uri(&url)
        .header("X-Auth-Token", config.secret_key.clone())
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| format!("failed to build email request: {e}"))?;

    let response = ctx
        .http
        .send(request)
        .map_err(|e| format!("email request failed: {e}"))?;
    let (parts, body) = response.into_parts();
    if parts.status.is_success() {
        Ok(())
    } else {
        let message = body.into_string_lossy();
        Err(format!(
            "scaleway returned {}: {message}",
            parts.status.as_u16()
        ))
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
