//! Periodic device metrics collection and reporting.
//!
//! The SpacetimeDB module is a deterministic sandbox, so system-stats
//! collection has to happen in the client. This module owns a
//! [`sysinfo::System`] handle (plus a [`sysinfo::Disks`] and
//! [`sysinfo::Networks`] pair for the v0.32 API), samples CPU / RAM /
//! swap / network / storage on a fixed interval, and pushes each sample
//! to the server via the `report_device_metrics` reducer.
//!
//! Two entry points are provided:
//!
//! - [`run_reporter`] — a long-lived tokio task meant to run alongside the
//!   background service. It subscribes to `my_devices` to learn the device
//!   id of the current machine and then reports on an interval until
//!   cancelled.
//! - [`collect_once`] — a one-shot collector used by the TUI / CLI so the
//!   local device can be exercised on demand (e.g. for `device status`).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sysinfo::{Disks, Networks, System};
use tokio::sync::oneshot;
use tokio::time::{Instant, interval_at};

use crate::auth::conn::ConnState;
use crate::bindings::*;
use crate::store::device::LocalDevice;

pub const REPORT_INTERVAL: Duration = Duration::from_secs(30);
const STARTUP_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct MetricsSample {
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub storage_used_bytes: u64,
    pub storage_total_bytes: u64,
}

pub struct MetricsCollector {
    system: System,
    disks: Disks,
    networks: Networks,
    last_sample: Option<MetricsSample>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        let mut system = System::new();
        // First refresh only seeds the baseline; CPU stats need two passes
        // before `global_cpu_usage()` returns a real number.
        system.refresh_cpu_usage();
        system.refresh_memory();
        let mut disks = Disks::new_with_refreshed_list();
        let _ = disks.refresh();
        let mut networks = Networks::new_with_refreshed_list();
        let _ = networks.refresh();
        Self {
            system,
            disks,
            networks,
            last_sample: None,
        }
    }

    pub fn refresh(&mut self) -> MetricsSample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let _ = self.disks.refresh();
        let _ = self.networks.refresh();

        let cpu_percent = self.system.global_cpu_usage().clamp(0.0, 100.0);

        let ram_used_bytes = self.system.used_memory();
        let ram_total_bytes = self.system.total_memory();
        let swap_used_bytes = self.system.used_swap();
        let swap_total_bytes = self.system.total_swap();

        let mut net_rx: u64 = 0;
        let mut net_tx: u64 = 0;
        for (iface_name, data) in &self.networks {
            if iface_name == "lo" || iface_name.starts_with("lo:") {
                continue;
            }
            net_rx = net_rx.saturating_add(data.total_received());
            net_tx = net_tx.saturating_add(data.total_transmitted());
        }
        // sysinfo's cumulative counters reset on interface bounce, so guard
        // against the "monotonicity" assumption the server depends on.
        if let Some(prev) = &self.last_sample {
            net_rx = net_rx.max(prev.net_rx_bytes);
            net_tx = net_tx.max(prev.net_tx_bytes);
        }

        let (storage_used, storage_total) = primary_disk_usage(&self.disks);
        let sample = MetricsSample {
            cpu_percent,
            ram_used_bytes,
            ram_total_bytes,
            swap_used_bytes,
            swap_total_bytes,
            net_rx_bytes: net_rx,
            net_tx_bytes: net_tx,
            storage_used_bytes: storage_used,
            storage_total_bytes: storage_total,
        };
        self.last_sample = Some(sample.clone());
        sample
    }

    #[allow(dead_code)]
    pub fn last_sample(&self) -> Option<&MetricsSample> {
        self.last_sample.as_ref()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

fn primary_disk_usage(disks: &Disks) -> (u64, u64) {
    let mut best: Option<(u64, u64)> = None;
    for disk in disks {
        if !disk.is_removable() {
            let total = disk.total_space();
            let free = disk.available_space();
            let used = total.saturating_sub(free);
            best = Some(match best {
                Some((u, t)) if total >= t => (u, t),
                _ => (used, total),
            });
        }
    }
    best.unwrap_or((0, 0))
}

fn sample_to_report(sample: &MetricsSample) -> DeviceMetricsReport {
    DeviceMetricsReport {
        cpu_percent: sample.cpu_percent,
        ram_used_bytes: sample.ram_used_bytes,
        ram_total_bytes: sample.ram_total_bytes,
        swap_used_bytes: sample.swap_used_bytes,
        swap_total_bytes: sample.swap_total_bytes,
        net_rx_bytes: sample.net_rx_bytes,
        net_tx_bytes: sample.net_tx_bytes,
        storage_used_bytes: sample.storage_used_bytes,
        storage_total_bytes: sample.storage_total_bytes,
    }
}

async fn report_sample(
    state: &ConnState,
    device_id: u64,
    report: DeviceMetricsReport,
) -> Result<()> {
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .report_device_metrics_then(device_id, report, move |_ctx, res| {
            let _ = tx.send(res);
        })
        .map_err(|err| anyhow::anyhow!("invoking report_device_metrics: {err:?}"))?;
    match rx.await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(msg))) => Err(anyhow::anyhow!("report_device_metrics rejected: {msg}")),
        Ok(Err(err)) => Err(anyhow::anyhow!("report_device_metrics failed: {err:?}")),
        Err(_) => Err(anyhow::anyhow!("report_device_metrics callback dropped")),
    }
}

pub async fn collect_once(state: &ConnState, device_id: u64) -> Result<MetricsSample> {
    // sysinfo needs a second refresh to compute CPU usage. Do the seeding
    // refresh on a blocking thread, sleep, then take the real sample.
    let sample = tokio::task::spawn_blocking(|| -> Result<MetricsSample> {
        let mut collector = MetricsCollector::new();
        std::thread::sleep(Duration::from_millis(500));
        Ok(collector.refresh())
    })
    .await
    .map_err(|err| anyhow::anyhow!("collector task panicked: {err}"))??;
    let report = sample_to_report(&sample);
    report_sample(state, device_id, report).await?;
    Ok(sample)
}

/// Long-lived background task: keeps a metrics collector ticking and pushes
/// samples to the server. Cancels itself when `cancel` resolves.
pub async fn run_reporter(
    state: Arc<ConnState>,
    local: LocalDevice,
    cancel: oneshot::Receiver<()>,
) -> Result<()> {
    state
        .conn
        .subscription_builder()
        .on_error(|_ctx, err| tracing::error!(?err, "metrics subscription error"))
        .subscribe(["SELECT * FROM my_devices"]);
    // Give the subscription a moment to apply so we know our device still
    // exists, but don't block forever if STDB is unreachable.
    tokio::time::sleep(STARTUP_DELAY).await;

    if state
        .conn
        .db()
        .my_devices()
        .iter()
        .all(|d| d.id != local.id)
    {
        anyhow::bail!(
            "device #{} ({}) is not in the current user's devices; \
             run `spacenix device` to re-select",
            local.id,
            local.name
        );
    }

    let start = Instant::now() + STARTUP_DELAY;
    let mut ticker = interval_at(start, REPORT_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    tokio::pin!(cancel);

    loop {
        tokio::select! {
            _ = &mut cancel => {
                tracing::info!("metrics reporter cancelled");
                return Ok(());
            }
            _ = ticker.tick() => {
                let device_id = local.id;
                let sample = tokio::task::spawn_blocking({
                    let mut c = MetricsCollector::new();
                    move || c.refresh()
                })
                .await;
                let sample = match sample {
                    Ok(s) => s,
                    Err(err) => {
                        tracing::warn!(?err, "metrics collector task panicked");
                        continue;
                    }
                };
                let report = sample_to_report(&sample);
                if let Err(err) = report_sample(&state, device_id, report).await {
                    tracing::warn!(?err, "failed to push device metrics");
                } else {
                    tracing::debug!(
                        device_id,
                        cpu = sample.cpu_percent,
                        ram_used = sample.ram_used_bytes,
                        "metrics reported"
                    );
                }
            }
        }
    }
}
