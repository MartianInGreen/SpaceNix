//! `spacenix ssh …` — manage SSH keys and endpoints.

use std::ffi::OsString;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use tokio::sync::oneshot;

use crate::auth::conn;
use crate::bindings::*;
use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum SshCommand {
    /// Manage SSH keys.
    #[command(subcommand)]
    Key(KeyCommand),

    /// Manage SSH endpoints.
    #[command(subcommand)]
    Endpoint(EndpointCommand),

    /// Open an interactive SSH session to a registered endpoint.
    ///
    /// The endpoint's stored private key is revealed on demand, written
    /// to a 0600 tempfile, and handed to the local `ssh(1)` binary. The
    /// tempfile is removed when the session ends. Any arguments after
    /// `--` (or all trailing args) are passed to the remote command.
    Connect(ConnectArgs),
}

#[derive(Debug, Args)]
pub struct ConnectArgs {
    /// Endpoint name or numeric id.
    pub endpoint: String,

    /// Extra arguments forwarded to `ssh(1)`. Use `--` to separate them
    /// from `spacenix` flags if you need to pass options that start
    /// with `-` (e.g. `spacenix ssh connect prod -- -L 8080:localhost:80`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub ssh_args: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    /// List SSH keys.
    List,
    /// Create an SSH key. Private key is read from --private-key or stdin.
    Create(KeyCreateArgs),
    /// Reveal an SSH key value.
    Reveal { id: u64 },
    /// Update public/private key material.
    SetValue(KeyValueArgs),
    /// Replace key device scope.
    Devices {
        id: u64,
        #[arg(long, value_delimiter = ',')]
        device: Vec<String>,
    },
    /// Replace key tags.
    Tags {
        id: u64,
        #[arg(long, value_delimiter = ',')]
        tag: Vec<String>,
    },
    /// Delete an SSH key.
    Delete { id: u64 },
}

#[derive(Debug, Args)]
pub struct KeyCreateArgs {
    pub name: String,
    #[arg(long)]
    pub public_key: String,
    #[arg(long)]
    pub private_key: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub device: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub tag: Vec<String>,
}

#[derive(Debug, Args)]
pub struct KeyValueArgs {
    pub id: u64,
    #[arg(long)]
    pub public_key: String,
    #[arg(long)]
    pub private_key: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum EndpointCommand {
    /// List SSH endpoints.
    List,
    /// Create an SSH endpoint.
    Create(EndpointCreateArgs),
    /// Update endpoint host/user/key fields.
    Update(EndpointUpdateArgs),
    /// Enable or disable an endpoint.
    Enabled { id: u64, enabled: bool },
    /// Replace endpoint device scope.
    Devices {
        id: u64,
        #[arg(long, value_delimiter = ',')]
        device: Vec<String>,
    },
    /// Replace endpoint tags.
    Tags {
        id: u64,
        #[arg(long, value_delimiter = ',')]
        tag: Vec<String>,
    },
    /// Delete an SSH endpoint.
    Delete { id: u64 },
}

#[derive(Debug, Args)]
pub struct EndpointCreateArgs {
    pub name: String,
    #[arg(long)]
    pub host: String,
    #[arg(long, default_value_t = 22)]
    pub port: u16,
    #[arg(long)]
    pub username: String,
    #[arg(long)]
    pub key_id: u64,
    #[arg(long, value_delimiter = ',')]
    pub device: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub tag: Vec<String>,
    #[arg(long, default_value_t = true)]
    pub enabled: bool,
}

#[derive(Debug, Args)]
pub struct EndpointUpdateArgs {
    pub id: u64,
    #[arg(long)]
    pub host: String,
    #[arg(long, default_value_t = 22)]
    pub port: u16,
    #[arg(long)]
    pub username: String,
    #[arg(long)]
    pub key_id: u64,
}

pub async fn run(config: Arc<Config>, cmd: SshCommand) -> Result<ExitCode> {
    match cmd {
        SshCommand::Key(cmd) => {
            let state = connect(&config).await?;
            run_key(&state, cmd).await
        }
        SshCommand::Endpoint(cmd) => {
            let state = connect(&config).await?;
            run_endpoint(&state, cmd).await
        }
        SshCommand::Connect(args) => {
            // Connect uses its own subscription set (we don't need
            // devices for the basic case) and runs a synchronous
            // process, so it doesn't share `state` with the others.
            run_connect(&config, args).await
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
        .on_error(|_ctx, err| tracing::error!(?err, "ssh subscription error"))
        .subscribe([
            "SELECT * FROM my_ssh_keys",
            "SELECT * FROM my_ssh_endpoints",
            "SELECT * FROM my_devices",
        ]);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(state)
}

async fn run_key(state: &conn::ConnState, cmd: KeyCommand) -> Result<ExitCode> {
    match cmd {
        KeyCommand::List => {
            let mut rows: Vec<_> = state.conn.db().my_ssh_keys().iter().collect();
            rows.sort_by(|a, b| a.name.cmp(&b.name));
            if rows.is_empty() {
                println!("(no ssh keys)");
            } else {
                for k in rows {
                    println!(
                        "#{:<6}\t{}\tfp={}\tdevices={}\ttags={}",
                        k.id,
                        k.name,
                        k.fingerprint,
                        list_or(&k.device_ids, "all"),
                        list_or(&k.tags, "-")
                    );
                }
            }
            Ok(ExitCode::from(0))
        }
        KeyCommand::Create(args) => {
            let private_key = args.private_key.unwrap_or_else(read_stdin);
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_key_then(
                    args.name.clone(),
                    args.public_key,
                    private_key,
                    args.device,
                    args.tag,
                    move |_ctx, res| {
                        let _ = tx.send(res);
                    },
                )
                .context("invoking set_ssh_key")?;
            wait_unit("set_ssh_key", rx).await?;
            println!("✓ ssh key {} saved", args.name);
            Ok(ExitCode::from(0))
        }
        KeyCommand::Reveal { id } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .procedures()
                .reveal_ssh_key_then(id, move |_ctx, res| {
                    let _ = tx.send(res);
                });
            let value = match rx.await.context("reveal_ssh_key callback dropped")? {
                Ok(Ok(Some(value))) => value,
                Ok(Ok(None)) => anyhow::bail!("ssh key #{id} not visible to this account"),
                Ok(Err(err)) => anyhow::bail!("reveal_ssh_key rejected: {err}"),
                Err(err) => anyhow::bail!("reveal_ssh_key failed: {err}"),
            };
            println!("public_key:\n{}", value.public_key);
            println!("private_key:\n{}", value.private_key);
            Ok(ExitCode::from(0))
        }
        KeyCommand::SetValue(args) => {
            let private_key = args.private_key.unwrap_or_else(read_stdin);
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_key_value_then(args.id, args.public_key, private_key, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking set_ssh_key_value")?;
            wait_unit("set_ssh_key_value", rx).await?;
            println!("✓ ssh key #{} value updated", args.id);
            Ok(ExitCode::from(0))
        }
        KeyCommand::Devices { id, device } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_key_devices_then(id, device, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking set_ssh_key_devices")?;
            wait_unit("set_ssh_key_devices", rx).await?;
            println!("✓ ssh key #{id} devices updated");
            Ok(ExitCode::from(0))
        }
        KeyCommand::Tags { id, tag } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_key_tags_then(id, tag, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking set_ssh_key_tags")?;
            wait_unit("set_ssh_key_tags", rx).await?;
            println!("✓ ssh key #{id} tags updated");
            Ok(ExitCode::from(0))
        }
        KeyCommand::Delete { id } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .delete_ssh_key_then(id, move |_ctx, res| {
                    let _ = tx.send(res);
                })?;
            wait_unit("delete_ssh_key", rx).await?;
            println!("✓ ssh key #{id} deleted");
            Ok(ExitCode::from(0))
        }
    }
}

async fn run_endpoint(state: &conn::ConnState, cmd: EndpointCommand) -> Result<ExitCode> {
    match cmd {
        EndpointCommand::List => {
            let mut rows: Vec<_> = state.conn.db().my_ssh_endpoints().iter().collect();
            rows.sort_by(|a, b| a.name.cmp(&b.name));
            if rows.is_empty() {
                println!("(no ssh endpoints)");
            } else {
                for e in rows {
                    let status = if e.enabled { "enabled" } else { "disabled" };
                    println!(
                        "#{:<6}\t{}\t{}@{}:{}\tkey=#{}\t{}\tdevices={}\ttags={}",
                        e.id,
                        e.name,
                        e.username,
                        e.host,
                        e.port,
                        e.key_id,
                        status,
                        list_or(&e.device_ids, "all"),
                        list_or(&e.tags, "-")
                    );
                }
            }
            Ok(ExitCode::from(0))
        }
        EndpointCommand::Create(args) => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_endpoint_then(
                    args.name.clone(),
                    args.host,
                    args.port,
                    args.username,
                    args.key_id,
                    args.device,
                    args.tag,
                    args.enabled,
                    None,
                    move |_ctx, res| {
                        let _ = tx.send(res);
                    },
                )
                .context("invoking set_ssh_endpoint")?;
            wait_unit("set_ssh_endpoint", rx).await?;
            println!("✓ ssh endpoint {} saved", args.name);
            Ok(ExitCode::from(0))
        }
        EndpointCommand::Update(args) => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .update_ssh_endpoint_then(
                    args.id,
                    args.host,
                    args.port,
                    args.username,
                    args.key_id,
                    move |_ctx, res| {
                        let _ = tx.send(res);
                    },
                )
                .context("invoking update_ssh_endpoint")?;
            wait_unit("update_ssh_endpoint", rx).await?;
            println!("✓ ssh endpoint #{} updated", args.id);
            Ok(ExitCode::from(0))
        }
        EndpointCommand::Enabled { id, enabled } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_endpoint_enabled_then(id, enabled, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking set_ssh_endpoint_enabled")?;
            wait_unit("set_ssh_endpoint_enabled", rx).await?;
            println!("✓ ssh endpoint #{id} enabled={enabled}");
            Ok(ExitCode::from(0))
        }
        EndpointCommand::Devices { id, device } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_endpoint_devices_then(id, device, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking set_ssh_endpoint_devices")?;
            wait_unit("set_ssh_endpoint_devices", rx).await?;
            println!("✓ ssh endpoint #{id} devices updated");
            Ok(ExitCode::from(0))
        }
        EndpointCommand::Tags { id, tag } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .set_ssh_endpoint_tags_then(id, tag, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking set_ssh_endpoint_tags")?;
            wait_unit("set_ssh_endpoint_tags", rx).await?;
            println!("✓ ssh endpoint #{id} tags updated");
            Ok(ExitCode::from(0))
        }
        EndpointCommand::Delete { id } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .delete_ssh_endpoint_then(id, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking delete_ssh_endpoint")?;
            wait_unit("delete_ssh_endpoint", rx).await?;
            println!("✓ ssh endpoint #{id} deleted");
            Ok(ExitCode::from(0))
        }
    }
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

fn list_or(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        empty.to_string()
    } else {
        items.join(",")
    }
}

fn read_stdin() -> String {
    use std::io::Read;
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

async fn run_connect(config: &Config, args: ConnectArgs) -> Result<ExitCode> {
    let state = connect(config).await?;
    let ep = resolve_endpoint(&state, &args.endpoint)
        .with_context(|| format!("resolving endpoint {:?}", args.endpoint))?;
    if !ep.enabled {
        anyhow::bail!(
            "endpoint {:?} is disabled — re-enable it with `spacenix ssh endpoint enabled {} true`",
            ep.name,
            ep.id
        );
    }

    let key = reveal_key(&state, ep.key_id)
        .await
        .with_context(|| format!("revealing ssh key #{} for endpoint {:?}", ep.key_id, ep.name))?
        .ok_or_else(|| {
            anyhow::anyhow!("ssh key #{} not visible to this account", ep.key_id)
        })?;

    let key_path = write_private_key(config, &ep.name, &key.private_key)
        .context("materializing private key")?;
    let _cleanup = PrivateKeyGuard::new(key_path.clone());

    let exit = invoke_ssh(&ep, &key_path, &args.ssh_args)
        .await
        .context("running ssh(1)")?;
    Ok(exit)
}

fn resolve_endpoint(state: &conn::ConnState, target: &str) -> Result<SshEndpointMetadata> {
    let rows: Vec<SshEndpointMetadata> = state.conn.db().my_ssh_endpoints().iter().collect();
    if let Ok(id) = target.parse::<u64>() {
        if let Some(ep) = rows.into_iter().find(|e| e.id == id) {
            return Ok(ep);
        }
        anyhow::bail!("no ssh endpoint with id {id}");
    }
    let mut matches = rows.into_iter().filter(|e| e.name == target);
    let Some(ep) = matches.next() else {
        anyhow::bail!("no ssh endpoint named {target:?}");
    };
    if matches.next().is_some() {
        anyhow::bail!(
            "multiple ssh endpoints named {target:?}; pass the numeric id to disambiguate"
        );
    }
    Ok(ep)
}

async fn reveal_key(
    state: &conn::ConnState,
    id: u64,
) -> Result<Option<module_bindings::SshKeyValue>> {
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .procedures()
        .reveal_ssh_key_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        });
    match rx.await.context("reveal_ssh_key callback dropped")? {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => anyhow::bail!("reveal_ssh_key rejected: {err}"),
        Err(err) => anyhow::bail!("reveal_ssh_key failed: {err:?}"),
    }
}

fn write_private_key(config: &Config, endpoint_name: &str, private_key: &str) -> Result<PathBuf> {
    use std::io::Write;
    let dir = config.config_dir.join("ssh-keys");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    // Uniquify so concurrent invocations don't stomp each other. The
    // name is opaque — only spacenix and the spawned ssh ever touch it.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let safe_name: String = endpoint_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = dir.join(format!("{safe_name}-{pid}-{nanos}.key"));

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("creating private key tempfile {}", path.display()))?;
    file.write_all(private_key.as_bytes())
        .with_context(|| format!("writing private key to {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("writing trailing newline to {}", path.display()))?;
    file.sync_all().ok();
    Ok(path)
}

struct PrivateKeyGuard {
    path: Option<PathBuf>,
}

impl PrivateKeyGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

impl Drop for PrivateKeyGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            // Best-effort: the key only ever existed for this run.
            let _ = std::fs::remove_file(&path);
        }
    }
}

async fn invoke_ssh(ep: &SshEndpointMetadata, key_path: &Path, extra: &[String]) -> Result<ExitCode> {
    // `-o IdentitiesOnly=yes` keeps the agent / other keys from
    // shadowing the one we just fetched. `-o StrictHostKeyChecking=accept-new`
    // is the standard TOFU behaviour: trust the host key on first
    // use, refuse if it ever changes. We deliberately leave the
    // user's ssh_config alone for everything else (ProxyJump, etc).
    let mut args: Vec<OsString> = Vec::with_capacity(10 + extra.len());
    args.push(OsString::from("-i"));
    args.push(key_path.as_os_str().to_owned());
    args.push(OsString::from("-p"));
    args.push(OsString::from(ep.port.to_string()));
    args.push(OsString::from("-l"));
    args.push(OsString::from(ep.username.as_str()));
    args.push(OsString::from("-o"));
    args.push(OsString::from("IdentitiesOnly=yes"));
    args.push(OsString::from("-o"));
    args.push(OsString::from("StrictHostKeyChecking=accept-new"));
    args.push(OsString::from(ep.host.as_str()));
    for a in extra {
        args.push(OsString::from(a));
    }

    eprintln!(
        "→ ssh {}@{}:{} (key #{})",
        ep.username, ep.host, ep.port, ep.key_id
    );

    use std::process::Stdio;
    let status = tokio::process::Command::new("ssh")
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| {
            format!(
                "failed to spawn `ssh` — is OpenSSH installed and on PATH? argv={args:?}"
            )
        })?;

    Ok(match status.code() {
        Some(code) if (0..256).contains(&code) => ExitCode::from(code as u8),
        _ => ExitCode::from(1),
    })
}
