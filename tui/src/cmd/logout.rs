//! `spacenix logout` — drop local credentials.

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;

use crate::config::Config;

pub async fn run(config: Arc<Config>) -> Result<ExitCode> {
    let path = config.credentials_file();
    match std::fs::remove_file(&path) {
        Ok(()) => {
            println!("✓ signed out (removed {})", path.display());
            Ok(ExitCode::from(0))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("(not signed in — nothing to do)");
            Ok(ExitCode::from(0))
        }
        Err(err) => Err(err.into()),
    }
}
