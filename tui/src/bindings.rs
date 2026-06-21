//! Re-exports of the most common `module_bindings` traits and the
//! `spacetimedb_sdk::DbContext` / `Table` traits so consumers can just
//! `use crate::bindings::*;` and forget about the per-table imports.
//!
//! Only items that are actually used by the TUI / CLI right now are kept
//! here — the generated module exposes many more.

pub use module_bindings::{
    create_api_key, delete_secret, reveal_secret, revoke_api_key, set_secret,
};
pub use module_bindings::{
    MyApiKeysTableAccess, MyFilesTableAccess, MySecretsTableAccess,
};
pub use spacetimedb_sdk::{DbContext, Table};
