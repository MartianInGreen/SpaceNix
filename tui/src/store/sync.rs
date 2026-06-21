//! Per-device sync selection persisted to `sync.toml`.
//!
//! The TUI lets the user tick which `UserFile` rows they want this device to
//! keep in sync. The selection is keyed by file id (and tracks the latest
//! known path so we can detect when the server-side name/path changed).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncSelection {
    /// file id -> selection
    #[serde(default)]
    pub selected: BTreeMap<u64, SelectedFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedFile {
    /// Server-side file id.
    pub id: u64,
    /// Path on the server at the time the user selected it.
    pub path: Option<String>,
    /// Server-side name at the time the user selected it.
    pub name: String,
    /// Whether the row was a folder at the time of selection.
    #[serde(default)]
    pub is_directory: bool,
    /// Local override of where to materialize. Defaults to the server path
    /// under `sync_root`.
    #[serde(default)]
    pub local_path: Option<String>,
    /// When the user first enabled syncing for this row.
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl SyncSelection {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        let sel: Self = toml::from_str(&raw).context("parsing sync.toml")?;
        Ok(sel)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("serializing sync selection")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating sync dir {}", parent.display()))?;
        }
        std::fs::write(path, raw).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn contains(&self, id: u64) -> bool {
        self.selected.contains_key(&id)
    }

    pub fn toggle(&mut self, file: &SelectedFile) -> bool {
        if self.selected.remove(&file.id).is_some() {
            false
        } else {
            self.selected.insert(file.id, file.clone());
            true
        }
    }
}
