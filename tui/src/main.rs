use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

mod auth;
mod bindings;
mod cli;
mod cmd;
mod config;
mod http;
mod metrics;
mod relay;
mod service;
mod store;
mod tui;
mod util;

use crate::store::service_lock::ServiceLock;

#[derive(Debug, Parser)]
#[command(
    name = "spacenix",
    version,
    about = "SpaceNix TUI / CLI / service — sync files and manage secrets.",
    long_about = None
)]
struct Cli {
    /// Path to a config directory (defaults to $XDG_CONFIG_HOME/spacenix or
    /// ~/.config/spacenix).
    #[arg(long, global = true, env = "SPACENIX_CONFIG_DIR")]
    config_dir: Option<PathBuf>,

    /// SpacetimeDB server URI.
    #[arg(long, global = true, env = "SPACENIX_STDB_URI")]
    stdb_uri: Option<String>,

    /// SpacetimeDB module / database name.
    #[arg(long, global = true, env = "SPACENIX_STDB_MODULE")]
    stdb_module: Option<String>,

    /// Override verbosity (RUST_LOG style filter).
    #[arg(long, global = true, env = "SPACENIX_LOG")]
    log: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Launch the interactive TUI (default).
    #[command(visible_alias = "ui")]
    Tui(tui::TuiArgs),

    /// First-run login: open the browser or paste a token.
    Login(cmd::login::LoginArgs),

    /// Sign out and forget local credentials.
    Logout,

    /// Show current sign-in / connection state.
    Whoami,

    /// Manage secrets.
    #[command(subcommand)]
    Secret(cli::secret::SecretCommand),

    /// Manage files and folders metadata.
    #[command(subcommand)]
    File(cli::file::FileCommand),

    /// Manage SSH keys and endpoints.
    #[command(subcommand)]
    Ssh(cli::ssh::SshCommand),

    /// Manage registered devices.
    #[command(subcommand)]
    Device(cli::device::DeviceCommand),

    /// Show and update account settings.
    #[command(subcommand)]
    Account(cli::account::AccountCommand),

    /// Manage personal access tokens.
    #[command(subcommand)]
    Token(cli::token::TokenCommand),

    /// Manage which files / folders are synced to this device.
    #[command(subcommand)]
    Sync(cli::sync::SyncCommand),

    /// Run the background service (HTTP API + sync worker).
    #[command(subcommand)]
    Service(service::ServiceCommand),

    /// Print the resolved configuration.
    Config,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();

    init_tracing(cli.log.as_deref());

    // Resolve the config dir early — every subcommand needs it.
    let config = config::Config::resolve(
        cli.config_dir.as_deref(),
        cli.stdb_uri.as_deref(),
        cli.stdb_module.as_deref(),
    )?;
    let config = Arc::new(config);

    // For `service start` we have to fork *before* building the tokio
    // runtime. Building a new runtime from a forked process is unsafe
    // (the parent's multi-thread runtime holds worker threads that
    // don't exist in the child, and tokio explicitly panics on nested
    // runtimes). So: if this is a non-foreground start, do the fork
    // here on a thread that has never touched a runtime, and only the
    // child falls through to build its own runtime.
    if let Command::Service(service::ServiceCommand::Start { port, foreground, bind }) = &cli.command {
        let port = *port;
        let foreground = *foreground;
        let bind = bind.clone();

        // Already-running check. A stale lock (the recorded pid is no
        // longer alive) is treated as "not running" so a crash loop
        // doesn't permanently wedge the service.
        if let Some(existing) = ServiceLock::load(&config.service_lock_file())? {
            if service::pid_alive(existing.pid) {
                eprintln!(
                    "service is already running (pid {}, port {})",
                    existing.pid, existing.port
                );
                return Ok(ExitCode::from(0));
            }
            eprintln!(
                "removing stale service lock (pid {} is no longer running)",
                existing.pid
            );
            let _ = std::fs::remove_file(&config.service_lock_file());
        }

        if !foreground {
            // Run the fork on a fresh OS thread so we're not on a
            // tokio worker thread. We don't have a runtime yet, but
            // being explicit costs nothing.
            return Ok(std::thread::Builder::new()
                .name("spacenix-svc-fork".into())
                .spawn(move || -> Result<ExitCode> {
                    match service::daemonize(&config) {
                        service::DaemonizeOutcome::Parent(child_pid) => {
                            let port = ServiceLock::load(&config.service_lock_file())
                                .ok()
                                .flatten()
                                .map(|l| l.port.to_string())
                                .unwrap_or_else(|| "?".to_string());
                            println!("spacenix service listening on http://{bind}:{port}");
                            println!("pid: {child_pid}");
                            println!("log:  {}/service.log", config.config_dir.display());
                            println!("stop: `spacenix service stop`");
                            Ok(ExitCode::from(0))
                        }
                        service::DaemonizeOutcome::NotSupported => {
                            eprintln!(
                                "warning: detaching is not supported on this platform; \
                                 falling back to foreground mode"
                            );
                            let rt = tokio::runtime::Builder::new_multi_thread()
                                .enable_all()
                                .build()
                                .context("building tokio runtime")?;
                            rt.block_on(service::run_service(config, port, Some(bind)))
                        }
                        service::DaemonizeOutcome::Child => {
                            // Grandchild. Build a fresh multi-threaded
                            // runtime in this process — the SpacetimeDB
                            // SDK requires `block_in_place`, which is
                            // only available on a multi-thread runtime.
                            // It's safe to build one here because no
                            // runtime was alive at fork time.
                            let rt = tokio::runtime::Builder::new_multi_thread()
                                .enable_all()
                                .build()
                                .context("building tokio runtime")?;
                            rt.block_on(service::run_service(config, port, Some(bind)))
                        }
                    }
                })
                .expect("spawning service fork thread")
                .join()
                .expect("service fork thread panicked")?);
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;

    let result = rt.block_on(async move {
        match cli.command {
            Command::Tui(args) => tui::run(config, args).await,
            Command::Login(args) => cmd::login::run(config, args).await,
            Command::Logout => cmd::logout::run(config).await,
            Command::Whoami => cmd::whoami::run(config).await,
            Command::Secret(cmd) => cli::secret::run(config, cmd).await,
            Command::File(cmd) => cli::file::run(config, cmd).await,
            Command::Ssh(cmd) => cli::ssh::run(config, cmd).await,
            Command::Device(cmd) => cli::device::run(config, cmd).await,
            Command::Account(cmd) => cli::account::run(config, cmd).await,
            Command::Token(cmd) => cli::token::run(config, cmd).await,
            Command::Sync(cmd) => cli::sync::run(config, cmd).await,
            Command::Service(cmd) => service::run(config, cmd),
            Command::Config => {
                println!("{:#?}", *config);
                Ok(ExitCode::from(0))
            }
        }
    });

    drop(rt);
    info!("exit ok");
    result
}

fn init_tracing(filter: Option<&str>) {
    let default = filter.unwrap_or("info,spacetimedb_sdk=warn,spacenix=info");
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .try_init();
}
