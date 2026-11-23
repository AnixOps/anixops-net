//! `health.json` in the client's state directory: whether crash reports are kept, whether the metrics endpoint
//! runs and where it listens. Missing or unreadable files give the defaults.

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
    /// Serve `/metrics`. Off by default and turned on in the settings page: the endpoint names policy groups,
    /// their chosen nodes and rule sets, which other users of the computer could read from a loopback port.
    /// A file saved before this switch existed has none and leaves the endpoint off, whatever its
    /// `metrics_listen` (every save wrote that address, so it never meant the user turned the endpoint on).
    pub metrics_enabled: bool,
    /// Where `/metrics` is served when it is on; `None` keeps it off. Only loopback addresses are accepted.
    pub metrics_listen: Option<SocketAddr>,
}

impl Default for HealthSettings {
    fn default() -> Self {
        Self {
            crash_reports: true,
            metrics_enabled: false,
            metrics_listen: Some(DEFAULT_METRICS_LISTEN),
        }
    }
}

impl HealthSettings {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// Where the endpoint listens when it should run.
    pub fn metrics_address(&self) -> Option<SocketAddr> {
        self.metrics_listen.filter(|_| self.metrics_enabled)
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
        let metrics_enabled = value
            .get("metrics_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.metrics_enabled);
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
            metrics_enabled,
            metrics_listen,
        }
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let value = json!({
            "crash_reports": self.crash_reports,
            "metrics_enabled": self.metrics_enabled,
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
            metrics_enabled: false,
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
        let on = HealthSettings {
            metrics_enabled: true,
            ..HealthSettings::default()
        };
        on.save(&dir).unwrap();
        assert_eq!(HealthSettings::load(&dir), on);
        assert_eq!(on.metrics_address(), Some(super::DEFAULT_METRICS_LISTEN));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_endpoint_is_off_by_default_and_for_files_without_the_switch() {
        let dir =
            std::env::temp_dir().join(format!("client-health-settings-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(HealthSettings::load(&dir).metrics_address(), None);
        std::fs::create_dir_all(&dir).unwrap();
        // What every earlier version saved after the crash report switch was touched.
        std::fs::write(
            HealthSettings::path(&dir),
            r#"{"crash_reports": false, "metrics_listen": "127.0.0.1:9477"}"#,
        )
        .unwrap();
        let loaded = HealthSettings::load(&dir);
        assert!(!loaded.crash_reports);
        assert!(!loaded.metrics_enabled);
        assert_eq!(loaded.metrics_address(), None);
        std::fs::write(
            HealthSettings::path(&dir),
            r#"{"metrics_enabled": true, "metrics_listen": null}"#,
        )
        .unwrap();
        assert_eq!(HealthSettings::load(&dir).metrics_address(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
