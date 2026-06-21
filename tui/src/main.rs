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
mod service;
mod store;
mod tui;
mod util;

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
            Command::Service(cmd) => service::run(config, cmd).await,
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
