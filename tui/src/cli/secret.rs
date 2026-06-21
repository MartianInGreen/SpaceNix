//! `spacenix secret …` subcommands.
//!
//! Secret *values* are only revealed via the `reveal_secret` procedure, which
//! requires the current connection to be signed in. Pulling via the CLI
//! therefore requires a prior `spacenix login`.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use tokio::sync::oneshot;

use crate::bindings::*;

use crate::auth::conn;
use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum SecretCommand {
    /// List secret names visible to you.
    List,

    /// Print a secret value to stdout (or `--export KEY=VAL` lines).
    Get {
        /// Env name, e.g. DATABASE_URL.
        env: String,
        /// Emit `export FOO=bar` lines instead of just the value.
        #[arg(long)]
        export: bool,
    },

    /// Create or update a secret. Reads the value from `--value` or stdin.
    Set {
        env: String,
        /// Secret value. If omitted, read from stdin.
        #[arg(long)]
        value: Option<String>,
        /// Comma-separated device ids to scope to (default: all).
        #[arg(long, value_delimiter = ',')]
        device: Vec<String>,
        /// Comma-separated permission grants to scope to (default: all).
        #[arg(long, value_delimiter = ',')]
        permission: Vec<String>,
    },

    /// Delete a secret.
    Delete { env: String },
}

pub async fn run(config: Arc<Config>, cmd: SecretCommand) -> Result<ExitCode> {
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?
        .context("not signed in — run `spacenix login` first")?;
    let state = conn::connect(&config, Some(creds.token))?;

    state
        .conn
        .subscription_builder()
        .on_applied(|_| tracing::debug!("secrets subscription applied"))
        .on_error(|_ctx, err| tracing::error!(?err, "secrets subscription error"))
        .subscribe(["SELECT * FROM my_secrets"]);

    // Give the SDK a moment to land the subscription. The connection's
    // message thread runs on its own; the first query results typically
    // arrive within a few hundred ms.
    tokio::time::sleep(Duration::from_millis(400)).await;

    match cmd {
        SecretCommand::List => cmd_list(&state).await,
        SecretCommand::Get { env, export } => cmd_get(&state, &env, export).await,
        SecretCommand::Set {
            env,
            value,
            device,
            permission,
        } => cmd_set(&state, env, value, device, permission).await,
        SecretCommand::Delete { env } => cmd_delete(&state, &env).await,
    }
}

async fn cmd_list(state: &conn::ConnState) -> Result<ExitCode> {
    let mut rows: Vec<_> = state.conn.db().my_secrets().iter().collect();
    rows.sort_by(|a, b| a.env.cmp(&b.env));
    if rows.is_empty() {
        println!("(no secrets)");
    } else {
        for r in rows {
            let devices = if r.device_ids.is_empty() {
                "all".to_string()
            } else {
                r.device_ids.join(",")
            };
            let perms = if r.permissions.is_empty() {
                "*".to_string()
            } else {
                r.permissions.join(",")
            };
            println!("{}\tdevices={}\tperms={}", r.env, devices, perms);
        }
    }
    Ok(ExitCode::from(0))
}

async fn cmd_get(state: &conn::ConnState, env: &str, export: bool) -> Result<ExitCode> {
    let id = state
        .conn
        .db()
        .my_secrets()
        .iter()
        .find(|s| s.env == env)
        .map(|s| s.id)
        .context(format!("no secret named {env}"))?;

    let (tx, rx) = oneshot::channel();
    state
        .conn
        .procedures()
        .reveal_secret_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        });
    let res = rx.await.context("reveal_secret callback dropped")?;

    let value = match res {
        Ok(Ok(Some(v))) => v,
        Ok(Ok(None)) => bail!("secret {env} not visible to this account"),
        Ok(Err(err)) => bail!("reveal_secret rejected: {err}"),
        Err(err) => bail!("reveal_secret failed: {err}"),
    };
    let value = value.value;
    if export {
        println!("export {env}={}", shell_quote(&value));
    } else {
        println!("{value}");
    }
    Ok(ExitCode::from(0))
}

async fn cmd_set(
    state: &conn::ConnState,
    env: String,
    value: Option<String>,
    devices: Vec<String>,
    permissions: Vec<String>,
) -> Result<ExitCode> {
    let value = match value {
        Some(v) => v,
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    if value.is_empty() {
        bail!("secret value is empty");
    }
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .set_secret_then(
            env.clone(),
            value,
            devices,
            permissions,
            move |_ctx, res| {
                let _ = tx.send(res);
            },
        )
        .context("invoking set_secret")?;
    let res = rx.await.context("set_secret callback dropped")?;
    match res {
        Ok(Ok(())) => {
            println!("✓ {env} saved");
            Ok(ExitCode::from(0))
        }
        Ok(Err(err)) => {
            bail!("set_secret rejected: {err}");
        }
        Err(err) => {
            bail!("set_secret failed: {err}");
        }
    }
}

async fn cmd_delete(state: &conn::ConnState, env: &str) -> Result<ExitCode> {
    let id = state
        .conn
        .db()
        .my_secrets()
        .iter()
        .find(|s| s.env == env)
        .map(|s| s.id)
        .context(format!("no secret named {env}"))?;
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .delete_secret_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        })
        .context("invoking delete_secret")?;
    let res = rx.await.context("delete_secret callback dropped")?;
    match res {
        Ok(Ok(())) => {
            println!("✓ {env} deleted");
            Ok(ExitCode::from(0))
        }
        Ok(Err(err)) => bail!("delete_secret rejected: {err}"),
        Err(err) => bail!("delete_secret failed: {err}"),
    }
}

fn shell_quote(s: &str) -> String {
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
