//! Re-exports of the most common `module_bindings` traits and the
//! `spacetimedb_sdk::DbContext` / `Table` traits so consumers can just
//! `use crate::bindings::*;` and forget about the per-table imports.
//!
//! Only items that are actually used by the TUI / CLI right now are kept
//! here — the generated module exposes many more.

#![allow(unused_imports)]

pub use module_bindings::{
    MyApiKeysTableAccess, MyDevicesTableAccess, MyFilesTableAccess, MySecretsTableAccess,
    MySshEndpointsTableAccess, MySshKeysTableAccess, MyUserTableAccess,
};
pub use module_bindings::{
    create_api_key, create_folder, delete_device, delete_file, delete_secret, delete_ssh_endpoint,
    delete_ssh_key, register_device, rename_device, rename_file, reveal_secret, reveal_ssh_key,
    revoke_api_key, set_device_hostname, set_secret, set_secret_devices, set_secret_permissions,
    set_secret_value, set_ssh_endpoint, set_ssh_endpoint_devices, set_ssh_endpoint_enabled,
    set_ssh_endpoint_tags, set_ssh_key, set_ssh_key_devices, set_ssh_key_tags, set_ssh_key_value,
    touch_device, update_api_key_permissions, update_email, update_password, update_ssh_endpoint,
};
pub use spacetimedb_sdk::{DbContext, Table};
