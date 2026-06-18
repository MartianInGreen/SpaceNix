use spacetimedb::{ReducerContext, Table};

pub mod api_key;
mod config;
mod device;
mod file;
mod secret;
mod user;

use crate::config::s3_config as _;
use crate::user::session as _;

#[spacetimedb::table(accessor = person, public)]
pub struct Person {
    name: String,
}

#[spacetimedb::reducer(init)]
pub fn init(ctx: &ReducerContext) {
    if ctx.db.s3_config().id().find(config::S3_CONFIG_ID).is_none() {
        ctx.db.s3_config().insert(config::S3Config {
            id: config::S3_CONFIG_ID,
            bucket: String::new(),
            region: String::new(),
            endpoint: None,
            access_key_id: String::new(),
            secret_access_key: String::new(),
            path_prefix: None,
            public_base_url: None,
        });
    }
}

#[spacetimedb::reducer(client_connected)]
pub fn identity_connected(_ctx: &ReducerContext) {
    // Each new client gets a fresh anonymous SpacetimeDB identity and
    // must call sign_in / sign_up before performing any data operations.
}

#[spacetimedb::reducer(client_disconnected)]
pub fn identity_disconnected(ctx: &ReducerContext) {
    // Drop the session for this connection so it cannot be reused.
    let _ = ctx.db.session().connection().delete(ctx.sender());
}
