//! Local device selection persisted to `device.toml`.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalDevice {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub hostname: Option<String>,
    pub selected_at: chrono::DateTime<chrono::Utc>,
}

impl LocalDevice {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        let device: Self = toml::from_str(&raw).context("parsing device.toml")?;
        Ok(Some(device))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("serializing local device")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating device parent dir {}", parent.display()))?;
        }
        std::fs::write(path, raw).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
