//! Browser SSH relay.
//!
//! When `spacenix service start` is running on a device that the user
//! has nominated as their SSH relay, this module subscribes to
//! `my_ssh_relay_sessions` on SpacetimeDB and:
//!
//! 1. For each new `Pending` session targeted at this device, mints a
//!    per-session token and calls `attach_ssh_relay_session_token` so
//!    the browser can read it back.
//! 2. The browser opens a WebSocket at
//!    `ws://<relay>:<port>/ssh/sessions/<id>?token=<token>`. The
//!    service validates the session and token, opens a pty, spawns
//!    `ssh(1)`, and bridges bytes.
//!
//! The actual SSH traffic flows over the WebSocket between browser
//! and relay — SpacetimeDB is only used for coordination.

use std::ffi::OsString;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use futures::StreamExt;
use rand::RngCore;
use spacetimedb_sdk::Timestamp;
use spacetimedb_sdk::Table;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::auth::conn::ConnState;
use crate::bindings::*;
use crate::config::Config;
use crate::store::device::LocalDevice;

/// Spawn the relay background task. It runs until `cancel` fires or
/// the SpacetimeDB connection drops. The task is a no-op if
/// `local_device.id` is not the user's configured `ssh_relay_device`.
pub fn spawn(
    config: Arc<Config>,
    state: Arc<ConnState>,
    local_device: LocalDevice,
    cancel: oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(err) =
            run_relay(config, state, local_device, cancel).await
        {
            tracing::warn!(?err, "ssh relay loop exited with error");
        }
    })
}

async fn run_relay(
    config: Arc<Config>,
    state: Arc<ConnState>,
    local_device: LocalDevice,
    mut cancel: oneshot::Receiver<()>,
) -> Result<()> {
    // Subscribe to the relay-related tables. We watch all of the
    // user's sessions and filter client-side by relay_device_id.
    state
        .conn
        .subscription_builder()
        .on_applied(|_| tracing::debug!("relay subscription applied"))
        .on_error(|_ctx, err| tracing::error!(?err, "relay subscription error"))
        .subscribe([
            "SELECT * FROM my_ssh_relay_sessions",
            "SELECT * FROM my_ssh_relay_device",
            "SELECT * FROM my_ssh_endpoints",
            "SELECT * FROM my_ssh_keys",
        ]);
    // Give the SDK a moment to land the initial subscription.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_seen_session_id: u64 = state
        .conn
        .db()
        .my_ssh_relay_sessions()
        .iter()
        .map(|s| s.id)
        .max()
        .unwrap_or(0);

    loop {
        tokio::select! {
            _ = &mut cancel => {
                tracing::info!("ssh relay: cancel received, exiting");
                return Ok(());
            }
            _ = tick.tick() => {
                // Only act on sessions targeted at this device.
                let max_id = state
                    .conn
                    .db()
                    .my_ssh_relay_sessions()
                    .iter()
                    .filter(|s| s.relay_device_id == local_device.id)
                    .map(|s| s.id)
                    .max()
                    .unwrap_or(0);
                if max_id > last_seen_session_id {
                    // Re-evaluate every session we haven't seen
                    // yet. We have to be careful: between minting
                    // the token and the browser connecting, the
                    // session row exists with a token attached. We
                    // should not mint a second token (the reducer
                    // is idempotent on success but would waste
                    // effort). Only mint for sessions that are
                    // still `Pending` and have no token.
                    for s in state
                        .conn
                        .db()
                        .my_ssh_relay_sessions()
                        .iter()
                        .filter(|s| s.relay_device_id == local_device.id)
                        .filter(|s| s.id > last_seen_session_id)
                    {
                        if s.status == SshRelaySessionStatus::Pending
                            && s.auth_token.is_none()
                        {
                            mint_and_attach(&state, s.id, &config).await;
                        }
                    }
                    last_seen_session_id = max_id;
                }
            }
        }
    }
}

async fn mint_and_attach(state: &Arc<ConnState>, session_id: u64, _config: &Arc<Config>) {
    // 32 bytes of randomness, base64url-ish but keeping the
    // character set bounded to the validator's allowed alphabet
    // ([A-Za-z0-9_-]). We use hex (0-9 a-f) and pad to 64 chars
    // so the validator is happy.
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let (tx, rx) = oneshot::channel();
    let res = state
        .conn
        .reducers()
        .attach_ssh_relay_session_token_then(session_id, token, move |_ctx, r| {
            let _ = tx.send(r);
        });
    if let Err(err) = res {
        tracing::warn!(session_id, ?err, "ssh relay: attach token invocation failed");
        return;
    }
    match rx.await {
        Ok(Ok(Ok(()))) => {
            tracing::info!(session_id, "ssh relay: attached session token");
        }
        Ok(Ok(Err(err))) => {
            tracing::warn!(session_id, ?err, "ssh relay: attach token rejected");
        }
        Ok(Err(err)) => {
            tracing::warn!(session_id, ?err, "ssh relay: attach token internal error");
        }
        Err(_) => {
            tracing::warn!(session_id, "ssh relay: attach token callback dropped");
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP / WebSocket layer
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RelayState {
    pub config: Arc<Config>,
    pub conn: Arc<ConnState>,
}

pub fn router(state: RelayState) -> Router {
    Router::new()
        .route("/ssh/sessions/:id", get(ws_upgrade))
        .with_state(state)
}

#[derive(serde::Deserialize)]
struct WsQuery {
    token: String,
}

async fn ws_upgrade(
    Path(id): Path<u64>,
    Query(q): Query<WsQuery>,
    State(state): State<RelayState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // 1. Look up the session. Must be `Active` (i.e. has a token).
    let Some(session) = state
        .conn
        .conn
        .db()
        .my_ssh_relay_sessions()
        .iter()
        .find(|s| s.id == id)
    else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "session not found",
        )
            .into_response();
    };
    if session.status == SshRelaySessionStatus::Closed {
        return (
            axum::http::StatusCode::GONE,
            "session closed",
        )
            .into_response();
    }
    if session.expires_at.to_micros_since_unix_epoch()
        <= Timestamp::now().to_micros_since_unix_epoch()
    {
        return (
            axum::http::StatusCode::GONE,
            "session expired",
        )
            .into_response();
    }
    let Some(expected) = session.auth_token.as_ref() else {
        return (
            axum::http::StatusCode::CONFLICT,
            "session has no auth token yet",
        )
            .into_response();
    };
    // Constant-time compare to avoid timing oracles.
    if !constant_time_eq(expected.as_bytes(), q.token.as_bytes()) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            "bad token",
        )
            .into_response();
    }

    // 2. Upgrade and hand off to the bridge.
    ws.on_upgrade(move |socket| handle_socket(state, session, socket))
}

async fn handle_socket(state: RelayState, session: SshRelaySessionMetadata, socket: WebSocket) {
    let session_id = session.id;
    let conn_for_close = Arc::clone(&state.conn);
    if let Err(err) = run_bridge(state.config, state.conn, session, socket).await {
        tracing::warn!(?err, "ssh relay: bridge exited");
    }
    // Tell the database the session is done so a follow-up browser
    // connection doesn't get matched to a stale row.
    let (tx, rx) = oneshot::channel();
    let res = conn_for_close
        .conn
        .reducers()
        .close_ssh_relay_session_then(session_id, move |_ctx, r| {
            let _ = tx.send(r);
        });
    if let Err(err) = res {
        tracing::warn!(?err, "ssh relay: close reducer invocation failed");
        return;
    }
    let _ = rx.await;
}

async fn run_bridge(
    config: Arc<Config>,
    state: Arc<ConnState>,
    session: SshRelaySessionMetadata,
    socket: WebSocket,
) -> Result<()> {
    let session_id = session.id;
    // Look up the endpoint.
    let endpoint = state
        .conn
        .db()
        .my_ssh_endpoints()
        .iter()
        .find(|e| e.id == session.endpoint_id)
        .ok_or_else(|| anyhow::anyhow!("endpoint disappeared"))?
        .clone();
    let key = reveal_ssh_key(&state, endpoint.key_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("ssh key not visible to relay"))?;
    let key_path = write_private_key(&config, &endpoint.name, &key.private_key)?;
    let _cleanup = PrivateKeyGuard::new(key_path.clone());

    // Spawn ssh(1) in a pty.
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("openpty")?;
    // Wrap ssh in a shell that puts the pty into raw, no-echo mode
    // before exec'ing ssh. The default pty termios is cooked/canonical
    // with echo on, which makes backspace erase the previous line
    // character and echoes every keystroke back to the master — the
    // browser would see "ls" and a stream of garbage echo bytes
    // rather than the clean keypress stream we want. `stty raw -echo`
    // disables both: bytes pass through one-for-one in both
    // directions.
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg(format!(
        "stty raw -echo 2>/dev/null; exec ssh -i {key} -p {port} -l {user} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -tt {host}",
        key = key_path.display(),
        port = endpoint.port,
        user = shell_escape(&endpoint.username),
        host = shell_escape(&endpoint.host),
    ));
    let mut child = pair.slave.spawn_command(cmd).context("spawn ssh")?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("clone pty reader")?;
    let writer = pair.master.take_writer().context("take pty writer")?;

    // WebSocket is a Stream + Sink. We don't have a clean split, so
    // we share it through a Mutex: the read task locks it just
    // long enough to pull the next message; the pump locks it just
    // long enough to send.
    let ws = Arc::new(tokio::sync::Mutex::new(socket));

    // pty -> websocket
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Bytes>();
    let reader_handle = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx.send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    // websocket -> pty
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Bytes>();
    let writer_for_thread = Arc::new(tokio::sync::Mutex::new(writer));
    let writer_for_thread_clone = Arc::clone(&writer_for_thread);
    let writer_handle = tokio::task::spawn_blocking(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(err) => {
                tracing::warn!(?err, "ssh relay: writer runtime build failed");
                return;
            }
        };
        rt.block_on(async move {
            use std::io::Write;
            while let Some(chunk) = in_rx.recv().await {
                let mut w = writer_for_thread_clone.lock().await;
                if w.write_all(&chunk).is_err() {
                    break;
                }
            }
        });
    });

    let ws_for_read = Arc::clone(&ws);
    let mut read_task = tokio::spawn(async move {
        loop {
            let msg_result = {
                let mut s = ws_for_read.lock().await;
                s.next().await
            };
            match msg_result {
                Some(Ok(Message::Binary(data))) => {
                    if in_tx.send(Bytes::from(data)).is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    if in_tx.send(Bytes::from(text.into_bytes())).is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => continue,
                Some(Err(_)) => break,
            }
        }
    });

    // pump
    let mut exit_status: i32 = 0;
    let mut child_done = false;
    let mut writer_done = false;
    loop {
        tokio::select! {
            biased;
            // Forward pty output to the websocket.
            chunk = out_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        let send_result = {
                            let mut s = ws.lock().await;
                            s.send(Message::Binary(bytes.to_vec())).await
                        };
                        if send_result.is_err() {
                            break;
                        }
                    }
                    None => {
                        // reader hit EOF; ssh exited.
                        child_done = true;
                    }
                }
            }
            // Watch the ssh child.
            status = wait_child_tick(&mut child), if !child_done => {
                match status {
                    Ok(Some(code)) => {
                        exit_status = code;
                        child_done = true;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!(?err, "ssh relay: wait_child error");
                        child_done = true;
                    }
                }
            }
            // Forward WS close to the writer task.
            _ = &mut read_task, if !writer_done => {
                writer_done = true;
            }
        }
        if child_done && writer_done {
            break;
        }
    }

    // Cleanup
    let _ = child.kill();
    let _ = reader_handle.await;
    read_task.abort();
    let _ = read_task.await;
    let _ = writer_handle.await;
    drop(writer_for_thread);
    tracing::info!(session_id = session_id, exit_status, "ssh relay: session ended");
    Ok(())
}

/// Poll the child for exit, yielding back to the runtime between
/// checks so the rest of the bridge can keep running. Returns
/// `Ok(None)` while still running, `Ok(Some(code))` on exit.
async fn wait_child_tick(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
) -> Result<Option<i32>> {
    match child.try_wait() {
        Ok(Some(status)) => Ok(Some(status.exit_code() as i32)),
        Ok(None) => {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(None)
        }
        Err(err) => Err(err.into()),
    }
}

async fn reveal_ssh_key(
    state: &Arc<ConnState>,
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
    file.write_all(b"\n")?;
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
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Constant-time byte slice comparison. Avoids leaking the token
/// length or contents through a timing side-channel.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[allow(dead_code)]
fn _osstring_marker(_: OsString) {}

/// Single-quote a string for use as one shell `argv` element.
/// Escapes any embedded single quotes per POSIX.
fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
