//! Persistent configuration for the TUI / CLI / service.
//!
//! Layout under the config dir (XDG-aware):
//!
//! ```text
//! config.toml      resolved on every run
//! credentials.toml { identity, token } after a successful login
//! sync.toml        local list of selected file ids / paths
//! service.lock     pid + bound port while the service is running
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_STDB_URI: &str = "https://maincloud.spacetimedb.com";
pub const DEFAULT_STDB_MODULE: &str =
    "c200d8e8d89037d457594229d7e351160475577171a6273c6d29f204fb98ee8a";

#[derive(Clone, Debug)]
pub struct Config {
    pub config_dir: PathBuf,
    pub stdb_uri: String,
    pub stdb_module: String,
    pub sync_root: PathBuf,
}

impl Config {
    pub fn resolve(
        override_dir: Option<&Path>,
        override_uri: Option<&str>,
        override_module: Option<&str>,
    ) -> Result<Self> {
        let config_dir = match override_dir {
            Some(p) => p.to_path_buf(),
            None => default_config_dir()?,
        };
        std::fs::create_dir_all(&config_dir)
            .with_context(|| format!("creating config dir {}", config_dir.display()))?;
        let preferences = Preferences::load(&config_dir.join("config.toml"));

        let stdb_uri = override_uri
            .map(str::to_owned)
            .or_else(|| std::env::var("SPACENIX_STDB_URI").ok())
            .or(preferences.stdb_uri)
            .unwrap_or_else(|| DEFAULT_STDB_URI.to_string());
        let stdb_module = override_module
            .map(str::to_owned)
            .or_else(|| std::env::var("SPACENIX_STDB_MODULE").ok())
            .or(preferences.stdb_module)
            .unwrap_or_else(|| DEFAULT_STDB_MODULE.to_string());
        let sync_root = std::env::var_os("SPACENIX_SYNC_ROOT")
            .map(PathBuf::from)
            .or(preferences.sync_root)
            .unwrap_or_else(default_sync_root);

        Ok(Self {
            config_dir,
            stdb_uri,
            stdb_module,
            sync_root,
        })
    }

    #[allow(dead_code)]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.config_dir.join("credentials.toml")
    }

    pub fn sync_file(&self) -> PathBuf {
        self.config_dir.join("sync.toml")
    }

    pub fn device_file(&self) -> PathBuf {
        self.config_dir.join("device.toml")
    }

    pub fn service_lock_file(&self) -> PathBuf {
        self.config_dir.join("service.lock")
    }
}

fn default_config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("spacenix"));
    }
    let home = dirs::home_dir().context("HOME is not set and XDG_CONFIG_HOME is not set")?;
    Ok(home.join(".config").join("spacenix"))
}

fn default_sync_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SpaceNix")
}

/// Optional persistent preferences written to `config.toml`. The CLI accepts
/// `--config-dir` / `SPACENIX_STDB_URI` / `SPACENIX_STDB_MODULE` to override.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Preferences {
    #[serde(default)]
    pub stdb_uri: Option<String>,
    #[serde(default)]
    pub stdb_module: Option<String>,
    /// Local directory the sync worker materializes files into.
    #[serde(default)]
    pub sync_root: Option<PathBuf>,
}

impl Preferences {
    #[allow(dead_code)]
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&raw).unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("serializing preferences")?;
        std::fs::write(path, raw)
            .with_context(|| format!("writing preferences to {}", path.display()))?;
        Ok(())
    }
}
