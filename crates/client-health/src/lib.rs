//! Health of the desktop clients (Windows and Linux), written in-house with no telemetry library:
//!
//! - [`metrics`]: the engine's Prometheus text plus the client's own gauges, served on a loopback port.
//! - [`crash`]: a panic hook that keeps a redacted report on disk; reports can be turned off.
//! - [`redact`]: removes URLs, hosts, addresses, e-mail addresses, keys and the user's name from text.
//! - [`integrity`]: checks the installed files against the `integrity.sha256` list the release writes.
//! - [`startup`]: the cold start probe CI runs.
//! - [`settings`]: the health settings file (`health.json`) beside the client's other state.
//!
//! Nothing here sends data anywhere unless the build names a crash report address
//! (`ANIXOPS_CRASH_REPORT_URL` at compile time) and the user leaves crash reports on.

pub mod crash;
pub mod integrity;
pub mod metrics;
pub mod redact;
pub mod settings;
pub mod startup;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{json, Value};

use crate::crash::CrashReports;
use crate::integrity::Integrity;
use crate::metrics::{ClientMetrics, MetricsServer};
use crate::settings::HealthSettings;

/// The engine's metrics text, when the engine runs, and whether the client is connected.
pub type EngineProbe = Arc<dyn Fn() -> (Option<String>, bool) + Send + Sync>;

/// One client's health pieces together: settings, crash reports, the integrity result and the endpoint.
pub struct Health {
    dir: PathBuf,
    platform: &'static str,
    version: &'static str,
    settings: Mutex<HealthSettings>,
    crash: CrashReports,
    integrity: Arc<Mutex<Option<Integrity>>>,
    server: Mutex<Option<MetricsServer>>,
    server_error: Mutex<Option<String>>,
    /// What the endpoint asks the engine, kept so turning the endpoint on later can start it.
    engine: Mutex<Option<EngineProbe>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Health {
    /// Loads `health.json` from `dir`, installs the crash hook and starts the integrity check of the
    /// executable `exe` on a background thread, so the window is not held up by hashing.
    pub fn start(
        dir: &Path,
        platform: &'static str,
        version: &'static str,
        exe: PathBuf,
    ) -> Arc<Self> {
        let settings = HealthSettings::load(dir);
        let crash = CrashReports::new(dir, settings.crash_reports);
        crash.install(platform, version);
        let integrity = Arc::new(Mutex::new(None));
        let result = integrity.clone();
        let _ = std::thread::Builder::new()
            .name("integrity".to_string())
            .spawn(move || {
                *lock(&result) = Some(integrity::check(&exe));
            });
        Arc::new(Self {
            dir: dir.to_path_buf(),
            platform,
            version,
            settings: Mutex::new(settings),
            crash,
            integrity,
            server: Mutex::new(None),
            server_error: Mutex::new(None),
            engine: Mutex::new(None),
        })
    }

    pub fn crash_reports(&self) -> &CrashReports {
        &self.crash
    }

    /// The integrity result, `None` while the check runs.
    pub fn integrity(&self) -> Option<Integrity> {
        lock(&self.integrity).clone()
    }

    fn client_metrics(&self, connected: bool) -> ClientMetrics {
        ClientMetrics {
            platform: self.platform,
            version: self.version,
            connected,
            crash_reports_enabled: self.crash.enabled(),
            crash_reports_kept: self.crash.pending().len(),
            integrity: self.integrity().map_or("", |result| result.state()),
            cold_start_ms: startup::cold_start().map(|ready| ready.as_millis() as u64),
        }
    }

    /// Starts the loopback endpoint at the configured address, unless it is turned off (the default). A port
    /// that is taken is reported in [`Health::status`], not as an error: the client runs without the endpoint.
    pub fn serve_metrics(self: &Arc<Self>, engine: EngineProbe) {
        *lock(&self.engine) = Some(engine);
        self.start_metrics();
    }

    fn start_metrics(self: &Arc<Self>) {
        let Some(address) = lock(&self.settings).metrics_address() else {
            return;
        };
        let Some(engine) = lock(&self.engine).clone() else {
            return;
        };
        // Held while starting, so two starts at once cannot both bind.
        let mut server = lock(&self.server);
        if server.is_some() {
            return;
        }
        *lock(&self.server_error) = None;
        let health = Arc::downgrade(self);
        let render = Arc::new(move || {
            let (text, connected) = engine();
            match health.upgrade() {
                Some(health) => metrics::combine(text, &health.client_metrics(connected)),
                None => text.unwrap_or_default(),
            }
        });
        match MetricsServer::start(address, render) {
            Ok(started) => *server = Some(started),
            Err(error) => *lock(&self.server_error) = Some(format!("{address}: {error}")),
        }
    }

    /// Turns the metrics endpoint on or off, saves the choice and starts or stops it now.
    pub fn set_metrics(self: &Arc<Self>, enabled: bool) -> std::io::Result<()> {
        {
            let mut settings = lock(&self.settings);
            if enabled && settings.metrics_listen.is_none() {
                settings.metrics_listen = Some(crate::settings::DEFAULT_METRICS_LISTEN);
            }
            settings.metrics_enabled = enabled;
            settings.save(&self.dir)?;
        }
        if enabled {
            self.start_metrics();
        } else {
            // Taken out first: dropping the server waits for its thread, which must not wait on this lock.
            let server = lock(&self.server).take();
            drop(server);
            *lock(&self.server_error) = None;
        }
        Ok(())
    }

    /// Turns crash reports on or off and saves the choice; off deletes kept reports.
    pub fn set_crash_reports(&self, enabled: bool) -> std::io::Result<()> {
        let mut settings = lock(&self.settings);
        settings.crash_reports = enabled;
        settings.save(&self.dir)?;
        self.crash.set_enabled(enabled);
        Ok(())
    }

    /// The settings page's view (camelCase JSON).
    pub fn status(&self) -> Value {
        let integrity = self.integrity();
        json!({
            "crashReports": self.crash.enabled(),
            "crashReportsKept": self.crash.pending().len(),
            "crashUploadConfigured": crash::UPLOAD_URL.is_some_and(|url| !url.is_empty()),
            "metricsEnabled": lock(&self.settings).metrics_enabled,
            "metricsAddress": lock(&self.server).as_ref().map(|server| format!("http://{}/metrics", server.address())),
            "metricsError": lock(&self.server_error).clone(),
            "integrity": integrity.as_ref().map_or("checking", Integrity::state),
            "integrityDetail": integrity.as_ref().map(Integrity::detail),
            "coldStartMs": startup::cold_start().map(|ready| ready.as_millis() as u64),
        })
    }

    /// The health section of a diagnostics report: integrity, cold start, the endpoint and the kept crash
    /// reports, all redacted.
    pub fn diagnostics(&self) -> String {
        let integrity = self.integrity();
        let mut out = format!(
            "integrity: {} ({})\ncold start: {}\nmetrics endpoint: {}\ncrash reports: {} ({} kept)\n",
            integrity.as_ref().map_or("checking", Integrity::state),
            integrity.as_ref().map_or_else(String::new, Integrity::detail),
            startup::cold_start().map_or_else(|| "unknown".to_string(), |ready| format!("{} ms", ready.as_millis())),
            lock(&self.server)
                .as_ref()
                .map_or_else(|| lock(&self.server_error).clone().unwrap_or_else(|| "off".to_string()), |server| server.address().to_string()),
            if self.crash.enabled() { "on" } else { "off" },
            self.crash.pending().len(),
        );
        let reports = self.crash.export();
        if !reports.is_empty() {
            out.push_str("crash reports:\n");
            out.push_str(&reports);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_starts_only_when_turned_on_and_stops_when_turned_off() {
        let dir = std::env::temp_dir().join(format!("client-health-toggle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // An earlier version's file: an address, no switch.
        std::fs::write(
            HealthSettings::path(&dir),
            r#"{"crash_reports": true, "metrics_listen": "127.0.0.1:0"}"#,
        )
        .unwrap();
        let health = Health::start(&dir, "linux", "0.0.0", dir.join("missing-exe"));
        health.serve_metrics(Arc::new(|| (None, false)));
        assert!(health.status()["metricsAddress"].is_null());
        assert_eq!(health.status()["metricsEnabled"], false);

        health.set_metrics(true).unwrap();
        assert!(health.status()["metricsAddress"]
            .as_str()
            .is_some_and(|address| address.starts_with("http://127.0.0.1:")));
        assert!(HealthSettings::load(&dir).metrics_enabled);

        health.set_metrics(false).unwrap();
        assert!(health.status()["metricsAddress"].is_null());
        assert!(!HealthSettings::load(&dir).metrics_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
