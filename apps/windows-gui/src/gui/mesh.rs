//! 组网 (mesh) in this window (docs/mesh-ui.md; docs/architecture/mesh-client.md): the saved networks, sealed for
//! this Windows account under the local application data folder (`client_core::backend::mesh`), and what the page
//! reads while no engine runs. The engine this window runs takes the joined networks with the profile
//! ([`with_joined`]) and joining, leaving, saving and removing at once ([`engine_request`]). With the proxy off and a
//! network joined, an engine runs for the mesh alone (mesh-status.md, 只开组网) on its own adapter, which carries only
//! the mesh's routes as the engine names them ([`settle`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use client_core::backend::mesh;
use serde_json::{json, Value};

use super::actions::connection::DesktopCore;
use super::capture_sessions;
use super::tun_mode::TunStartError;

/// `%LOCALAPPDATA%\AnixOps Net`, beside the LAN share's sealed password.
pub fn dir() -> Result<PathBuf, String> {
    capture_sessions::capture_dir()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "the local application data folder could not be found".to_string())
}

/// One mesh request to the engine this window runs; `None` while none runs, else its answer (`{"ok":true}` or its
/// refusal, `{"ok":false,"error":{code,detail}}`).
pub fn engine_request(core: &Mutex<DesktopCore>, request: Value) -> Option<Value> {
    let mut core = core.lock().ok()?;
    let bytes = match serde_json::to_vec(&request) {
        Ok(bytes) => bytes,
        Err(error) => return Some(refusal(&error.to_string())),
    };
    let answer = if core.is_running().unwrap_or(false) {
        core.control(&bytes)
    } else {
        core.mesh_only_control(&bytes)?
    };
    let answer = answer
        .and_then(|answer| serde_json::from_slice(&answer).map_err(|error| error.to_string()));
    Some(answer.unwrap_or_else(|error| refusal(&error)))
}

/// How often the engine for the mesh alone is asked for its routes (R23: 2 s).
const ROUTES_POLL: Duration = Duration::from_secs(2);

/// The first wait before a start that failed, or an engine that ended by itself, is tried again; it doubles each
/// time up to [`RETRY_MAX`].
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// Counts the starts of the engine for the mesh alone; a route watch ends once a newer start began, or the engine
/// was stopped.
static WATCH: AtomicU64 = AtomicU64::new(0);

/// Counts the calls to [`settle`]; a waiting retry gives way to a newer call.
static RETRY: AtomicU64 = AtomicU64::new(0);

/// Set while the window exits or the session ends ([`end`]): nothing starts the engine for the mesh alone again
/// until [`resume`].
static ENDING: AtomicBool = AtomicBool::new(false);

/// Why the engine for the mesh alone is not running although a network is joined (`network.failure`,
/// mesh-status.md), for the page while it shows the saved networks.
static FAILURE: Mutex<Option<Value>> = Mutex::new(None);

/// Whether the engine should run for the mesh alone: this build runs mesh networks and a saved one is joined.
fn wanted() -> bool {
    mesh::supported()
        && dir()
            .and_then(|dir| mesh::load(&dir))
            .is_ok_and(|networks| networks.iter().any(|network| network.joined))
}

/// Brings the engine for the mesh alone in line with the rest, after a connection started or ended, after a network
/// was saved, joined, left or removed, and after the network changed: it runs while the proxy is off and a network is
/// joined, and stops otherwise. A start that failed, or an engine that ended by itself, is shown on the page
/// (`network.failure`) and tried again after a growing wait.
pub fn settle(core: &Arc<Mutex<DesktopCore>>) {
    RETRY.fetch_add(1, Ordering::AcqRel);
    settle_attempt(core, 0);
}

fn settle_attempt(core: &Arc<Mutex<DesktopCore>>, attempt: u32) {
    if ENDING.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut guard) = core.lock() else {
        return;
    };
    let proxy = guard.is_running().unwrap_or(false);
    if proxy || !wanted() {
        WATCH.fetch_add(1, Ordering::AcqRel);
        set_failure(None);
        if guard.stop_mesh_only() {
            log("mesh only: stopped");
        }
        return;
    }
    if guard.mesh_only_running() {
        return;
    }
    // One that ended by itself lets go of its adapter before the next starts.
    guard.stop_mesh_only();
    let started = dir()
        .and_then(|dir| mesh::load(&dir))
        .and_then(|networks| {
            mesh::only_profile(&mesh::outbounds(&networks))
                .ok_or_else(|| "no mesh network is joined".to_string())
        })
        .map_err(|message| TunStartError {
            code: "other",
            message,
        })
        .and_then(|profile| guard.start_mesh_only(profile.to_string().as_bytes()));
    drop(guard);
    match started {
        Ok(()) => {
            set_failure(None);
            log("mesh only: started");
            watch_routes(core.clone());
        }
        Err(error) => {
            log(&format!("mesh only: the engine did not start: {error}"));
            set_failure(Some(json!({ "code": error.code, "detail": null })));
            retry_later(core.clone(), attempt);
        }
    }
}

/// Calls [`settle_attempt`] again after the wait for `attempt`, unless a newer [`settle`] came first or the window
/// is ending.
fn retry_later(core: Arc<Mutex<DesktopCore>>, attempt: u32) {
    let generation = RETRY.load(Ordering::Acquire);
    let wait = retry_wait(attempt);
    let spawned = thread::Builder::new()
        .name("mesh-retry".into())
        .spawn(move || {
            thread::sleep(wait);
            if RETRY.load(Ordering::Acquire) != generation || ENDING.load(Ordering::Acquire) {
                return;
            }
            settle_attempt(&core, attempt.saturating_add(1));
        });
    if let Err(error) = spawned {
        log(&format!("mesh only: the retry did not start: {error}"));
    }
}

/// 5 s, 10 s, 20 s, 40 s, then every 60 s.
fn retry_wait(attempt: u32) -> Duration {
    RETRY_FIRST
        .checked_mul(1u32.checked_shl(attempt.min(16)).unwrap_or(u32::MAX))
        .map_or(RETRY_MAX, |wait| wait.min(RETRY_MAX))
}

fn set_failure(failure: Option<Value>) {
    if let Ok(mut current) = FAILURE.lock() {
        *current = failure;
    }
}

/// Stops the engine for the mesh alone, its route watch and any retry, and keeps it from starting again: as the
/// window exits, and as Windows ends the session (sign-out, shutdown, restart), so that its adapter and routes go
/// with it rather than with the process. `reason` goes to the log.
pub fn end(core: &Mutex<DesktopCore>, reason: &str) {
    ENDING.store(true, Ordering::Release);
    WATCH.fetch_add(1, Ordering::AcqRel);
    RETRY.fetch_add(1, Ordering::AcqRel);
    let Ok(mut guard) = core.lock() else {
        return;
    };
    if guard.stop_mesh_only() {
        log(&format!("mesh only: stopped ({reason})"));
    }
}

/// The session goes on after all (a cancelled session end): the engine for the mesh alone may run again.
pub fn resume(core: &Arc<Mutex<DesktopCore>>) {
    ENDING.store(false, Ordering::Release);
    settle(core);
}

/// Puts the engine's mesh routes on the adapter every [`ROUTES_POLL`] while the engine for the mesh alone runs;
/// ends with it, or when a newer start began or it was stopped. An engine that ended by itself is shown on the page
/// and started again ([`retry_later`]).
fn watch_routes(core: Arc<Mutex<DesktopCore>>) {
    let generation = WATCH.fetch_add(1, Ordering::AcqRel) + 1;
    let spawned = thread::Builder::new()
        .name("mesh-routes".into())
        .spawn(move || loop {
            thread::sleep(ROUTES_POLL);
            if WATCH.load(Ordering::Acquire) != generation {
                return;
            }
            let Ok(mut guard) = core.lock() else {
                return;
            };
            if WATCH.load(Ordering::Acquire) != generation {
                return;
            }
            if guard.mesh_only_ended() {
                guard.stop_mesh_only();
                drop(guard);
                if ENDING.load(Ordering::Acquire) {
                    return;
                }
                log("mesh only: the engine ended by itself");
                set_failure(Some(json!({ "code": "other", "detail": null })));
                retry_later(core.clone(), 0);
                return;
            }
            if !guard.mesh_only_running() {
                return;
            }
            match guard.apply_mesh_routes() {
                Ok(Some(count)) => log(&format!("mesh only: {count} routes applied")),
                Ok(None) => {}
                Err(error) => log(&format!(
                    "mesh only: the routes could not be applied: {error}"
                )),
            }
        });
    if let Err(error) = spawned {
        log(&format!(
            "mesh only: the route watch did not start: {error}"
        ));
    }
}

/// `mesh_status` while the proxy's engine does not run: the engine for the mesh alone's own answer while it runs
/// (`request` is the window's, already checked), else the saved networks as the page shows them offline
/// ([`mesh::offline_status`]) on a build whose engine runs mesh networks, else the engine's own refusal, which
/// hides the entry.
pub fn offline_status(core: &DesktopCore, request: &[u8]) -> Value {
    if let Some(answer) = core.mesh_only_control(request) {
        return answer
            .and_then(|answer| serde_json::from_slice(&answer).map_err(|error| error.to_string()))
            .unwrap_or_else(|error| refusal(&error));
    }
    if !mesh::supported() {
        return json!({ "ok": false, "error": { "code": "unsupported" } });
    }
    match dir().and_then(|dir| mesh::load(&dir)) {
        Ok(networks) => with_failure(mesh::offline_status(&networks), current_failure()),
        Err(error) => refusal(&error),
    }
}

fn current_failure() -> Option<Value> {
    FAILURE.lock().ok().and_then(|failure| failure.clone())
}

/// The saved networks' status with why the engine for the mesh alone does not run, on a joined network: `failed`
/// with that `failure`.
fn with_failure(mut status: Value, failure: Option<Value>) -> Value {
    let Some(failure) = failure else {
        return status;
    };
    if let Some(network) = status.get_mut("network").and_then(Value::as_object_mut) {
        if network.get("state").and_then(Value::as_str) == Some("joining") {
            network.insert("state".into(), json!("failed"));
            network.insert("failure".into(), failure);
        }
    }
    status
}

/// `envelope` with the joined networks' outbounds; the reason, when they were left out, goes to the log.
pub fn with_joined(envelope: String) -> String {
    let dir = match dir() {
        Ok(dir) => dir,
        Err(error) => {
            log(&error);
            return envelope;
        }
    };
    let (envelope, left_out) = mesh::with_joined(&dir, envelope);
    if let Some(why) = left_out {
        log(&why);
    }
    envelope
}

fn refusal(detail: &str) -> Value {
    json!({ "ok": false, "error": { "code": "other", "detail": detail } })
}

fn log(message: &str) {
    let _ = super::append_managed_log("tauri-gui", &format!("mesh: {message}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_start_is_tried_again_after_a_growing_wait() {
        let waits: Vec<u64> = (0..7)
            .map(|attempt| retry_wait(attempt).as_secs())
            .collect();
        assert_eq!(waits, [5, 10, 20, 40, 60, 60, 60]);
        assert_eq!(retry_wait(u32::MAX), RETRY_MAX);
    }

    #[test]
    fn a_failed_start_is_shown_on_a_joined_network() {
        let other = json!({ "code": "other", "detail": null });
        let joining = json!({ "network": { "id": "office", "state": "joining" } });
        let failed = with_failure(joining.clone(), Some(other.clone()));
        assert_eq!(failed["network"]["state"], "failed");
        assert_eq!(failed["network"]["failure"], other);
        assert_eq!(with_failure(joining.clone(), None), joining);
        // A network that is not joined has nothing to fail.
        let left = json!({ "network": { "id": "office", "state": "not_joined" } });
        assert_eq!(with_failure(left.clone(), Some(other)), left);
    }

    #[test]
    fn an_engine_that_does_not_run_answers_nothing() {
        let core = Mutex::new(DesktopCore::default());
        assert_eq!(
            engine_request(&core, json!({ "cmd": "mesh_leave", "network": "office" })),
            None
        );
    }
}
