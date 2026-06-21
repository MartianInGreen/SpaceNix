//! `spacenix login` — first-run setup.
//!
//! Two paths:
//! - **Browser flow** (default): spin up the local callback server, open the
//!   web app's login page with `?callback=…`, wait for the redirect.
//! - **Token paste** (`--token <pat>` or `--token-stdin`): skip the browser
//!   and use a previously-issued PAT.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;

use crate::auth::login;
use crate::config::Config;

#[derive(Debug, Args, Default)]
pub struct LoginArgs {
    /// Skip the browser, use this token directly. Useful for headless boxes.
    #[arg(long)]
    pub token: Option<String>,

    /// Read the token from stdin instead of an arg.
    #[arg(long, conflicts_with = "token")]
    pub token_stdin: bool,

    /// Maximum time to wait for the browser to redirect back.
    #[arg(long, default_value_t = 120)]
    pub browser_timeout_secs: u64,
}

pub async fn run(config: Arc<Config>, args: LoginArgs) -> Result<ExitCode> {
    if let Some(token) = args.token.or_else(|| {
        if args.token_stdin {
            let mut buf = String::new();
            use std::io::Read;
            std::io::stdin().read_to_string(&mut buf).ok()?;
            let trimmed = buf.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        } else {
            None
        }
    }) {
        let outcome = login::complete_login(Arc::clone(&config), token, None)
            .context("logging in with token")?;
        crate::auth::device::ensure_local_device(Arc::clone(&config), &outcome.conn)
            .await
            .context("selecting local device")?;
        println!("✓ logged in as {}", outcome.credentials.identity);
        return Ok(ExitCode::from(0));
    }

    let pending = login::start_callback_server()
        .await
        .context("starting local callback server")?;
    let web_url = login::build_web_login_url(&config, &pending.url);
    println!("Opening browser to: {web_url}");
    if let Err(err) = open::that_detached(&web_url) {
        eprintln!("could not open browser: {err}");
        eprintln!("Open this URL manually: {web_url}");
    }
    println!("(if the browser did not open, paste a token with --token)");

    let payload = pending
        .wait(Duration::from_secs(args.browser_timeout_secs))
        .await
        .context("waiting for browser callback")?;

    let outcome = login::complete_login(Arc::clone(&config), payload.token, payload.identity)
        .context("completing login")?;
    crate::auth::device::ensure_local_device(Arc::clone(&config), &outcome.conn)
        .await
        .context("selecting local device")?;
    println!("✓ logged in as {}", outcome.credentials.identity);
    Ok(ExitCode::from(0))
}
