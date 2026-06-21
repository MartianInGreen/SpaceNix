//! `spacenix device …` — manage registered devices.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use tokio::sync::oneshot;

use crate::auth::conn;
use crate::bindings::*;
use crate::config::Config;
use crate::store::device::LocalDevice;

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// List registered devices and their latest metrics.
    List,

    /// Collect and report this device's current metrics, then print them.
    Status,

    /// Register this or another device by name.
    Register {
        name: String,
        #[arg(long)]
        hostname: Option<String>,
    },

    /// Rename a device.
    Rename { id: u64, name: String },

    /// Set or clear a device hostname.
    Hostname {
        id: u64,
        #[arg(long)]
        clear: bool,
        hostname: Option<String>,
    },

    /// Mark a device as seen now.
    Touch { id: u64 },

    /// Set or clear metrics retention for a device, in seconds. Pass `0` to
    /// clear the override and fall back to the server default (1 hour).
    Retention {
        id: u64,
        /// New retention in seconds. Must be at least 60 and at most 30 days.
        seconds: u64,
    },

    /// Delete a device.
    Delete { id: u64 },
}

pub async fn run(config: Arc<Config>, cmd: DeviceCommand) -> Result<ExitCode> {
    let state = connect(&config).await?;
    match cmd {
        DeviceCommand::List => list(&state).await,
        DeviceCommand::Status => report_and_print(config.as_ref(), &state).await,
        DeviceCommand::Register { name, hostname } => {
            call_unit("register_device", |tx| {
                state
                    .conn
                    .reducers()
                    .register_device_then(name, hostname, move |_ctx, res| {
                        let _ = tx.send(res);
                    })
            })
            .await?;
            println!("✓ device registered");
            Ok(ExitCode::from(0))
        }
        DeviceCommand::Rename { id, name } => {
            call_unit("rename_device", |tx| {
                state
                    .conn
                    .reducers()
                    .rename_device_then(id, name, move |_ctx, res| {
                        let _ = tx.send(res);
                    })
            })
            .await?;
            println!("✓ device #{id} renamed");
            Ok(ExitCode::from(0))
        }
        DeviceCommand::Hostname {
            id,
            clear,
            hostname,
        } => {
            if clear && hostname.is_some() {
                anyhow::bail!("pass either --clear or a hostname, not both");
            }
            let hostname = if clear { None } else { hostname };
            call_unit("set_device_hostname", |tx| {
                state
                    .conn
                    .reducers()
                    .set_device_hostname_then(id, hostname, move |_ctx, res| {
                        let _ = tx.send(res);
                    })
            })
            .await?;
            println!("✓ device #{id} hostname updated");
            Ok(ExitCode::from(0))
        }
        DeviceCommand::Touch { id } => {
            call_unit("touch_device", |tx| {
                state
                    .conn
                    .reducers()
                    .touch_device_then(id, move |_ctx, res| {
                        let _ = tx.send(res);
                    })
            })
            .await?;
            println!("✓ device #{id} marked seen");
            Ok(ExitCode::from(0))
        }
        DeviceCommand::Retention { id, seconds } => {
            call_unit("set_device_metrics_retention", |tx| {
                state
                    .conn
                    .reducers()
                    .set_device_metrics_retention_then(id, seconds, move |_ctx, res| {
                        let _ = tx.send(res);
                    })
            })
            .await?;
            if seconds == 0 {
                println!("✓ device #{id} retention cleared (server default applies)");
            } else {
                println!(
                    "✓ device #{id} retention set to {}",
                    humantime::format_duration(Duration::from_secs(seconds))
                );
            }
            Ok(ExitCode::from(0))
        }
        DeviceCommand::Delete { id } => {
            call_unit("delete_device", |tx| {
                state
                    .conn
                    .reducers()
                    .delete_device_then(id, move |_ctx, res| {
                        let _ = tx.send(res);
                    })
            })
            .await?;
            println!("✓ device #{id} deleted");
            Ok(ExitCode::from(0))
        }
    }
}

async fn connect(config: &Config) -> Result<conn::ConnState> {
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?
        .context("not signed in — run `spacenix login` first")?;
    let state = conn::connect(config, Some(creds.token))?;
    state
        .conn
        .subscription_builder()
        .on_error(|_ctx, err| tracing::error!(?err, "devices subscription error"))
        .subscribe([
            "SELECT * FROM my_devices",
            "SELECT * FROM my_ssh_endpoints",
            "SELECT * FROM my_device_metrics",
        ]);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(state)
}

fn latest_metrics_per_device(state: &conn::ConnState) -> HashMap<u64, DeviceMetricSample> {
    let mut latest: HashMap<u64, DeviceMetricSample> = HashMap::new();
    for m in state.conn.db().my_device_metrics().iter() {
        let entry = latest.entry(m.device_id).or_insert_with(|| m.clone());
        if m.recorded_at > entry.recorded_at {
            *entry = m.clone();
        }
    }
    latest
}

fn format_metrics_line(m: &DeviceMetricSample) -> String {
    use crate::util::formatting;
    let ram_pct = percent(m.ram_used_bytes, m.ram_total_bytes);
    let swap_pct = percent(m.swap_used_bytes, m.swap_total_bytes);
    let sync_pct = percent(
        m.storage_sync_root_used_bytes,
        m.storage_sync_root_total_bytes,
    );
    let sys_pct = percent(
        m.storage_system_used_bytes,
        m.storage_system_total_bytes,
    );
    format!(
        "cpu {:>5.1}% | ram {:>5.1}% ({} / {}) | swap {:>5.1}% ({} / {}) | net {}↓ {}↑ | sync_root {:>5.1}% ({} / {}) | sys {:>5.1}% ({} / {})",
        m.cpu_percent,
        ram_pct,
        formatting::bytes(m.ram_used_bytes),
        formatting::bytes(m.ram_total_bytes),
        swap_pct,
        formatting::bytes(m.swap_used_bytes),
        formatting::bytes(m.swap_total_bytes),
        formatting::bytes(m.net_rx_bytes),
        formatting::bytes(m.net_tx_bytes),
        sync_pct,
        formatting::bytes(m.storage_sync_root_used_bytes),
        formatting::bytes(m.storage_sync_root_total_bytes),
        sys_pct,
        formatting::bytes(m.storage_system_used_bytes),
        formatting::bytes(m.storage_system_total_bytes),
    )
}

async fn list(state: &conn::ConnState) -> Result<ExitCode> {
    let mut rows: Vec<_> = state.conn.db().my_devices().iter().collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let metrics = latest_metrics_per_device(state);
    if rows.is_empty() {
        println!("(no devices)");
    } else {
    for d in rows {
        let hostname = d.hostname.as_deref().unwrap_or("-");
        let last_seen = d
            .last_seen_at
            .map(|ts| crate::util::formatting::short_ts(&ts))
            .unwrap_or_else(|| "never".to_string());
        let retention: String = match d.metrics_retention {
            Some(t) => humantime::format_duration(Duration::from_micros(t.to_micros() as u64))
                .to_string(),
            None => "default (1h)".to_string(),
        };
        println!(
            "#{:<6}\t{:<24}\thost={:<24}\tlast_seen={}\tretention={}",
            d.id, d.name, hostname, last_seen, retention
        );
        if let Some(m) = metrics.get(&d.id) {
            println!("         metrics: {}", format_metrics_line(m));
        } else {
            println!("         metrics: (no reports yet)");
        }
    }
    }
    Ok(ExitCode::from(0))
}

async fn report_and_print(config: &Config, state: &conn::ConnState) -> Result<ExitCode> {
    let local = LocalDevice::load(&config.device_file())?
        .context("no local device selected; run `spacenix` interactively first")?;
    if state
        .conn
        .db()
        .my_devices()
        .iter()
        .all(|d| d.id != local.id)
    {
        anyhow::bail!("device #{} is not owned by the current user", local.id);
    }
    let sample = crate::metrics::collect_once(config, state, local.id).await?;
    println!("device #{} ({})", local.id, local.name);
    println!("  cpu:     {:.1}%", sample.cpu_percent);
    println!(
        "  ram:     {} / {} ({:.1}%)",
        crate::util::formatting::bytes(sample.ram_used_bytes),
        crate::util::formatting::bytes(sample.ram_total_bytes),
        percent(sample.ram_used_bytes, sample.ram_total_bytes)
    );
    println!(
        "  swap:    {} / {} ({:.1}%)",
        crate::util::formatting::bytes(sample.swap_used_bytes),
        crate::util::formatting::bytes(sample.swap_total_bytes),
        percent(sample.swap_used_bytes, sample.swap_total_bytes)
    );
    println!(
        "  net:     {} rx / {} tx (cumulative)",
        crate::util::formatting::bytes(sample.net_rx_bytes),
        crate::util::formatting::bytes(sample.net_tx_bytes)
    );
    if sample.net_rx_bps > 0.0 || sample.net_tx_bps > 0.0 {
        println!(
            "  net speed: {} ↓ / {} ↑",
            crate::util::formatting::bytes_per_sec(sample.net_rx_bps),
            crate::util::formatting::bytes_per_sec(sample.net_tx_bps)
        );
    }
    if !sample.sync_root_path.is_empty() {
        println!("  sync_root: {}", sample.sync_root_path);
    }
    println!(
        "  storage (sync_root): {} / {} ({:.1}%)",
        crate::util::formatting::bytes(sample.storage_sync_root_used_bytes),
        crate::util::formatting::bytes(sample.storage_sync_root_total_bytes),
        percent(
            sample.storage_sync_root_used_bytes,
            sample.storage_sync_root_total_bytes
        )
    );
    println!(
        "  storage (system):    {} / {} ({:.1}%)",
        crate::util::formatting::bytes(sample.storage_system_used_bytes),
        crate::util::formatting::bytes(sample.storage_system_total_bytes),
        percent(
            sample.storage_system_used_bytes,
            sample.storage_system_total_bytes
        )
    );
    Ok(ExitCode::from(0))
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f32 / total as f32) * 100.0
    }
}

async fn call_unit<F, E>(name: &str, invoke: F) -> Result<()>
where
    F: FnOnce(oneshot::Sender<Result<Result<(), String>, E>>) -> spacetimedb_sdk::Result<()>,
    E: std::fmt::Debug + Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    invoke(tx).with_context(|| format!("invoking {name}"))?;
    match rx
        .await
        .with_context(|| format!("{name} callback dropped"))?
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => anyhow::bail!("{name} rejected: {err}"),
        Err(err) => anyhow::bail!("{name} failed: {err:?}"),
    }
}
