//! Resolve which registered SpaceNix device represents this machine.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::oneshot;

use crate::auth::conn::ConnState;
use crate::bindings::*;
use crate::config::Config;
use crate::store::device::LocalDevice;

#[derive(Clone, Debug)]
struct DeviceCandidate {
    id: u64,
    name: String,
    hostname: Option<String>,
}

pub async fn ensure_local_device(config: Arc<Config>, state: &ConnState) -> Result<LocalDevice> {
    state
        .conn
        .subscription_builder()
        .on_error(|_ctx, err| tracing::error!(?err, "devices subscription error"))
        .subscribe(["SELECT * FROM my_devices"]);
    tokio::time::sleep(Duration::from_millis(400)).await;

    if let Some(local) = LocalDevice::load(&config.device_file())? {
        if find_device(state, local.id).is_some() {
            touch_device(state, local.id).await.ok();
            return Ok(local);
        }
        eprintln!(
            "Saved local device #{} no longer exists; choose or register a device.",
            local.id
        );
    }

    let hostname = local_hostname();
    let devices = list_devices(state);
    let selected = match choose_device(&devices, hostname.as_deref())? {
        DeviceSelection::Existing(device) => device,
        DeviceSelection::Register { name, hostname } => {
            register_device(state, name.clone(), hostname.clone()).await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            list_devices(state)
                .into_iter()
                .filter(|d| d.name == name && d.hostname == hostname)
                .max_by_key(|d| d.id)
                .context("registered device but could not find it in my_devices")?
        }
    };

    touch_device(state, selected.id).await.ok();
    let local = LocalDevice {
        id: selected.id,
        name: selected.name,
        hostname: selected.hostname,
        selected_at: chrono::Utc::now(),
    };
    local.save(&config.device_file())?;
    println!("✓ using device #{} ({})", local.id, local.name);
    Ok(local)
}

enum DeviceSelection {
    Existing(DeviceCandidate),
    Register {
        name: String,
        hostname: Option<String>,
    },
}

fn choose_device(devices: &[DeviceCandidate], hostname: Option<&str>) -> Result<DeviceSelection> {
    let default_name = hostname.unwrap_or("this-device").to_string();
    let matches: Vec<_> = hostname
        .map(|host| {
            devices
                .iter()
                .filter(|d| d.hostname.as_deref() == Some(host) || d.name == host)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if matches.len() == 1 {
        let device = matches.into_iter().next().expect("one match");
        println!(
            "✓ matched local hostname to device #{} ({})",
            device.id, device.name
        );
        return Ok(DeviceSelection::Existing(device));
    }

    if devices.is_empty() {
        println!("No devices are registered yet; registering this machine as {default_name}.");
        return Ok(DeviceSelection::Register {
            name: default_name,
            hostname: hostname.map(str::to_owned),
        });
    }

    println!("Select which SpaceNix device this machine is:");
    for (idx, device) in devices.iter().enumerate() {
        println!(
            "  {}. #{} {} host={}",
            idx + 1,
            device.id,
            device.name,
            device.hostname.as_deref().unwrap_or("-")
        );
    }
    println!("  n. Register this machine as {default_name}");
    print!("Device [n]: ");
    std::io::stdout().flush().ok();

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading device selection")?;
    let answer = answer.trim();
    if answer.is_empty() || answer.eq_ignore_ascii_case("n") {
        return Ok(DeviceSelection::Register {
            name: default_name,
            hostname: hostname.map(str::to_owned),
        });
    }
    let index: usize = answer
        .parse()
        .context("device selection must be a number or n")?;
    let Some(device) = devices.get(index.saturating_sub(1)).cloned() else {
        anyhow::bail!("device selection out of range");
    };
    Ok(DeviceSelection::Existing(device))
}

fn list_devices(state: &ConnState) -> Vec<DeviceCandidate> {
    let mut devices: Vec<_> = state
        .conn
        .db()
        .my_devices()
        .iter()
        .map(|d| DeviceCandidate {
            id: d.id,
            name: d.name.clone(),
            hostname: d.hostname.clone(),
        })
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    devices
}

fn find_device(state: &ConnState, id: u64) -> Option<DeviceCandidate> {
    state
        .conn
        .db()
        .my_devices()
        .iter()
        .find(|d| d.id == id)
        .map(|d| DeviceCandidate {
            id: d.id,
            name: d.name.clone(),
            hostname: d.hostname.clone(),
        })
}

async fn register_device(state: &ConnState, name: String, hostname: Option<String>) -> Result<()> {
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .register_device_then(name, hostname, move |_ctx, res| {
            let _ = tx.send(res);
        })
        .context("invoking register_device")?;
    wait_unit("register_device", rx).await
}

async fn touch_device(state: &ConnState, id: u64) -> Result<()> {
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .touch_device_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        })
        .context("invoking touch_device")?;
    wait_unit("touch_device", rx).await
}

async fn wait_unit(
    name: &str,
    rx: oneshot::Receiver<Result<Result<(), String>, impl std::fmt::Debug>>,
) -> Result<()> {
    match rx
        .await
        .with_context(|| format!("{name} callback dropped"))?
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => anyhow::bail!("{name} rejected: {err}"),
        Err(err) => anyhow::bail!("{name} failed: {err:?}"),
    }
}

fn local_hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}
