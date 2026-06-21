//! Owns the SpacetimeDB `DbConnection` for the lifetime of the TUI / CLI /
//! service process. The connection runs on its own thread (via
//! `run_threaded`) and all interaction happens through the standard SDK API.

use std::sync::Arc;

use anyhow::{Context, Result};
use spacetimedb_sdk::{DbContext, Identity};
use tokio::sync::watch;

use module_bindings::DbConnection;

use crate::config::Config;

/// A handle to the running STDB connection plus the channels used by the
/// TUI / service to observe its lifecycle.
#[derive(Clone)]
#[allow(dead_code)]
pub struct ConnState {
    pub conn: Arc<DbConnection>,
    /// Last-known connection status pushed by the SDK callbacks.
    pub status_rx: watch::Receiver<Status>,
    pub identity_rx: watch::Receiver<Option<Identity>>,
    /// Cached connection token (refreshed on every successful connect).
    pub token_rx: watch::Receiver<Option<String>>,
}

impl std::fmt::Debug for ConnState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnState")
            .field("conn", &"Arc<DbConnection>")
            .field("status", &*self.status_rx.borrow())
            .field("identity_set", &self.identity_rx.borrow().is_some())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

impl ConnState {
    pub fn identity(&self) -> Option<Identity> {
        *self.identity_rx.borrow()
    }

    #[allow(dead_code)]
    pub fn status(&self) -> Status {
        *self.status_rx.borrow()
    }

    #[allow(dead_code)]
    pub fn latest_token(&self) -> Option<String> {
        self.token_rx.borrow().clone()
    }
}

/// Connect to STDB using a previously-saved token (anonymous or post-signin).
///
/// `token` is the JWT-style auth token from SpacetimeDB. If `None`, the host
/// issues a fresh anonymous identity.
pub fn connect(config: &Config, token: Option<String>) -> Result<ConnState> {
    let (status_tx, status_rx) = watch::channel(Status::Connecting);
    let (identity_tx, identity_rx) = watch::channel(None);
    let (token_tx, token_rx) = watch::channel(token.clone());

    let conn = DbConnection::builder()
        .with_uri(&config.stdb_uri)
        .with_database_name(&config.stdb_module)
        .with_token(token)
        .on_connect({
            let status_tx = status_tx.clone();
            let identity_tx = identity_tx.clone();
            let token_tx = token_tx.clone();
            move |_ctx, identity, token| {
                let _ = status_tx.send(Status::Connected);
                let _ = identity_tx.send(Some(identity));
                let _ = token_tx.send(Some(token.to_owned()));
            }
        })
        .on_connect_error({
            let status_tx = status_tx.clone();
            move |_ctx, err| {
                tracing::warn!(?err, "spacetimedb connect error");
                let _ = status_tx.send(Status::Error);
            }
        })
        .on_disconnect({
            let status_tx = status_tx.clone();
            move |_ctx, _err| {
                let _ = status_tx.send(Status::Disconnected);
            }
        })
        .build()
        .context("opening SpacetimeDB connection")?;

    let conn = Arc::new(conn);

    // Background thread that advances the connection's message loop.
    //
    // The SDK ships a `run_threaded` helper but it `panic!`s on any
    // non-normal-disconnect error (e.g. a 5xx from the host during the
    // initial handshake), which would take the whole TUI process down
    // even though the user might be able to recover by retrying. We run
    // our own `frame_tick` loop and swallow non-fatal errors so the TUI
    // can report the failure via `status_rx` and the caller can decide
    // what to do.
    let conn_for_thread = Arc::clone(&conn);
    let _handle = std::thread::Builder::new()
        .name("spacenix-stdb".to_owned())
        .spawn(move || {
            loop {
                // The two outcomes we treat as terminal:
                //   1. The connection has been disconnected (we asked for it).
                //   2. `frame_tick` reports `is_active() == false`, which means
                //      the SDK has given up.
                if !conn_for_thread.is_active() {
                    return;
                }
                if let Err(err) = conn_for_thread.frame_tick() {
                    tracing::warn!(?err, "spacenix-stdb tick error");
                    // Brief pause so we don't spin if the error is sticky.
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        })
        .context("spawning STDB message thread")?;

    Ok(ConnState {
        conn,
        status_rx,
        identity_rx,
        token_rx,
    })
}
