//! `spacenix account …` — show and update account settings.

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
pub enum AccountCommand {
    /// Show account profile details.
    Show,

    /// Update the sign-in email address.
    UpdateEmail {
        new_email: String,
        /// Current password. If omitted, read it from stdin.
        #[arg(long)]
        current_password: Option<String>,
    },
}

pub async fn run(config: Arc<Config>, cmd: AccountCommand) -> Result<ExitCode> {
    let state = connect(&config).await?;
    match cmd {
        AccountCommand::Show => show(&state).await,
        AccountCommand::UpdateEmail {
            new_email,
            current_password,
        } => {
            let current_password = current_password.unwrap_or_else(read_stdin_trimmed);
            if current_password.is_empty() {
                anyhow::bail!("current password is required");
            }
            let (tx, rx) = oneshot::channel();
            state
                .conn
                .reducers()
                .update_email_then(new_email, current_password, move |_ctx, res| {
                    let _ = tx.send(res);
                })
                .context("invoking update_email")?;
            match rx.await.context("update_email callback dropped")? {
                Ok(Ok(())) => {
                    println!("✓ email updated");
                    Ok(ExitCode::from(0))
                }
                Ok(Err(err)) => anyhow::bail!("update_email rejected: {err}"),
                Err(err) => anyhow::bail!("update_email failed: {err}"),
            }
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
        .on_error(|_ctx, err| tracing::error!(?err, "user subscription error"))
        .subscribe(["SELECT * FROM my_user"]);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(state)
}

async fn show(state: &conn::ConnState) -> Result<ExitCode> {
    let user = state.conn.db().my_user().iter().next();
    let identity = state
        .identity()
        .map(|i| i.to_hex().to_string())
        .unwrap_or_else(|| "-".to_string());
    if let Some(user) = user {
        println!(
            "display_name\t{}",
            user.display_name.as_deref().unwrap_or("-")
        );
        println!("email\t{}", user.email);
        println!("role\t{}", user.role);
        println!("identity\t{}", identity);
    } else {
        println!("identity\t{}", identity);
        println!("(profile not available)");
    }
    Ok(ExitCode::from(0))
}

fn read_stdin_trimmed() -> String {
    use std::io::Read;
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf.trim().to_string()
}
