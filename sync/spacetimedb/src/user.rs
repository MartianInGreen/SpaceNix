use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view};

#[spacetimedb::table(accessor = user, public)]
pub struct User {
    #[primary_key]
    pub identity: Identity,
    pub display_name: Option<String>,
    pub created_at: Timestamp,
    pub last_login_at: Timestamp,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct UserProfile {
    pub identity: Identity,
    pub display_name: Option<String>,
    pub created_at: Timestamp,
    pub last_login_at: Timestamp,
}

impl From<User> for UserProfile {
    fn from(u: User) -> Self {
        Self {
            identity: u.identity,
            display_name: u.display_name,
            created_at: u.created_at,
            last_login_at: u.last_login_at,
        }
    }
}

#[view(accessor = my_user, public)]
fn my_user(ctx: &ViewContext) -> Option<UserProfile> {
    ctx.db
        .user()
        .identity()
        .find(ctx.sender())
        .map(UserProfile::from)
}

#[spacetimedb::reducer]
pub fn sign_up(ctx: &ReducerContext, display_name: Option<String>) -> Result<(), String> {
    let identity = ctx.sender();
    if ctx.db.user().identity().find(identity).is_some() {
        return Err("identity already registered".to_string());
    }

    let display_name = normalize_display_name(display_name)?;

    ctx.db.user().insert(User {
        identity,
        display_name,
        created_at: ctx.timestamp,
        last_login_at: ctx.timestamp,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn sign_in(ctx: &ReducerContext) -> Result<(), String> {
    let mut user = ctx
        .db
        .user()
        .identity()
        .find(ctx.sender())
        .ok_or_else(|| "identity is not registered".to_string())?;
    user.last_login_at = ctx.timestamp;
    ctx.db.user().identity().update(user);
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
    if name.len() > 128 {
        return Err("display_name must be 128 characters or fewer".to_string());
    }
    Ok(Some(name))
}
