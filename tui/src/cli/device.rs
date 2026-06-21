//! `spacenix device …` — manage registered devices.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use tokio::sync::oneshot;

use crate::auth::conn;
use crate::bindings::*;
use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// List registered devices.
    List,

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

    /// Delete a device.
    Delete { id: u64 },
}

pub async fn run(config: Arc<Config>, cmd: DeviceCommand) -> Result<ExitCode> {
    let state = connect(&config).await?;
    match cmd {
        DeviceCommand::List => list(&state).await,
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
        .subscribe(["SELECT * FROM my_devices", "SELECT * FROM my_ssh_endpoints"]);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(state)
}

async fn list(state: &conn::ConnState) -> Result<ExitCode> {
    let mut rows: Vec<_> = state.conn.db().my_devices().iter().collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    if rows.is_empty() {
        println!("(no devices)");
    } else {
        for d in rows {
            let hostname = d.hostname.as_deref().unwrap_or("-");
            let last_seen = d
                .last_seen_at
                .map(|ts| format!("{:?}", ts))
                .unwrap_or_else(|| "never".to_string());
            println!(
                "#{:<6}\t{}\thost={}\tlast_seen={}",
                d.id, d.name, hostname, last_seen
            );
        }
    }
    Ok(ExitCode::from(0))
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
