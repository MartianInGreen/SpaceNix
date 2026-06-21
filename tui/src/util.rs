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
}

#[allow(dead_code)]
pub async fn open_url(url: &str) -> Result<()> {
    open::that_detached(url)?;
    Ok(())
}
