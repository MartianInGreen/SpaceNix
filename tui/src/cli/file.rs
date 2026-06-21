//! `spacenix file …` — manage file and folder metadata.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use tokio::sync::oneshot;

use crate::auth::conn;
use crate::bindings::*;
use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum FileCommand {
    /// List files and folders visible to you.
    List,

    /// Create a folder in the remote file tree.
    Mkdir {
        name: String,
        /// Full tree path for the folder. Defaults to name at root.
        #[arg(long)]
        tree_path: Option<String>,
        /// Optional local path hint.
        #[arg(long)]
        local_path: Option<String>,
    },

    /// Rename or move a file/folder metadata record.
    Rename {
        id: u64,
        name: String,
        #[arg(long)]
        tree_path: Option<String>,
        #[arg(long)]
        local_path: Option<String>,
    },

    /// Delete a file/folder metadata record.
    Delete { id: u64 },
}

pub async fn run(config: Arc<Config>, cmd: FileCommand) -> Result<ExitCode> {
    let state = connect(&config).await?;
    match cmd {
        FileCommand::List => list(&state).await,
        FileCommand::Mkdir {
            name,
            tree_path,
            local_path,
        } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .create_folder_then(name.clone(), tree_path, local_path, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking create_folder")?;
            wait_unit("create_folder", rx).await?;
            println!("✓ folder {name} created");
            Ok(ExitCode::from(0))
        }
        FileCommand::Rename {
            id,
            name,
            tree_path,
            local_path,
        } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .rename_file_then(id, name, tree_path, local_path, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking rename_file")?;
            wait_unit("rename_file", rx).await?;
            println!("✓ file #{id} updated");
            Ok(ExitCode::from(0))
        }
        FileCommand::Delete { id } => {
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .delete_file_then(id, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking delete_file")?;
            wait_unit("delete_file", rx).await?;
            println!("✓ file #{id} deleted");
            Ok(ExitCode::from(0))
        }
    }
}

async fn connect(config: &Config) -> Result<conn::ConnState> {
    let creds = crate::store::credentials::Credentials::load(&config.credentials_file())?
        .context("not signed in — run `spacenix login` first")?;
    let state = conn::connect(config, Some(creds.token))?;
    state
        .conn
        .subscription_builder()
        .on_error(|_ctx, err| tracing::error!(?err, "files subscription error"))
        .subscribe(["SELECT * FROM my_files"]);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(state)
}

async fn list(state: &conn::ConnState) -> Result<ExitCode> {
    let mut rows: Vec<_> = state.conn.db().my_files().iter().collect();
    rows.sort_by(|a, b| {
        a.tree_path
            .as_deref()
            .unwrap_or(&a.name)
            .cmp(b.tree_path.as_deref().unwrap_or(&b.name))
    });
    if rows.is_empty() {
        println!("(no files)");
    } else {
        for r in rows {
            let kind = if r.is_directory { "d" } else { "f" };
            let path = r.tree_path.as_deref().unwrap_or("(root)");
            let content_type = r.content_type.as_deref().unwrap_or("-");
            println!(
                "[{kind}] #{:<6}\t{}\t{} bytes\t{}\t{}",
                r.id, r.name, r.size_bytes, content_type, path
            );
        }
    }
    Ok(ExitCode::from(0))
}

async fn wait_unit(
    name: &str,
    rx: oneshot::Receiver<Result<Result<(), String>, impl std::fmt::Debug>>,
) -> Result<()> {
    match rx
        .await
        .with_context(|| format!("{name} callback dropped"))?
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => anyhow::bail!("{name} rejected: {err}"),
        Err(err) => anyhow::bail!("{name} failed: {err:?}"),
    }
}
