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

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sysinfo::{Disks, Networks, System};
use tokio::sync::oneshot;
use tokio::time::{Instant, interval_at};

use crate::auth::conn::ConnState;
use crate::bindings::*;
use crate::config::Config;
use crate::store::device::LocalDevice;

/// Throttle for repeating the "metrics upload failed" diagnostic so we don't
/// spam the log on a sticky failure (e.g. un-republished server schema).
const REPORT_FAIL_LOG_THROTTLE: Duration = Duration::from_secs(5 * 60);

pub const REPORT_INTERVAL: Duration = Duration::from_secs(30);
const STARTUP_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct MetricsSample {
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    /// Cumulative bytes received across all *physical* interfaces. Tunnels
    /// (tailscale, wireguard, docker bridges, veth, …) are excluded so
    /// tunneled traffic isn't counted twice.
    pub net_rx_bytes: u64,
    /// Cumulative bytes transmitted across all *physical* interfaces.
    pub net_tx_bytes: u64,
    /// Instantaneous receive rate in bytes/sec, derived from the delta
    /// between this sample and the previous one. `0` for the first sample.
    pub net_rx_bps: f64,
    /// Instantaneous transmit rate in bytes/sec. `0` for the first sample.
    pub net_tx_bps: f64,
    pub storage_sync_root_used_bytes: u64,
    pub storage_sync_root_total_bytes: u64,
    pub storage_system_used_bytes: u64,
    pub storage_system_total_bytes: u64,
    pub sync_root_path: String,
}

pub struct MetricsCollector {
    system: System,
    disks: Disks,
    networks: Networks,
    sync_root: Option<std::path::PathBuf>,
    last_sample: Option<MetricsSample>,
    last_refresh: Option<Instant>,
}

impl MetricsCollector {
    pub fn new(sync_root: Option<std::path::PathBuf>) -> Self {
        let mut system = System::new();
        let mut disks = Disks::new_with_refreshed_list();
        let _ = disks.refresh();
        let mut networks = Networks::new_with_refreshed_list();
        let _ = networks.refresh();
        // Seed the CPU counters so the *next* refresh yields a real
        // `global_cpu_usage()` value. sysinfo computes the per-CPU
        // percentage from the delta between two snapshots, so the
        // very first call returns 0 and the second call returns the
        // value over whatever time elapsed between them.
        system.refresh_cpu_usage();
        system.refresh_memory();
        Self {
            system,
            disks,
            networks,
            sync_root,
            last_sample: None,
            last_refresh: None,
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
        // sysinfo's `Networks` is a `HashMap<String, NetworkData>`.
        // Iterate by `&str` so we can pass a borrowed name to
        // `is_virtual_interface` without an extra allocation.
        let names: Vec<&str> = self
            .networks
            .keys()
            .map(|s| s.as_str())
            .filter(|name| !is_virtual_interface(name))
            .collect();
        for name in names {
            if let Some(data) = self.networks.get(name) {
                net_rx = net_rx.saturating_add(data.total_received());
                net_tx = net_tx.saturating_add(data.total_transmitted());
            }
        }
        // sysinfo's cumulative counters reset on interface bounce, so guard
        // against the "monotonicity" assumption the server depends on.
        if let Some(prev) = &self.last_sample {
            net_rx = net_rx.max(prev.net_rx_bytes);
            net_tx = net_tx.max(prev.net_tx_bytes);
        }

        // Compute instantaneous bytes/sec from the delta over the elapsed
        // wall-clock time. The first sample has no prior, so we report 0
        // — the UI shows "0 B/s" until the second tick lands.
        let now = Instant::now();
        let (net_rx_bps, net_tx_bps) = match (self.last_refresh, self.last_sample.as_ref()) {
            (Some(prev_t), Some(prev_s))
                if prev_t != now =>
            {
                let secs = now.duration_since(prev_t).as_secs_f64().max(0.001);
                let rx_delta = net_rx.saturating_sub(prev_s.net_rx_bytes) as f64;
                let tx_delta = net_tx.saturating_sub(prev_s.net_tx_bytes) as f64;
                (rx_delta / secs, tx_delta / secs)
            }
            _ => (0.0, 0.0),
        };
        self.last_refresh = Some(now);

        let sync_root_path = self
            .sync_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let (sync_used, sync_total) =
            disk_usage_for_path(&self.disks, self.sync_root.as_deref());
        let (sys_used, sys_total) = primary_disk_usage(&self.disks);
        let sample = MetricsSample {
            cpu_percent,
            ram_used_bytes,
            ram_total_bytes,
            swap_used_bytes,
            swap_total_bytes,
            net_rx_bytes: net_rx,
            net_tx_bytes: net_tx,
            net_rx_bps,
            net_tx_bps,
            storage_sync_root_used_bytes: sync_used,
            storage_sync_root_total_bytes: sync_total,
            storage_system_used_bytes: sys_used,
            storage_system_total_bytes: sys_total,
            sync_root_path,
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
        Self::new(None)
    }
}

/// True for interfaces whose bytes also appear on a *physical* interface
/// (tunnels, bridges, virtual Ethernet, etc). Counting these alongside
/// `eth0` / `wlan0` double-counts every packet that travels through them.
///
/// The denylist covers the names we see in practice on Linux + macOS:
/// loopback, TUN/TAP, WireGuard, Tailscale, Docker bridge + veth, KVM /
/// virtio bridges, VPN/overlay (awdl, llw, ipv6 tunnels), generic
/// virtual Ethernet (veth*, vnet*, macvlan/tap), and the kernel
/// `bridge` master. Add more here if your platform uses a different
/// naming convention.
fn is_virtual_interface(name: &str) -> bool {
    if name == "lo" {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "lo",
        "tun",
        "tap",
        "wg",
        "tailscale",
        "docker",
        "br-",
        "veth",
        "virbr",
        "vnet",
        "awdl",
        "llw",
        "bridge",
        "ipv6tnl",
        "sit",
        "ip6",
        "macvtap",
        "macvlan",
        "vxlan",
        "geneve",
        "gretap",
        "erspan",
        "tunl",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Storage stats for the disk that hosts `path`, if any. Falls back to the
/// largest non-removable disk so we never report zeros when the path doesn't
/// exist yet (e.g. before the first sync).
fn disk_usage_for_path(disks: &Disks, path: Option<&Path>) -> (u64, u64) {
    if let Some(path) = path {
        if let Some(stats) = disk_for_path(disks, path) {
            return stats;
        }
    }
    primary_disk_usage(disks)
}

fn disk_for_path(disks: &Disks, path: &Path) -> Option<(u64, u64)> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut best_len: usize = 0;
    let mut best: Option<(u64, u64)> = None;
    for disk in disks {
        if disk.is_removable() {
            continue;
        }
        let mount = disk.mount_point();
        let mount_canon = std::fs::canonicalize(mount).unwrap_or_else(|_| mount.to_path_buf());
        if !path_within(&target, &mount_canon) {
            continue;
        }
        let mount_len = mount_canon.as_os_str().len();
        if mount_len >= best_len {
            best_len = mount_len;
            let total = disk.total_space();
            let used = total.saturating_sub(disk.available_space());
            best = Some((used, total));
        }
    }
    best
}

fn path_within(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// Storage stats for the largest non-removable disk on the system. This is
/// the "system disk" view — independent of where `sync_root` happens to live.
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
        storage_sync_root_used_bytes: sample.storage_sync_root_used_bytes,
        storage_sync_root_total_bytes: sample.storage_sync_root_total_bytes,
        storage_system_used_bytes: sample.storage_system_used_bytes,
        storage_system_total_bytes: sample.storage_system_total_bytes,
        sync_root_path: sample.sync_root_path.clone(),
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

pub async fn collect_once(
    config: &Config,
    state: &ConnState,
    device_id: u64,
) -> Result<MetricsSample> {
    // Two refreshes with a 500ms gap so sysinfo's CPU usage is real,
    // not a process-startup artifact. Run on a blocking thread because
    // sysinfo walks /proc and we don't want it on a tokio worker.
    let sync_root = Some(config.sync_root.clone());
    let sample = tokio::task::spawn_blocking(move || -> Result<MetricsSample> {
        let mut collector = MetricsCollector::new(sync_root);
        // First refresh seeds the CPU counters; second one (after a
        // short sleep) yields a meaningful `global_cpu_usage()`.
        let _ = collector.refresh();
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
    config: Arc<Config>,
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

    // Build a single collector that lives for the whole reporter. A
    // fresh `MetricsCollector` per tick is the bug behind the "CPU stuck
    // at 13.2%" symptom: sysinfo needs two CPU-counter refreshes
    // separated by real wall-clock time, and a brand-new `System` only
    // has its first refresh in its history — so on the *next* tick the
    // computed percentage is dominated by process-startup deltas.
    //
    // Wrap it in a `Mutex` so we can hand it briefly to a blocking
    // thread (the refresh walks /proc, which we don't want to do on a
    // tokio worker) without juggling moves back and forth.
    let collector = std::sync::Arc::new(std::sync::Mutex::new(
        MetricsCollector::new(Some(config.sync_root.clone())),
    ));

    // Warm-up pass: a second refresh ~500ms after construction makes
    // `global_cpu_usage()` return a meaningful percentage on the first
    // *published* sample. We then reset the per-tick delta state so
    // the warmup doesn't leak into the first real sample.
    let warmup_collector = std::sync::Arc::clone(&collector);
    tokio::task::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(mut c) = warmup_collector.lock() {
            let _ = c.refresh();
        }
    })
    .await
    .ok();
    *collector.lock().expect("collector mutex poisoned") =
        MetricsCollector::new(Some(config.sync_root.clone()));

    let start = Instant::now() + STARTUP_DELAY;
    let mut ticker = interval_at(start, REPORT_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    tokio::pin!(cancel);

    let mut last_fail_log: Option<Instant> = None;
    let mut last_fail_msg: Option<String> = None;

    loop {
        tokio::select! {
            _ = &mut cancel => {
                tracing::info!("metrics reporter cancelled");
                return Ok(());
            }
            _ = ticker.tick() => {
                let device_id = local.id;
                let collector = std::sync::Arc::clone(&collector);
                let sample = tokio::task::spawn_blocking(move || {
                    let mut c = collector.lock().expect("collector mutex poisoned");
                    c.refresh()
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
                match report_sample(&state, device_id, report).await {
                    Ok(()) => {
                        tracing::debug!(
                            device_id,
                            cpu = sample.cpu_percent,
                            ram_used = sample.ram_used_bytes,
                            "metrics reported"
                        );
                    }
                    Err(err) => {
                        let msg = format!("{err:#}");
                        let now = Instant::now();
                        let should_log = match last_fail_log {
                            None => true,
                            Some(prev)
                                if now.duration_since(prev) >= REPORT_FAIL_LOG_THROTTLE =>
                            {
                                true
                            }
                            Some(_) => last_fail_msg.as_deref() != Some(&msg),
                        };
                        if should_log {
                            let module = config.stdb_module.as_str();
                            tracing::error!(
                                device_id,
                                error = %msg,
                                "failed to push device metrics — this usually means the \
                                 SpacetimeDB module on the server is out of date with the \
                                 client. Run `spacetime publish {module} --yes` and restart \
                                 the service.",
                                module = module,
                            );
                            last_fail_log = Some(now);
                            last_fail_msg = Some(msg);
                        } else {
                            tracing::debug!(
                                device_id,
                                error = %msg,
                                "failed to push device metrics (throttled)"
                            );
                        }
                    }
                }
            }
        }
    }
}
