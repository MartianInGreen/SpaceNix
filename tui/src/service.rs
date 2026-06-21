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
    /// Start the service. By default the service is forked into the
    /// background and `spacenix` returns once it is listening; pass
    /// `--foreground` to keep the process attached to the current TTY.
    Start {
        /// Port to bind (default: random).
        #[arg(long)]
        port: Option<u16>,

        /// Run in the foreground (do not detach). Useful for debugging and
        /// for running under supervisors like `systemd` or `tmux`.
        #[arg(long, short = 'f')]
        foreground: bool,
    },

    /// Print where the service lock is and whether the service is running.
    Status,

    /// Stop a running service.
    Stop,
}

pub fn run(config: Arc<Config>, cmd: ServiceCommand) -> Result<ExitCode> {
    match cmd {
        // `start` is normally handled by `main::run` so the fork can
        // happen before any runtime is built. This arm only fires
        // for `--foreground` (no fork) or the unusual case where
        // `daemonize` returned `NotSupported`. Either way we need a
        // runtime to drive the service. We can't build one inline
        // because we're already inside `block_on` of a multi-thread
        // runtime — so we hop to a fresh OS thread first.
        ServiceCommand::Start { port, foreground: _ } => {
            std::thread::Builder::new()
                .name("spacenix-svc".into())
                .spawn(move || -> Result<ExitCode> {
                    // The SpacetimeDB SDK needs a multi-thread runtime
                    // (it uses `block_in_place` internally). We hop to a
                    // fresh OS thread first so we can safely build one
                    // even when the caller is already inside a runtime
                    // context.
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .context("building tokio runtime")?;
                    rt.block_on(run_service(config, port))
                })
                .expect("spawning service thread")
                .join()
                .expect("service thread panicked")
        }
        ServiceCommand::Status => status(config),
        ServiceCommand::Stop => stop(config),
    }
}

pub async fn run_service(config: Arc<Config>, port: Option<u16>) -> Result<ExitCode> {
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

    let app = crate::http::router(Arc::clone(&config), state.clone());
    let server = axum::serve(listener, app);

    let (metrics_cancel_tx, metrics_cancel_rx) = tokio::sync::oneshot::channel();
    let metrics_handle = match state.as_ref() {
        Some(state) => match crate::store::device::LocalDevice::load(&config.device_file()) {
            Ok(Some(local)) => {
                let state = Arc::clone(state);
                let config = Arc::clone(&config);
                let local_clone = local.clone();
                Some(tokio::spawn(async move {
                    if let Err(err) =
                        crate::metrics::run_reporter(config, state, local_clone, metrics_cancel_rx).await
                    {
                        tracing::warn!(?err, "metrics reporter exited with error");
                    }
                }))
            }
            Ok(None) => {
                eprintln!(
                    "warning: no local device selected; metrics reporter disabled \
                     (run `spacenix` once interactively to pick a device)"
                );
                None
            }
            Err(err) => {
                eprintln!("warning: could not read device selection: {err:#}");
                None
            }
        },
        None => None,
    };

    let shutdown = async {
        if let Ok(()) = signal::ctrl_c().await {
            tracing::info!("ctrl-c received; shutting down service");
        }
    };
    tokio::select! {
        res = server => res.context("service server error")?,
        _ = shutdown => {}
    }

    if let Some(handle) = metrics_handle {
        let _ = metrics_cancel_tx.send(());
        let _ = handle.await;
    }

    let _ = std::fs::remove_file(&lock_path);
    Ok(ExitCode::from(0))
}

/// Outcome of [`daemonize`]. `Child` runs the service; `Parent` returns to
/// the shell after waiting for the child to write its lock file.
pub enum DaemonizeOutcome {
    Child,
    Parent(u32),
    NotSupported,
}

/// On Unix, double-fork the current process, create a new session, and
/// redirect stdio to a log file under the config dir. Returns whether the
/// current process should run the service or exit.
#[cfg(unix)]
pub fn daemonize(config: &Config) -> DaemonizeOutcome {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    // First fork. The parent waits for the child's lock file to appear so
    // the user gets a meaningful `pid:` line; the child proceeds to detach.
    let first = unsafe { libc::fork() };
    if first < 0 {
        eprintln!("warning: fork failed; running in foreground");
        return DaemonizeOutcome::NotSupported;
    }
    if first > 0 {
        // Parent: wait up to 5s for the service.lock to materialize.
        let lock_path = config.service_lock_file();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(lock)) = ServiceLock::load(&lock_path) {
                return DaemonizeOutcome::Parent(lock.pid);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // The child may have failed to start; we don't know the pid. Best
        // effort: report 0 and let the user check `service status`.
        eprintln!("warning: timed out waiting for service to start; check `spacenix service status`");
        return DaemonizeOutcome::Parent(0);
    }

    // First child: become a session leader so we can drop the controlling
    // TTY, then fork again. The grandchild is the actual service so it
    // can never reacquire a TTY (avoids SIGHUP-on-shell-exit surprises).
    if unsafe { libc::setsid() } < 0 {
        eprintln!("warning: setsid failed; continuing without full detach");
    }
    let second = unsafe { libc::fork() };
    if second < 0 {
        eprintln!("warning: second fork failed; running in foreground");
        return DaemonizeOutcome::NotSupported;
    }
    if second > 0 {
        // First child exits immediately; the grandchild is reparented to
        // init and is the actual service process.
        std::process::exit(0);
    }

    // Grandchild: redirect stdio to a log file under the config dir so the
    // user has something to inspect if startup fails.
    let log_path = config.config_dir.join("service.log");
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&log_path)
    {
        use std::os::unix::io::IntoRawFd;
        let raw = file.into_raw_fd();
        unsafe {
            for fd in 0..=2 {
                libc::dup2(raw, fd);
            }
            if raw > 2 {
                libc::close(raw);
            }
        }
    }

    DaemonizeOutcome::Child
}

#[cfg(not(unix))]
pub fn daemonize(_config: &Config) -> DaemonizeOutcome {
    DaemonizeOutcome::NotSupported
}

fn status(config: Arc<Config>) -> Result<ExitCode> {
    match ServiceLock::load(&config.service_lock_file())? {
        Some(lock) if pid_alive(lock.pid) => {
            println!("running · pid {} · port {}", lock.pid, lock.port);
            println!("started  {}", lock.started_at);
            Ok(ExitCode::from(0))
        }
        Some(lock) => {
            println!(
                "not running (stale lock: pid {} is gone, port {})",
                lock.pid, lock.port
            );
            Ok(ExitCode::from(1))
        }
        None => {
            println!("not running");
            Ok(ExitCode::from(1))
        }
    }
}

fn stop(config: Arc<Config>) -> Result<ExitCode> {
    match ServiceLock::load(&config.service_lock_file())? {
        Some(lock) if !pid_alive(lock.pid) => {
            // Lock is stale; clean it up so the next `start` doesn't
            // think the service is already running.
            let _ = std::fs::remove_file(&config.service_lock_file());
            println!(
                "removed stale lock (pid {} is no longer running)",
                lock.pid
            );
            Ok(ExitCode::from(0))
        }
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

/// `kill(pid, 0)` returns 0 if the process exists and we have permission to
/// signal it, -1 / ESRCH otherwise. This is the cheapest way to ask the
/// kernel "is this pid alive?" without scanning `/proc`.
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    // Safety: kill with signal 0 is a no-op; we just inspect the return
    // value. We don't dereference any pointer.
    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if !alive {
        // EPERM means the pid exists but we can't signal it. Treat as alive
        // so we don't accidentally clobber someone else's service.
        let errno = std::io::Error::last_os_error().raw_os_error();
        errno == Some(libc::EPERM)
    } else {
        true
    }
}

#[cfg(not(unix))]
pub fn pid_alive(_pid: u32) -> bool {
    true
}
