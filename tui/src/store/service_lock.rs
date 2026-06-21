//! File lock for the background service. Holds the bound port + pid so other
//! invocations of `spacenix service start` / `service stop` can find it.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceLock {
    pub pid: u32,
    pub port: u16,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl ServiceLock {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        let lock: Self = toml::from_str(&raw).context("parsing service.lock")?;
        Ok(Some(lock))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("serializing service lock")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating lock dir {}", parent.display()))?;
        }
        std::fs::write(path, raw)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
