use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table, TimeDuration, Timestamp, ViewContext, view};

use crate::user::{require_registered_user, session__view as _};

/// Default retention for device metrics when a device row doesn't override
/// it. One hour matches the previous hard-coded behaviour.
pub const DEFAULT_METRICS_RETENTION_MICROS: i64 = 60 * 60 * 1_000_000;
const MIN_METRICS_RETENTION_MICROS: i64 = 60 * 1_000_000;
const MAX_METRICS_RETENTION_MICROS: i64 = 30 * 24 * 60 * 60 * 1_000_000;

/// Returns the default [`TimeDuration`] for metrics retention. Wraps
/// [`DEFAULT_METRICS_RETENTION_MICROS`] in a non-const constructor.
pub fn default_metrics_retention() -> TimeDuration {
    TimeDuration::from_micros(DEFAULT_METRICS_RETENTION_MICROS)
}

#[spacetimedb::table(accessor = device)]
pub struct Device {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    pub name: String,
    pub hostname: Option<String>,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
    /// How long device metrics samples are kept server-side for this device.
    /// `None` falls back to [`DEFAULT_METRICS_RETENTION`].
    pub metrics_retention: Option<TimeDuration>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct DeviceMetadata {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub hostname: Option<String>,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
    pub metrics_retention: Option<TimeDuration>,
}

impl From<Device> for DeviceMetadata {
    fn from(d: Device) -> Self {
        Self {
            id: d.id,
            owner: d.owner,
            name: d.name,
            hostname: d.hostname,
            created_at: d.created_at,
            last_seen_at: d.last_seen_at,
            metrics_retention: d.metrics_retention,
        }
    }
}

fn validate_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > 128 {
        return Err("name must be 128 characters or fewer".to_string());
    }
    Ok(name)
}

fn normalize_hostname(hostname: Option<String>) -> Result<Option<String>, String> {
    let Some(h) = hostname else {
        return Ok(None);
    };
    let h = h.trim().to_string();
    if h.is_empty() {
        return Ok(None);
    }
    if h.len() > 256 {
        return Err("hostname must be 256 characters or fewer".to_string());
    }
    Ok(Some(h))
}

#[spacetimedb::reducer]
pub fn register_device(
    ctx: &ReducerContext,
    name: String,
    hostname: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_name(name)?;
    let hostname = normalize_hostname(hostname)?;
    ctx.db.device().insert(Device {
        id: 0,
        owner: user.identity,
        name,
        hostname,
        created_at: ctx.timestamp,
        last_seen_at: None,
        metrics_retention: None,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn rename_device(ctx: &ReducerContext, device_id: u64, name: String) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let name = validate_name(name)?;
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    device.name = name;
    ctx.db.device().id().update(device);
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_device_hostname(
    ctx: &ReducerContext,
    device_id: u64,
    hostname: Option<String>,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let hostname = normalize_hostname(hostname)?;
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    device.hostname = hostname;
    ctx.db.device().id().update(device);
    Ok(())
}

#[spacetimedb::reducer]
pub fn touch_device(ctx: &ReducerContext, device_id: u64) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    device.last_seen_at = Some(ctx.timestamp);
    ctx.db.device().id().update(device);
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_device(ctx: &ReducerContext, device_id: u64) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    ctx.db.device().id().delete(device_id);
    Ok(())
}

/// Set the metrics retention for a device. `retention_secs == 0` clears the
/// override and falls back to the server default. Bounded to one minute on
/// the low end (so the prune pass always has work to do) and 30 days on the
/// high end.
#[spacetimedb::reducer]
pub fn set_device_metrics_retention(
    ctx: &ReducerContext,
    device_id: u64,
    retention_secs: u64,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    device.metrics_retention = if retention_secs == 0 {
        None
    } else {
        let micros = (retention_secs as i64).saturating_mul(1_000_000);
        if micros < MIN_METRICS_RETENTION_MICROS {
            return Err(format!(
                "retention must be at least {} seconds",
                MIN_METRICS_RETENTION_MICROS / 1_000_000
            ));
        }
        if micros > MAX_METRICS_RETENTION_MICROS {
            return Err(format!(
                "retention must be at most {} seconds",
                MAX_METRICS_RETENTION_MICROS / 1_000_000
            ));
        }
        Some(TimeDuration::from_micros(micros))
    };
    ctx.db.device().id().update(device);
    Ok(())
}

#[view(accessor = my_devices, public)]
fn my_devices(ctx: &ViewContext) -> Vec<DeviceMetadata> {
    let Some(user) = ctx.db.session().connection().find(ctx.sender()).map(|s| s.user) else {
        return Vec::new();
    };
    ctx.db
        .device()
        .owner()
        .filter(user)
        .map(DeviceMetadata::from)
        .collect()
}
