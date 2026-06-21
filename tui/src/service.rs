//! `spacenix service …` — run the background HTTP + sync worker.

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Subcommand;
use tokio::signal;

use crate::auth::conn;
use crate::config::Config;
use crate::store::credentials::Credentials;
use crate::store::service_lock::ServiceLock;

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Start the service in the foreground.
    Start {
        /// Port to bind (default: random).
        #[arg(long)]
        port: Option<u16>,
    },

    /// Print where the service lock is and whether the service is running.
    Status,

    /// Stop a running service.
    Stop,
}

pub async fn run(config: Arc<Config>, cmd: ServiceCommand) -> Result<ExitCode> {
    match cmd {
        ServiceCommand::Start { port } => start(config, port).await,
        ServiceCommand::Status => status(config),
        ServiceCommand::Stop => stop(config),
    }
}

async fn start(config: Arc<Config>, port: Option<u16>) -> Result<ExitCode> {
    if let Some(existing) = ServiceLock::load(&config.service_lock_file())? {
        eprintln!(
            "service is already running (pid {}, port {})",
            existing.pid, existing.port
        );
        return Ok(ExitCode::from(0));
    }
    let lock_path = config.service_lock_file();
    let bind = port.unwrap_or(0);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", bind))
        .await
        .context("binding service listener")?;
    let bound_port = listener.local_addr()?.port();
    let pid = std::process::id();

    let lock = ServiceLock {
        pid,
        port: bound_port,
        started_at: chrono::Utc::now(),
    };
    lock.save(&lock_path).context("writing service lock")?;

    println!("spacenix service listening on http://127.0.0.1:{bound_port}");
    println!("pid: {pid}");

    // Best-effort STDB connection. The service can run in a degraded state if
    // the user hasn't logged in yet.
    let state = match Credentials::load(&config.credentials_file()) {
        Ok(Some(creds)) => match conn::connect(&config, Some(creds.token)) {
            Ok(s) => Some(Arc::new(s)),
            Err(err) => {
                eprintln!("warning: could not connect to SpacetimeDB: {err:#}");
                None
            }
        },
        Ok(None) => {
            eprintln!("warning: not logged in; run `spacenix login` first");
            None
        }
        Err(err) => {
            eprintln!("warning: could not read credentials: {err:#}");
            None
        }
    };

    let app = crate::http::router(state);
    let server = axum::serve(listener, app);
    let shutdown = async {
        if let Ok(()) = signal::ctrl_c().await {
            tracing::info!("ctrl-c received; shutting down service");
        }
    };
    tokio::select! {
        res = server => res.context("service server error")?,
        _ = shutdown => {}
    }

    let _ = std::fs::remove_file(&lock_path);
    Ok(ExitCode::from(0))
}

fn status(config: Arc<Config>) -> Result<ExitCode> {
    match ServiceLock::load(&config.service_lock_file())? {
        Some(lock) => {
            println!("running · pid {} · port {}", lock.pid, lock.port);
            println!("started  {}", lock.started_at);
            Ok(ExitCode::from(0))
        }
        None => {
            println!("not running");
            Ok(ExitCode::from(1))
        }
    }
}

fn stop(config: Arc<Config>) -> Result<ExitCode> {
    match ServiceLock::load(&config.service_lock_file())? {
        Some(lock) => {
            #[cfg(unix)]
            {
                use std::process::Command;
                Command::new("kill")
                    .arg("-TERM")
                    .arg(lock.pid.to_string())
                    .status()
                    .ok();
            }
            println!("sent SIGTERM to pid {}", lock.pid);
            Ok(ExitCode::from(0))
        }
        None => {
            println!("not running");
            Ok(ExitCode::from(0))
        }
    }
}
