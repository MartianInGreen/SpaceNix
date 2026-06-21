//! Re-exports of the most common `module_bindings` traits and the
//! `spacetimedb_sdk::DbContext` / `Table` traits so consumers can just
//! `use crate::bindings::*;` and forget about the per-table imports.
//!
//! Only items that are actually used by the TUI / CLI right now are kept
//! here — the generated module exposes many more.

#![allow(unused_imports)]

pub use module_bindings::{
    DeviceMetric, DeviceMetricSample, DeviceMetricsReport, SshEndpointMetadata, SshKeyMetadata,
    SshKeyValue, SshRelayDeviceMetadata, SshRelaySessionMetadata, SshRelaySessionStatus,
    ack_ui_command, attach_ssh_relay_session_token, clear_ssh_relay_device,
    close_ssh_relay_session, create_api_key, create_folder, delete_device, delete_file,
    delete_secret, delete_ssh_endpoint, delete_ssh_key, open_ssh_relay_session, register_device,
    rename_device, rename_file, report_device_metrics, reveal_secret, reveal_ssh_key,
    revoke_api_key, send_ui_event, set_device_hostname, set_device_metrics_retention, set_secret,
    set_secret_devices, set_secret_permissions, set_secret_value, set_ssh_endpoint,
    set_ssh_endpoint_devices, set_ssh_endpoint_enabled, set_ssh_endpoint_tags, set_ssh_key,
    set_ssh_key_devices, set_ssh_key_tags, set_ssh_key_value,     set_ssh_relay_device, touch_device,
    update_api_key_permissions, update_email, update_password, update_ssh_endpoint,
};
pub use module_bindings::{
    MyApiKeysTableAccess, MyDeviceMetricsTableAccess, MyDevicesTableAccess, MyFilesTableAccess,
    MySecretsTableAccess, MySshEndpointsTableAccess, MySshKeysTableAccess, MySshRelayDeviceTableAccess,
    MySshRelaySessionsTableAccess, MyUiCommandsTableAccess, MyUserTableAccess, UiEventTableAccess,
};
pub use spacetimedb_sdk::{DbContext, EventTable, Table};
