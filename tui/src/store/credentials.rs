//! Persistent credentials file: SpacetimeDB identity + token.
//!
//! Kept in a separate file with conservative permissions (0600 on unix).

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    /// SpacetimeDB identity hex string.
    pub identity: String,
    /// SpacetimeDB auth token (rotate this via `spacenix logout && spacenix login`).
    pub token: String,
    /// Optional display email captured from the server's `my_user` view.
    #[serde(default)]
    pub email: Option<String>,
    /// When the credentials were last written.
    pub saved_at: chrono::DateTime<chrono::Utc>,
}

impl Credentials {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", path.display()));
            }
        };
        let creds: Self = toml::from_str(&raw).context("parsing credentials.toml")?;
        Ok(Some(creds))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("serializing credentials")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("creating credentials parent dir {}", parent.display())
            })?;
        }
        write_atomic(path, raw.as_bytes())
            .with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms).ok();
        }
        Ok(())
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("creds")
    ));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
