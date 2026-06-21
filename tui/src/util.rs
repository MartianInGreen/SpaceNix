//! Small helpers shared across modules.

use anyhow::Result;

#[allow(dead_code)]
pub mod formatting {
    use chrono::{DateTime, Utc};

    pub fn short_ts(ts: &spacetimedb_sdk::Timestamp) -> String {
        let micros = ts.to_micros_since_unix_epoch();
        let dt: DateTime<Utc> = DateTime::<Utc>::from_timestamp_micros(micros)
            .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
        dt.format("%Y-%m-%d %H:%M").to_string()
    }

    pub fn bytes(n: u64) -> String {
        const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
        let mut value = n as f64;
        let mut unit = 0;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{} {}", n, UNITS[0])
        } else {
            format!("{:.1} {}", value, UNITS[unit])
        }
    }

    /// Format a bytes-per-second value with the largest unit that keeps
    /// the number ≥ 1 (e.g. `1.4 KiB/s`, `38 B/s`).
    pub fn bytes_per_sec(bps: f64) -> String {
        if !bps.is_finite() || bps <= 0.0 {
            return "0 B/s".to_string();
        }
        const UNITS: &[&str] = &["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"];
        let mut value = bps;
        let mut unit = 0;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{} {}", bps as u64, UNITS[0])
        } else {
            format!("{:.1} {}", value, UNITS[unit])
        }
    }
}

#[allow(dead_code)]
pub async fn open_url(url: &str) -> Result<()> {
    open::that_detached(url)?;
    Ok(())
}
