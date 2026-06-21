use spacetimedb::{ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view};

use crate::device::{default_metrics_retention, device as _, device__view as _};
use crate::user::{require_registered_user, session__view as _};

const MIN_SAMPLE_GAP_MICROS: i64 = 5 * 1_000_000;

#[spacetimedb::table(accessor = device_metric)]
pub struct DeviceMetric {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub device_id: u64,
    pub recorded_at: Timestamp,
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    /// Storage used on the disk that backs the device's `sync_root` (in bytes).
    pub storage_sync_root_used_bytes: u64,
    /// Total space on the disk that backs the device's `sync_root` (in bytes).
    pub storage_sync_root_total_bytes: u64,
    /// Storage used on the system / root disk (in bytes).
    pub storage_system_used_bytes: u64,
    /// Total space on the system / root disk (in bytes).
    pub storage_system_total_bytes: u64,
    /// Path the client reported as its `sync_root`, or empty if unknown.
    pub sync_root_path: String,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct DeviceMetricSample {
    pub id: u64,
    pub device_id: u64,
    pub recorded_at: Timestamp,
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub storage_sync_root_used_bytes: u64,
    pub storage_sync_root_total_bytes: u64,
    pub storage_system_used_bytes: u64,
    pub storage_system_total_bytes: u64,
    pub sync_root_path: String,
}

impl From<DeviceMetric> for DeviceMetricSample {
    fn from(m: DeviceMetric) -> Self {
        Self {
            id: m.id,
            device_id: m.device_id,
            recorded_at: m.recorded_at,
            cpu_percent: m.cpu_percent,
            ram_used_bytes: m.ram_used_bytes,
            ram_total_bytes: m.ram_total_bytes,
            swap_used_bytes: m.swap_used_bytes,
            swap_total_bytes: m.swap_total_bytes,
            net_rx_bytes: m.net_rx_bytes,
            net_tx_bytes: m.net_tx_bytes,
            storage_sync_root_used_bytes: m.storage_sync_root_used_bytes,
            storage_sync_root_total_bytes: m.storage_sync_root_total_bytes,
            storage_system_used_bytes: m.storage_system_used_bytes,
            storage_system_total_bytes: m.storage_system_total_bytes,
            sync_root_path: m.sync_root_path,
        }
    }
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct DeviceMetricsReport {
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub storage_sync_root_used_bytes: u64,
    pub storage_sync_root_total_bytes: u64,
    pub storage_system_used_bytes: u64,
    pub storage_system_total_bytes: u64,
    pub sync_root_path: String,
}

fn require_owned_device(ctx: &ReducerContext, device_id: u64) -> Result<(), String> {
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
    Ok(())
}

fn validate_report(report: &DeviceMetricsReport) -> Result<(), String> {
    if !report.cpu_percent.is_finite() || report.cpu_percent < 0.0 || report.cpu_percent > 100.0 {
        return Err("cpu_percent must be between 0 and 100".to_string());
    }
    if report.ram_used_bytes > report.ram_total_bytes {
        return Err("ram_used_bytes cannot exceed ram_total_bytes".to_string());
    }
    if report.swap_used_bytes > report.swap_total_bytes {
        return Err("swap_used_bytes cannot exceed swap_total_bytes".to_string());
    }
    if report.storage_sync_root_used_bytes > report.storage_sync_root_total_bytes {
        return Err("storage_sync_root_used_bytes cannot exceed storage_sync_root_total_bytes".to_string());
    }
    if report.storage_system_used_bytes > report.storage_system_total_bytes {
        return Err("storage_system_used_bytes cannot exceed storage_system_total_bytes".to_string());
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn report_device_metrics(
    ctx: &ReducerContext,
    device_id: u64,
    report: DeviceMetricsReport,
) -> Result<(), String> {
    require_owned_device(ctx, device_id)?;
    validate_report(&report)?;

    let now = ctx.timestamp;
    let cutoff = Timestamp::from_micros_since_unix_epoch(
        now.to_micros_since_unix_epoch() - MIN_SAMPLE_GAP_MICROS,
    );
    if let Some(recent) = ctx
        .db
        .device_metric()
        .device_id()
        .filter(device_id)
        .find(|m| m.recorded_at > cutoff)
    {
        if recent.net_rx_bytes <= report.net_rx_bytes
            && recent.net_tx_bytes <= report.net_tx_bytes
            && report.cpu_percent == recent.cpu_percent
            && report.ram_used_bytes == recent.ram_used_bytes
            && report.storage_sync_root_used_bytes == recent.storage_sync_root_used_bytes
            && report.storage_system_used_bytes == recent.storage_system_used_bytes
        {
            return Ok(());
        }
    }

    ctx.db.device_metric().insert(DeviceMetric {
        id: 0,
        device_id,
        recorded_at: now,
        cpu_percent: report.cpu_percent,
        ram_used_bytes: report.ram_used_bytes,
        ram_total_bytes: report.ram_total_bytes,
        swap_used_bytes: report.swap_used_bytes,
        swap_total_bytes: report.swap_total_bytes,
        net_rx_bytes: report.net_rx_bytes,
        net_tx_bytes: report.net_tx_bytes,
        storage_sync_root_used_bytes: report.storage_sync_root_used_bytes,
        storage_sync_root_total_bytes: report.storage_sync_root_total_bytes,
        storage_system_used_bytes: report.storage_system_used_bytes,
        storage_system_total_bytes: report.storage_system_total_bytes,
        sync_root_path: report.sync_root_path,
    });

    let retention_micros = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .and_then(|d| d.metrics_retention)
        .unwrap_or_else(default_metrics_retention)
        .to_micros();
    let prune_cutoff = Timestamp::from_micros_since_unix_epoch(
        now.to_micros_since_unix_epoch() - retention_micros,
    );
    let stale: Vec<u64> = ctx
        .db
        .device_metric()
        .iter()
        .filter(|m| m.device_id == device_id && m.recorded_at < prune_cutoff)
        .map(|m| m.id)
        .collect();
    for id in stale {
        ctx.db.device_metric().id().delete(id);
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn prune_device_metrics(
    ctx: &ReducerContext,
    device_id: u64,
    older_than: Timestamp,
) -> Result<(), String> {
    require_owned_device(ctx, device_id)?;
    let stale: Vec<u64> = ctx
        .db
        .device_metric()
        .device_id()
        .filter(device_id)
        .filter(|m| m.recorded_at < older_than)
        .map(|m| m.id)
        .collect();
    for id in stale {
        ctx.db.device_metric().id().delete(id);
    }
    Ok(())
}

#[view(accessor = my_device_metrics, public)]
fn my_device_metrics(ctx: &ViewContext) -> Vec<DeviceMetricSample> {
    let Some(user) = ctx
        .db
        .session()
        .connection()
        .find(ctx.sender())
        .map(|s| s.user)
    else {
        return Vec::new();
    };
    let mut samples: Vec<DeviceMetricSample> = Vec::new();
    for device in ctx.db.device().owner().filter(user) {
        for m in ctx.db.device_metric().device_id().filter(device.id) {
            samples.push(DeviceMetricSample::from(m));
        }
    }
    samples
}
