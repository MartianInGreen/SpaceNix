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
    // We use `advance_one_message_blocking` (the same call the SDK's
    // built-in `run_threaded` makes) rather than `frame_tick`, which
    // drains only the *currently pending* messages and returns
    // immediately when the queue is empty. Wrapping `frame_tick` in a
    // `loop {}` is what was previously pinning one CPU core — the
    // blocking variant sleeps on the WebSocket channel and only wakes
    // when a real message arrives.
    //
    // The SDK's `run_threaded` helper panics on any non-normal
    // disconnect error, so we hand-roll this loop, log the error, and
    // back off briefly so a transient host hiccup doesn't take the TUI
    // process down. The caller observes the failure via `status_rx`.
    let conn_for_thread = Arc::clone(&conn);
    let _handle = std::thread::Builder::new()
        .name("spacenix-stdb".to_owned())
        .spawn(move || {
            loop {
                if !conn_for_thread.is_active() {
                    return;
                }
                match conn_for_thread.advance_one_message_blocking() {
                    Ok(()) => {}
                    Err(err) => {
                        tracing::warn!(?err, "spacenix-stdb advance error");
                        // Back off so a sticky error doesn't burn CPU
                        // (which is what the previous `frame_tick`-in-
                        // a-loop design was doing).
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
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
