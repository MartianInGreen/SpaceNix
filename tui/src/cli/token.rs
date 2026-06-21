//! `spacenix token …` — manage personal access tokens.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use tokio::sync::oneshot;

use crate::bindings::*;

use crate::auth::conn;
use crate::auth::pat::{PatRecord, PatStore};
use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// List tokens visible to you.
    List,

    /// Issue a new token. The token is printed to stdout and also appended to
    /// `pats.toml` so the TUI can present it on demand.
    Create {
        /// Friendly name, e.g. `laptop-cli`.
        name: String,
        /// Comma-separated permission grants.
        #[arg(long, value_delimiter = ',')]
        permission: Vec<String>,
    },

    /// Revoke a token by id.
    Revoke { id: u64 },
}

pub async fn run(config: Arc<Config>, cmd: TokenCommand) -> Result<ExitCode> {
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?
        .context("not signed in — run `spacenix login` first")?;
    let state = conn::connect(&config, Some(creds.token))?;

    state
        .conn
        .subscription_builder()
        .on_applied(|_| tracing::debug!("api keys subscription applied"))
        .on_error(|_ctx, err| tracing::error!(?err, "api keys subscription error"))
        .subscribe(["SELECT * FROM my_api_keys"]);
    tokio::time::sleep(Duration::from_millis(400)).await;

    match cmd {
        TokenCommand::List => cmd_list(&state).await,
        TokenCommand::Create { name, permission } => {
            cmd_create(&config, &state, name, permission).await
        }
        TokenCommand::Revoke { id } => cmd_revoke(&state, id).await,
    }
}

async fn cmd_list(state: &conn::ConnState) -> Result<ExitCode> {
    let mut rows: Vec<_> = state.conn.db().my_api_keys().iter().collect();
    rows.sort_by_key(|a| a.id);
    if rows.is_empty() {
        println!("(no tokens)");
    } else {
        for r in rows {
            let status = if r.revoked_at.is_some() { "revoked" } else { "active" };
            println!("{}\t{}\t{}\t{:?}", r.id, status, r.name, r.permissions);
        }
    }
    Ok(ExitCode::from(0))
}

async fn cmd_create(
    config: &Config,
    state: &conn::ConnState,
    name: String,
    permissions: Vec<String>,
) -> Result<ExitCode> {
    if permissions.is_empty() {
        anyhow::bail!("at least one permission is required (e.g. --permission 'files:read,secrets:read')");
    }
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .procedures()
        .create_api_key_then(name.clone(), permissions.clone(), move |_ctx, res| {
            let _ = tx.send(res);
        });
    let res = rx.await.context("create_api_key callback dropped")?;
    let created = match res {
        Ok(Ok(c)) => c,
        Ok(Err(err)) => anyhow::bail!("create_api_key rejected: {err}"),
        Err(err) => anyhow::bail!("create_api_key failed: {err}"),
    };
    println!("token: {}", created.token);
    let store_path = config.config_dir.join("pats.toml");
    let mut store = PatStore::load(&store_path).unwrap_or_default();
    store.tokens.push(PatRecord {
        id: created.metadata.id,
        name: created.metadata.name.clone(),
        token: created.token.clone(),
        created_at: chrono::Utc::now(),
    });
    store.save(&store_path)?;
    Ok(ExitCode::from(0))
}

async fn cmd_revoke(state: &conn::ConnState, id: u64) -> Result<ExitCode> {
    let (tx, rx) = oneshot::channel();
    state
        .conn
        .reducers()
        .revoke_api_key_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        })
        .context("invoking revoke_api_key")?;
    let res = rx.await.context("revoke_api_key callback dropped")?;
    match res {
        Ok(Ok(())) => {
            println!("✓ token #{id} revoked");
            Ok(ExitCode::from(0))
        }
        Ok(Err(err)) => anyhow::bail!("revoke_api_key rejected: {err}"),
        Err(err) => anyhow::bail!("revoke_api_key failed: {err}"),
    }
}
