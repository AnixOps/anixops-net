//! `health.json` in the client's state directory: whether crash reports are kept and where the metrics
//! endpoint listens. Missing or unreadable files give the defaults.

use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const FILE_NAME: &str = "health.json";
/// The metrics endpoint's default address: loopback only.
pub const DEFAULT_METRICS_LISTEN: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 9477);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthSettings {
    /// Keep redacted crash reports (and send them, in builds that name an upload address). On by default.
    pub crash_reports: bool,
    /// Where `/metrics` is served; `None` turns the endpoint off. Only loopback addresses are accepted.
    pub metrics_listen: Option<SocketAddr>,
}

impl Default for HealthSettings {
    fn default() -> Self {
        Self {
            crash_reports: true,
            metrics_listen: Some(DEFAULT_METRICS_LISTEN),
        }
    }
}

impl HealthSettings {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    pub fn load(dir: &Path) -> Self {
        let defaults = Self::default();
        let Some(value) = fs::read(Self::path(dir))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            return defaults;
        };
        let crash_reports = value
            .get("crash_reports")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.crash_reports);
        let metrics_listen = match value.get("metrics_listen") {
            Some(Value::Null) => None,
            Some(Value::String(text)) => text
                .parse::<SocketAddr>()
                .ok()
                .filter(|address| address.ip().is_loopback())
                .or(defaults.metrics_listen),
            _ => defaults.metrics_listen,
        };
        Self {
            crash_reports,
            metrics_listen,
        }
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let value = json!({
            "crash_reports": self.crash_reports,
            "metrics_listen": self.metrics_listen.map(|address| address.to_string()),
        });
        let bytes = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
        fs::write(Self::path(dir), bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::HealthSettings;

    #[test]
    fn settings_round_trip_and_reject_non_loopback() {
        let dir =
            std::env::temp_dir().join(format!("client-health-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(HealthSettings::load(&dir), HealthSettings::default());
        let off = HealthSettings {
            crash_reports: false,
            metrics_listen: None,
        };
        off.save(&dir).unwrap();
        assert_eq!(HealthSettings::load(&dir), off);
        std::fs::write(
            HealthSettings::path(&dir),
            r#"{"crash_reports": true, "metrics_listen": "0.0.0.0:9477"}"#,
        )
        .unwrap();
        assert_eq!(HealthSettings::load(&dir), HealthSettings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
