//! Resolve which registered SpaceNix device represents this machine.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
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
        .subscribe([
            "SELECT * FROM my_devices",
            "SELECT * FROM my_ssh_keys",
            "SELECT * FROM my_ssh_endpoints",
        ]);
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
    let mut just_registered = false;
    let selected = match choose_device(&devices, hostname.as_deref())? {
        DeviceSelection::Existing(device) => device,
        DeviceSelection::Register { name, hostname } => {
            register_device(state, name.clone(), hostname.clone()).await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            let device = list_devices(state)
                .into_iter()
                .filter(|d| d.name == name && d.hostname == hostname)
                .max_by_key(|d| d.id)
                .context("registered device but could not find it in my_devices")?;
            just_registered = true;
            device
        }
    };

    if just_registered {
        maybe_create_ssh_endpoint(state, &selected).await.ok();
    }

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

fn local_username() -> Option<String> {
    for var in ["USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .with_context(|| format!("reading {prompt}"))?;
    Ok(answer.trim().to_string())
}

async fn maybe_create_ssh_endpoint(state: &ConnState, device: &DeviceCandidate) -> Result<()> {
    let mut keys: Vec<_> = state
        .conn
        .db()
        .my_ssh_keys()
        .iter()
        .map(|k| (k.id, k.name.clone(), k.public_key.clone()))
        .collect();
    keys.sort_by(|a, b| a.1.cmp(&b.1));

    println!();
    println!("Add this device as an SSH endpoint?");
    if keys.is_empty() {
        println!("  (no SSH keys are stored yet — skipping)");
        return Ok(());
    }
    for (idx, (id, name, _)) in keys.iter().enumerate() {
        println!("  {}. #{} {}", idx + 1, id, name);
    }
    println!("  s. skip");
    let answer = prompt_line("Key [s]: ")?;
    if answer.is_empty() || answer.eq_ignore_ascii_case("s") {
        return Ok(());
    }
    let index: usize = answer
        .parse()
        .context("ssh key selection must be a number or s")?;
    let Some((key_id, _key_name, public_key)) = keys.get(index.saturating_sub(1)) else {
        anyhow::bail!("ssh key selection out of range");
    };

    let default_host = device
        .hostname
        .clone()
        .or_else(local_hostname)
        .unwrap_or_else(|| device.name.clone());
    let host = {
        let s = prompt_line(&format!("Host [{default_host}]: "))?;
        if s.is_empty() { default_host.clone() } else { s }
    };
    let port_str = prompt_line("Port [22]: ")?;
    let port: u16 = if port_str.is_empty() {
        22
    } else {
        port_str
            .parse()
            .with_context(|| format!("invalid port: {port_str}"))?
    };
    let default_user = local_username().unwrap_or_else(|| "root".to_string());
    let username = {
        let s = prompt_line(&format!("Username [{default_user}]: "))?;
        if s.is_empty() { default_user } else { s }
    };
    let default_name = format!("{}:{}", username, host);
    let name = {
        let s = prompt_line(&format!("Endpoint name [{default_name}]: "))?;
        if s.is_empty() { default_name } else { s }
    };

    create_ssh_endpoint(state, name, host, port, username, *key_id, device.id).await?;
    println!("✓ ssh endpoint created");

    maybe_install_authorized_key(public_key)?;
    Ok(())
}

fn maybe_install_authorized_key(public_key: &str) -> Result<()> {
    let path = default_authorized_keys_path()
        .context("locating ~/.ssh/authorized_keys")?;
    println!();
    let answer = prompt_line(&format!(
        "Add the selected key's public key to {} on this device? [y/N]: ",
        path.display()
    ))?;
    if !answer.eq_ignore_ascii_case("y") {
        return Ok(());
    }
    append_authorized_key(&path, public_key)
        .with_context(|| format!("appending to {}", path.display()))?;
    println!("✓ added public key to {}", path.display());
    Ok(())
}

fn default_authorized_keys_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir())?;
    Some(home.join(".ssh").join("authorized_keys"))
}

fn append_authorized_key(path: &Path, public_key: &str) -> Result<()> {
    let trimmed = public_key.trim();
    if trimmed.is_empty() {
        anyhow::bail!("public key is empty");
    }

    if let Some(existing) = std::fs::read_to_string(path).ok() {
        for line in existing.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == trimmed {
                println!("(public key already present, skipping)");
                return Ok(());
            }
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(parent) {
                let mut perms = meta.permissions();
                perms.set_mode(0o700);
                let _ = std::fs::set_permissions(parent, perms);
            }
        }
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("opening {} for append", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = file.metadata() {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = file.set_permissions(perms);
        }
    }
    writeln!(file, "{trimmed} spacenix")?;
    Ok(())
}

async fn create_ssh_endpoint(
    state: &ConnState,
    name: String,
    host: String,
    port: u16,
    username: String,
    key_id: u64,
    device_id: u64,
) -> Result<()> {
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .set_ssh_endpoint_then(
            name,
            host,
            port,
            username,
            key_id,
            vec![device_id.to_string()],
            Vec::new(),
            true,
            None,
            move |_ctx, res| {
                let _ = tx.send(res);
            },
        )
        .context("invoking set_ssh_endpoint")?;
    wait_unit("set_ssh_endpoint", rx).await
}
