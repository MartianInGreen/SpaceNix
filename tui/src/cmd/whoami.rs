//! `spacenix whoami` — show current sign-in state.

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;

use crate::config::Config;
use crate::store::credentials::Credentials;

pub async fn run(config: Arc<Config>) -> Result<ExitCode> {
    let Some(creds) = Credentials::load(&config.credentials_file())? else {
        println!("not signed in");
        return Ok(ExitCode::from(1));
    };
    let short_id = short(&creds.identity, 8);
    println!("identity  {short_id}");
    println!("saved at  {}", creds.saved_at.format("%Y-%m-%d %H:%M:%S UTC"));
    if let Some(email) = creds.email.as_deref() {
        println!("email     {email}");
    }
    println!("server    {}", config.stdb_uri);
    println!("module    {}", config.stdb_module);
    Ok(ExitCode::from(0))
}

fn short(hex: &str, n: usize) -> String {
    if hex.len() <= 2 * n {
        hex.to_string()
    } else {
        format!("{}…{}", &hex[..n], &hex[hex.len() - n..])
    }
}
