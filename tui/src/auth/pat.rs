//! Personal access tokens (PATs) are issued by the server via the
//! `create_api_key` procedure. The TUI / CLI uses them to authenticate
//! scripts and the background service. Storing a PAT also lets the user
//! connect without going through the email/password flow.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatRecord {
    pub id: u64,
    pub name: String,
    pub token: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PatStore {
    pub tokens: Vec<PatRecord>,
}

impl PatStore {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        let store: Self = toml::from_str(&raw).context("parsing pats.toml")?;
        Ok(store)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("serializing pat store")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating pats dir {}", parent.display()))?;
        }
        std::fs::write(path, raw).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
