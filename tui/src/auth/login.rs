//! First-run login flow.
//!
//! Strategy:
//! 1. Bind a tiny axum server to `127.0.0.1:0` (random port).
//! 2. Open the user's web browser to the SpaceNix web app with a
//!    `?callback=http://127.0.0.1:<port>/oauth/callback` query string. The
//!    web app (when wired up) will redirect the browser to that local URL
//!    once the user finishes signing in, carrying the SpacetimeDB connection
//!    token in the query string.
//! 3. Wait for the callback. If it never arrives, fall back to asking the
//!    user to paste a PAT in the TUI (and persist it to credentials.toml).
//!
//! The PAT paste path also covers first-time use on a headless box where the
//! browser may not be available.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Json;
use axum::Router;
use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use serde::Deserialize;
use tokio::sync::oneshot;

use crate::auth::conn::{self, ConnState};
use crate::config::Config;
use crate::store::credentials::Credentials;

/// Outcome of a successful login.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LoginOutcome {
    pub credentials: Credentials,
    pub conn: ConnState,
}

#[allow(dead_code)]
pub struct PendingCallback {
    pub port: u16,
    pub url: String,
    rx: oneshot::Receiver<CallbackPayload>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CallbackPayload {
    /// Connection token, the OIDC-style JWT issued by SpacetimeDB.
    pub token: String,
    /// Identity hex, optional. If absent we read it from the connection.
    #[serde(default)]
    pub identity: Option<String>,
}

/// Start the local callback server. Returns the URL the user should be sent
/// to. The caller is responsible for opening the browser and (asynchronously)
/// awaiting `wait` for the callback payload.
pub async fn start_callback_server() -> Result<PendingCallback> {
    let (tx, rx) = oneshot::channel();
    // The handler can be called multiple times (axum's Handler is `Fn`), but
    // we only care about the first call. We protect `tx` with a Mutex so
    // subsequent requests become no-ops.
    let tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let app = Router::new()
        .route(
            "/oauth/callback",
            get(move |Query(payload): Query<CallbackPayload>| {
                let tx = std::sync::Arc::clone(&tx);
                async move {
                    let body = format!(
                        "<!doctype html><html lang=\"en\"><head>\
                        <meta charset=\"utf-8\">\
                        <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
                        <title>SpaceNix · signed in</title>\
                        <style>\
                        :root{{color-scheme:light dark}}\
                        html,body{{margin:0;padding:0;background:#fafafa;color:#1a1a1a;\
                        font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;\
                        line-height:1.5}}\
                        @media (prefers-color-scheme:dark){{\
                          html,body{{background:#0f0f10;color:#e4e4e7}}\
                        }}\
                        main{{max-width:36rem;margin:0 auto;padding:3rem 1.5rem}}\
                        .badge{{display:inline-flex;align-items:center;gap:.5rem;\
                        background:#dcfce7;color:#166534;border-radius:9999px;\
                        padding:.35rem .85rem;font-size:.85rem;font-weight:600;\
                        letter-spacing:.02em}}\
                        @media (prefers-color-scheme:dark){{\
                          .badge{{background:#052e16;color:#86efac}}\
                        }}\
                        h1{{font-size:1.75rem;font-weight:600;margin:1.5rem 0 .75rem;\
                        letter-spacing:-.01em}}\
                        p{{margin:0 0 1rem;font-size:1rem;color:#3f3f46}}\
                        @media (prefers-color-scheme:dark){{p{{color:#a1a1aa}}}}\
                        .muted{{color:#71717a;font-size:.9rem}}\
                        @media (prefers-color-scheme:dark){{.muted{{color:#a1a1aa}}}}\
                        details{{margin-top:1.5rem;border:1px solid #e4e4e7;\
                        border-radius:.5rem;padding:0;background:#fff}}\
                        @media (prefers-color-scheme:dark){{\
                          details{{background:#18181b;border-color:#27272a}}\
                        }}\
                        summary{{padding:.75rem 1rem;cursor:pointer;\
                        font-size:.9rem;font-weight:500;user-select:none;\
                        list-style:none}}\
                        summary::-webkit-details-marker{{display:none}}\
                        pre{{margin:0;padding:0 1rem 1rem;font-size:.75rem;\
                        font-family:ui-monospace,SFMono-Regular,Menlo,monospace;\
                        word-break:break-all;white-space:pre-wrap;color:#52525b}}\
                        @media (prefers-color-scheme:dark){{\
                          pre{{color:#a1a1aa}}\
                        }}\
                        .close{{margin-top:1.5rem;font-size:.85rem;color:#71717a}}\
                        </style>\
                        </head><body><main>\
                        <span class=\"badge\">✓ Signed in</span>\
                        <h1>You can close this tab.</h1>\
                        <p>The connection token has been sent back to the <code>spacenix</code> \
                        TUI on your local machine. Return to the terminal to continue.</p>\
                        <p class=\"muted\">If for some reason the TUI did not pick the token up, \
                        you can copy it from the box below.</p>\
                        <details>\
                        <summary>Show token</summary>\
                        <pre>{}</pre>\
                        </details>\
                        <p class=\"close\">This tab will close automatically in a few seconds.</p>\
                        <script>setTimeout(function(){{window.close()}},16000)</script>\
                        </main></body></html>",
                        html_escape(&payload.token)
                    );
                    let mut guard = tx.lock().await;
                    if let Some(sender) = guard.take() {
                        let _ = sender.send(payload);
                    }
                    Html(body)
                }
            }),
        )
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({ "ok": true })) }),
        );

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("binding local callback listener")?;
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}/oauth/callback");
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!(?err, "local callback server stopped");
        }
    });

    Ok(PendingCallback { port, url, rx })
}

impl PendingCallback {
    pub async fn wait(self, timeout: Duration) -> Result<CallbackPayload> {
        match tokio::time::timeout(timeout, self.rx).await {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(_)) => anyhow::bail!("callback channel dropped before delivery"),
            Err(_) => anyhow::bail!(
                "timed out waiting for browser to redirect back to the TUI. \
                 Paste a personal access token instead with `spacenix login --token …`."
            ),
        }
    }
}

/// Complete a login with a token + optional identity. Persists the
/// credentials and (best-effort) opens a live connection.
///
/// If the connection check fails (e.g. the TUI is pointed at the wrong
/// STDB server / module name), the token is still persisted so the user
/// can fix the config without re-running the browser flow.
pub fn complete_login(
    config: Arc<Config>,
    token: String,
    identity: Option<String>,
) -> Result<LoginOutcome> {
    match conn::connect(&config, Some(token.clone())) {
        Ok(conn) => {
            let Some(identity_hex) = identity.or_else(|| wait_for_identity(&conn).ok()) else {
                anyhow::bail!(
                    "connected to SpacetimeDB but the server did not report an \
                     identity. Try again once the module is published."
                );
            };
            let credentials = Credentials {
                identity: identity_hex,
                token: token.clone(),
                email: None,
                saved_at: chrono::Utc::now(),
            };
            credentials
                .save(&config.credentials_file())
                .context("persisting credentials")?;
            Ok(LoginOutcome { credentials, conn })
        }
        Err(connect_err) => {
            tracing::warn!(
                ?connect_err,
                "could not connect to SpacetimeDB during login; persisting token anyway"
            );
            let identity = identity.ok_or_else(|| {
                anyhow::anyhow!(
                    "could not connect to SpacetimeDB and no identity was supplied by \
                     the callback. The server replied:\n  {connect_err:#}\n\n\
                     The token was NOT saved. Check --stdb-uri / --stdb-module, then \
                     try again."
                )
            })?;
            let credentials = Credentials {
                identity,
                token: token.clone(),
                email: None,
                saved_at: chrono::Utc::now(),
            };
            credentials
                .save(&config.credentials_file())
                .context("persisting credentials")?;
            // Re-raise the connection error as a warning so the caller can
            // surface it, but the credentials are now on disk.
            Err(connect_err).context(
                "credentials saved, but the TUI could not open a live connection. \
                 Fix the SpacetimeDB URL / module name and re-run.",
            )
        }
    }
}

pub fn build_web_login_url(config: &Config, callback: &str) -> String {
    let origin = std::env::var("SPACENIX_WEB_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:5173".to_string());
    let module = &config.stdb_module;
    let uri = &config.stdb_uri;
    format!(
        "{origin}/login?callback={}&module={}&uri={}",
        urlencoding(callback),
        urlencoding(module),
        urlencoding(uri)
    )
}

fn wait_for_identity(conn: &ConnState) -> Result<String> {
    // Block on the watch channel for up to a few seconds.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(id) = conn.identity() {
            return Ok(id.to_hex().to_string());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "connected to SpacetimeDB but the identity was not yet \
                 available. Try again."
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
    out
}

fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + n - 10) as char,
        _ => unreachable!(),
    }
}
