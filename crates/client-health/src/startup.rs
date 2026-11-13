//! Cold start timing.
//!
//! `main` calls [`mark_process_start`] first; the window calls [`first_page_loaded`] when its page has loaded.
//! The time between them is the client's cold start, shown on the metrics endpoint. With
//! `ANIXOPS_STARTUP_PROBE=<file>` set (CI's cold start job) the client writes `{"ready_ms": n}` to that file
//! and exits at that point, so the job can time whole launches of the real binary.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const PROBE_ENV: &str = "ANIXOPS_STARTUP_PROBE";
/// The cold start budget CI holds the clients to.
pub const BUDGET: Duration = Duration::from_millis(1500);

static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static READY: OnceLock<Duration> = OnceLock::new();

/// Records the start of the process; later calls keep the first time.
pub fn mark_process_start() {
    let _ = PROCESS_START.get_or_init(Instant::now);
}

/// Records the first page load and returns the time since the process started. Later calls return the first
/// value.
pub fn first_page_loaded() -> Duration {
    *READY.get_or_init(|| {
        PROCESS_START
            .get()
            .map_or(Duration::ZERO, |start| start.elapsed())
    })
}

/// The recorded cold start, once the page has loaded.
pub fn cold_start() -> Option<Duration> {
    READY.get().copied()
}

/// The probe file, when CI asked for one.
pub fn probe_path() -> Option<PathBuf> {
    std::env::var_os(PROBE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Writes the probe result; the caller exits afterwards.
pub fn write_probe(path: &std::path::Path, ready: Duration) -> std::io::Result<()> {
    let text = format!("{{\"ready_ms\": {}}}\n", ready.as_millis());
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::{cold_start, first_page_loaded, mark_process_start, write_probe};

    #[test]
    fn the_first_page_load_is_kept() {
        mark_process_start();
        let first = first_page_loaded();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(first_page_loaded(), first);
        assert_eq!(cold_start(), Some(first));
        let path = std::env::temp_dir().join(format!("client-health-probe-{}", std::process::id()));
        write_probe(&path, first).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(value["ready_ms"].is_u64());
        let _ = std::fs::remove_file(&path);
    }
}
