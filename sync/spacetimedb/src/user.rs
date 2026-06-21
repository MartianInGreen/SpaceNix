use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view};

use spacetimedb::rand::RngCore;

const PASSWORD_MIN_LEN: usize = 8;
const PASSWORD_MAX_LEN: usize = 256;
const EMAIL_MAX_LEN: usize = 254;
const DISPLAY_NAME_MAX_LEN: usize = 128;

#[spacetimedb::table(accessor = user)]
pub struct User {
    #[primary_key]
    pub identity: Identity,
    #[unique]
    pub email: String,
    pub display_name: Option<String>,
    pub role: String,
    pub email_verified: bool,
    pub created_at: Timestamp,
    pub last_login_at: Timestamp,
}

#[spacetimedb::table(accessor = session)]
pub struct Session {
    #[primary_key]
    pub connection: Identity,
    pub user: Identity,
    pub created_at: Timestamp,
}

#[spacetimedb::table(accessor = password)]
pub struct PasswordCredential {
    #[primary_key]
    pub user: Identity,
    pub password_hash: String,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct UserProfile {
    pub identity: Identity,
    pub email: String,
    pub display_name: Option<String>,
    pub role: String,
    pub email_verified: bool,
    pub created_at: Timestamp,
    pub last_login_at: Timestamp,
}

impl From<User> for UserProfile {
    fn from(u: User) -> Self {
        Self {
            identity: u.identity,
            email: u.email,
            display_name: u.display_name,
            role: u.role,
            email_verified: u.email_verified,
            created_at: u.created_at,
            last_login_at: u.last_login_at,
        }
    }
}

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_USER: &str = "user";

#[view(accessor = my_user, public)]
fn my_user(ctx: &ViewContext) -> Option<UserProfile> {
    let user_identity = ctx.db.session().connection().find(ctx.sender())?.user;
    ctx.db
        .user()
        .identity()
        .find(user_identity)
        .map(UserProfile::from)
}

#[spacetimedb::reducer]
pub fn sign_up(
    ctx: &ReducerContext,
    email: String,
    password: String,
    display_name: Option<String>,
) -> Result<(), String> {
    let email = normalize_email(&email)?;
    validate_password(&password)?;
    let display_name = normalize_display_name(display_name)?;
    let password_hash = hash_password(ctx, &password)?;

    if ctx.db.user().email().find(&email).is_some() {
        return Err("an account with that email already exists".to_string());
    }

    let identity = ctx.sender();
    let is_first_user = ctx.db.user().count() == 0;
    let role = if is_first_user {
        ROLE_ADMIN
    } else {
        ROLE_USER
    };

    ctx.db.user().insert(User {
        identity,
        email,
        display_name,
        role: role.to_string(),
        email_verified: is_first_user,
        created_at: ctx.timestamp,
        last_login_at: ctx.timestamp,
    });
    ctx.db.password().insert(PasswordCredential {
        user: identity,
        password_hash,
    });
    bind_session(ctx, identity)?;
    Ok(())
}

#[spacetimedb::reducer]
pub fn sign_in(ctx: &ReducerContext, email: String, password: String) -> Result<(), String> {
    let email = normalize_email(&email)?;
    validate_password(&password)?;

    let Some(user) = ctx.db.user().email().find(&email) else {
        // Still hash a throwaway password to make timing roughly constant.
        let _ = hash_password(ctx, &password);
        return Err("invalid email or password".to_string());
    };

    let Some(cred) = ctx.db.password().user().find(user.identity) else {
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

    let mut user = ctx
        .db
        .user()
        .identity()
        .find(user.identity)
        .ok_or_else(|| "invalid email or password".to_string())?;
    user.last_login_at = ctx.timestamp;
    let user_identity = user.identity;
    ctx.db.user().identity().update(user);

    bind_session(ctx, user_identity)?;
    Ok(())
}

#[spacetimedb::reducer]
pub fn sign_out(ctx: &ReducerContext) -> Result<(), String> {
    if ctx.db.session().connection().delete(ctx.sender()) {
        Ok(())
    } else {
        Err("not signed in".to_string())
    }
}

#[spacetimedb::reducer]
pub fn update_email(
    ctx: &ReducerContext,
    new_email: String,
    current_password: String,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    if !user.email_verified {
        return Err("verify your current email before changing it".to_string());
    }
    verify_current_password(ctx, &user.identity, &current_password)?;

    let email = normalize_email(&new_email)?;
    if email == user.email {
        return Err("new email is the same as the current email".to_string());
    }
    if ctx.db.user().email().find(&email).is_some() {
        return Err("an account with that email already exists".to_string());
    }

    ctx.db.user().identity().update(User {
        email: email.clone(),
        email_verified: false,
        ..user
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn update_password(
    ctx: &ReducerContext,
    current_password: String,
    new_password: String,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    verify_current_password(ctx, &user.identity, &current_password)?;
    validate_password(&new_password)?;

    let password_hash = hash_password(ctx, &new_password)?;
    let cred = ctx
        .db
        .password()
        .user()
        .find(user.identity)
        .ok_or_else(|| "password record missing".to_string())?;
    ctx.db.password().user().update(PasswordCredential {
        password_hash,
        ..cred
    });
    Ok(())
}

pub fn verify_current_password(
    ctx: &ReducerContext,
    user_identity: &Identity,
    password: &str,
) -> Result<(), String> {
    let cred = ctx
        .db
        .password()
        .user()
        .find(*user_identity)
        .ok_or_else(|| "password record missing".to_string())?;
    let parsed = PasswordHash::new(&cred.password_hash)
        .map_err(|e| format!("stored password hash is invalid: {e}"))?;
    if Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_err()
    {
        return Err("current password is incorrect".to_string());
    }
    Ok(())
}

pub fn require_registered_user(ctx: &ReducerContext) -> Result<User, String> {
    let user_identity = ctx
        .db
        .session()
        .connection()
        .find(ctx.sender())
        .ok_or_else(|| "sign in first".to_string())?
        .user;
    ctx.db
        .user()
        .identity()
        .find(user_identity)
        .ok_or_else(|| "sign in first".to_string())
}

pub fn require_admin(ctx: &ReducerContext) -> Result<User, String> {
    let user = require_registered_user(ctx)?;
    if user.role != ROLE_ADMIN {
        return Err("admin access required".to_string());
    }
    Ok(user)
}

fn bind_session(ctx: &ReducerContext, user: Identity) -> Result<(), String> {
    if let Some(prev) = ctx.db.session().connection().find(ctx.sender()) {
        if prev.user != user {
            return Err("this connection is already signed in as another user".to_string());
        }
        return Ok(());
    }
    ctx.db.session().insert(Session {
        connection: ctx.sender(),
        user,
        created_at: ctx.timestamp,
    });
    Ok(())
}

fn hash_password(ctx: &ReducerContext, password: &str) -> Result<String, String> {
    // Reducers must be deterministic. Use the reducer's deterministic RNG for
    // salt generation rather than a real OS RNG.
    let mut salt_bytes = [0u8; 16];
    ctx.rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::from_b64(&encode_b64_nopad(&salt_bytes))
        .map_err(|e| format!("could not build salt: {e}"))?;
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("could not hash password: {e}"))
}

/// Base64 encode without padding. `SaltString::from_b64` rejects the `=`
/// padding character.
fn encode_b64_nopad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b0 = bytes[i];
        let b1 = bytes[i + 1];
        let b2 = bytes[i + 2];
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let b0 = bytes[i];
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[((b0 & 0x03) << 4) as usize] as char);
    } else if rem == 2 {
        let b0 = bytes[i];
        let b1 = bytes[i + 1];
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(ALPHABET[((b1 & 0x0f) << 2) as usize] as char);
    }
    out
}

pub fn normalize_email(email: &str) -> Result<String, String> {
    let email = email.trim().to_lowercase();
    if email.is_empty() {
        return Err("email cannot be empty".to_string());
    }
    if email.len() > EMAIL_MAX_LEN {
        return Err("email is too long".to_string());
    }
    let mut parts = email.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err("email is not valid".to_string());
    }
    if !local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        return Err("email is not valid".to_string());
    }
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("email is not valid".to_string());
    }
    Ok(email)
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.chars().count() < PASSWORD_MIN_LEN {
        return Err(format!(
            "password must be at least {PASSWORD_MIN_LEN} characters"
        ));
    }
    if password.chars().count() > PASSWORD_MAX_LEN {
        return Err(format!(
            "password must be {PASSWORD_MAX_LEN} characters or fewer"
        ));
    }
    Ok(())
}

fn normalize_display_name(display_name: Option<String>) -> Result<Option<String>, String> {
    let Some(name) = display_name else {
        return Ok(None);
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() > DISPLAY_NAME_MAX_LEN {
        return Err(format!(
            "display_name must be {DISPLAY_NAME_MAX_LEN} characters or fewer"
        ));
    }
    Ok(Some(name))
}
