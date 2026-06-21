//! `spacenix sync …` — manage which files / folders this device keeps in
//! sync. (The actual file materialization is performed by the background
//! service; the CLI just edits the local selection.)

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Subcommand;

use crate::bindings::*;
use crate::config::Config;
use crate::store::sync::{SelectedFile, SyncSelection};

#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// List available files / folders.
    Ls,

    /// Add a file or folder to the local sync selection.
    Add { id: u64 },

    /// Remove from the local sync selection.
    Remove { id: u64 },

    /// Show the current selection.
    Status,
}

pub async fn run(config: Arc<Config>, cmd: SyncCommand) -> Result<ExitCode> {
    match cmd {
        SyncCommand::Ls => ls(config).await,
        SyncCommand::Add { id } => add(config, id).await,
        SyncCommand::Remove { id } => remove(config, id).await,
        SyncCommand::Status => status(config).await,
    }
}

async fn ls(config: Arc<Config>) -> Result<ExitCode> {
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?;
    let state = crate::auth::conn::connect(&config, creds.map(|c| c.token))?;
    state
        .conn
        .subscription_builder()
        .on_applied(|_| tracing::debug!("files subscription applied"))
        .on_error(|_ctx, err| tracing::error!(?err, "files subscription error"))
        .subscribe(["SELECT * FROM my_files"]);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let sel = SyncSelection::load(&config.sync_file())?;
    let mut rows: Vec<_> = state.conn.db().my_files().iter().collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    if rows.is_empty() {
        println!("(no files)");
    } else {
        for r in rows {
            let mark = if sel.contains(r.id) { "✓" } else { " " };
            let kind = if r.is_directory { "d" } else { "f" };
            let path = r.tree_path.as_deref().unwrap_or("(root)");
            println!("{mark} [{kind}] #{:<6} {}\t{}", r.id, r.name, path);
        }
    }
    Ok(ExitCode::from(0))
}

async fn add(config: Arc<Config>, id: u64) -> Result<ExitCode> {
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?;
    let state = crate::auth::conn::connect(&config, creds.map(|c| c.token))?;
    state
        .conn
        .subscription_builder()
        .on_applied(|_| {})
        .on_error(|_ctx, err| tracing::error!(?err, "files subscription error"))
        .subscribe(["SELECT * FROM my_files"]);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut sel = SyncSelection::load(&config.sync_file())?;
    let row = state
        .conn
        .db()
        .my_files()
        .iter()
        .find(|f| f.id == id)
        .ok_or_else(|| anyhow::anyhow!("no file with id {id}"))?;
    let file = SelectedFile {
        id: row.id,
        path: row.tree_path.clone(),
        name: row.name.clone(),
        is_directory: row.is_directory,
        local_path: None,
        added_at: chrono::Utc::now(),
    };
    let was_new = !sel.contains(file.id);
    sel.toggle(&file);
    sel.save(&config.sync_file())?;
    if was_new {
        println!("✓ added {}", file.name);
    } else {
        println!("(was already in selection) {}", file.name);
    }
    Ok(ExitCode::from(0))
}

async fn remove(config: Arc<Config>, id: u64) -> Result<ExitCode> {
    let mut sel = SyncSelection::load(&config.sync_file())?;
    if sel.selected.remove(&id).is_some() {
        sel.save(&config.sync_file())?;
        println!("✓ removed #{id} from sync selection");
    } else {
        println!("(not in selection)");
    }
    Ok(ExitCode::from(0))
}

async fn status(config: Arc<Config>) -> Result<ExitCode> {
    let sel = SyncSelection::load(&config.sync_file())?;
    if sel.selected.is_empty() {
        println!("(empty selection)");
    } else {
        for (id, f) in &sel.selected {
            let kind = if f.is_directory { "d" } else { "f" };
            let path = f.path.as_deref().unwrap_or("(root)");
            println!("[{kind}] #{id} {}\t{}", f.name, path);
        }
    }
    Ok(ExitCode::from(0))
}
