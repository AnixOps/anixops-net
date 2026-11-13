//! Crash reports: a panic hook that writes a redacted report to `<dir>/crashes/`, and the list of reports the
//! next start, the diagnostics export or an upload picks up.
//!
//! A report holds the client name and version, the operating system and architecture, the thread name, the
//! panic message and location and a backtrace, all passed through [`crate::redact::redact`]. It never holds
//! profile content, subscription URLs, node addresses or account names. Turning crash reports off (the
//! `crash_reports` setting) stops new reports and deletes the kept ones.
//!
//! Only Rust panics are caught; a native fault in a system library still ends the process without a report.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::redact::redact;

/// Reports kept on disk; older ones are deleted when a new one is written.
pub const MAX_REPORTS: usize = 10;
/// The address a build sends reports to, if any (`ANIXOPS_CRASH_REPORT_URL` when the client was compiled).
pub const UPLOAD_URL: Option<&str> = option_env!("ANIXOPS_CRASH_REPORT_URL");

/// Whether reports are written; shared with the settings page so turning it off takes effect at once.
#[derive(Clone, Debug)]
pub struct CrashReports {
    dir: PathBuf,
    enabled: Arc<AtomicBool>,
}

impl CrashReports {
    /// Reports for a client whose state lives in `state_dir`.
    pub fn new(state_dir: &Path, enabled: bool) -> Self {
        Self {
            dir: state_dir.join("crashes"),
            enabled: Arc::new(AtomicBool::new(enabled)),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Turns reports on or off; off deletes the kept reports.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// Installs the panic hook. The previous hook still runs afterwards (it prints the panic to stderr).
    pub fn install(&self, client: &'static str, version: &'static str) {
        let reports = self.clone();
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if reports.enabled() {
                let _ = reports.write(&report(client, version, info));
            }
            previous(info);
        }));
    }

    /// Writes one report and trims the directory to [`MAX_REPORTS`].
    pub fn write(&self, report: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        let path = self.dir.join(format!("crash-{millis}.txt"));
        fs::write(&path, report)?;
        let mut kept = self.pending();
        while kept.len() > MAX_REPORTS {
            let _ = fs::remove_file(kept.remove(0));
        }
        Ok(path)
    }

    /// Kept reports, oldest first.
    pub fn pending(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("crash-") && name.ends_with(".txt"))
            })
            .collect();
        paths.sort();
        paths
    }

    /// The kept reports joined for the diagnostics export (each redacted again, in case an older build wrote it).
    pub fn export(&self) -> String {
        let mut out = String::new();
        for path in self.pending() {
            if let Ok(text) = fs::read_to_string(&path) {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("crash");
                let _ = writeln!(out, "--- {name} ---\n{}", redact(&text));
            }
        }
        out
    }

    /// Sends each kept report with `send` when the build names an upload address and reports are on; a sent
    /// report is deleted. Returns how many were sent.
    pub fn upload(&self, send: impl Fn(&str, &str) -> Result<(), String>) -> usize {
        let Some(url) = UPLOAD_URL.filter(|url| !url.is_empty()) else {
            return 0;
        };
        if !self.enabled() {
            return 0;
        }
        let mut sent = 0;
        for path in self.pending() {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if send(url, &redact(&text)).is_ok() {
                let _ = fs::remove_file(&path);
                sent += 1;
            }
        }
        sent
    }
}

/// The report text for one panic.
pub fn report(client: &str, version: &str, info: &PanicHookInfo<'_>) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-text panic payload".to_string());
    let location = info.location().map_or_else(
        || "unknown".to_string(),
        |location| format!("{}:{}", location.file(), location.line()),
    );
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let backtrace = std::backtrace::Backtrace::force_capture();
    compose(
        client,
        version,
        &thread,
        &message,
        &location,
        &backtrace.to_string(),
    )
}

fn compose(
    client: &str,
    version: &str,
    thread: &str,
    message: &str,
    location: &str,
    backtrace: &str,
) -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let text = format!(
        "{client} {version} crash report\nunix_time: {time}\nos: {} {}\nthread: {thread}\nlocation: {location}\nmessage: {message}\nbacktrace:\n{backtrace}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    redact(&text)
}

#[cfg(test)]
mod tests {
    use super::{compose, CrashReports, MAX_REPORTS};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("client-health-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn reports_are_redacted_and_trimmed() {
        let dir = temp_dir("crash");
        let reports = CrashReports::new(&dir, true);
        let text = compose(
            "anixops-net",
            "0.3.0",
            "main",
            "fetch https://panel.example.com/sub?token=secret failed for node 198.51.100.4",
            "src/app.rs:10",
            "0: std::rt::lang_start",
        );
        assert!(!text.contains("panel.example.com"));
        assert!(!text.contains("198.51.100.4"));
        assert!(text.contains("src/app.rs:10"));
        for _ in 0..MAX_REPORTS + 3 {
            reports.write(&text).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(reports.pending().len(), MAX_REPORTS);
        assert!(reports.export().contains("crash report"));
        reports.set_enabled(false);
        assert!(reports.pending().is_empty());
        assert_eq!(reports.upload(|_, _| Ok(())), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
