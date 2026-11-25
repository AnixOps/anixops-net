//! 组网 (mesh) in this window (docs/mesh-ui.md; docs/architecture/mesh-client.md): the saved networks, sealed for
//! this Windows account under the local application data folder (`client_core::backend::mesh`), and what the page
//! reads while no engine runs. The engine this window runs takes the joined networks with the profile
//! ([`with_joined`]) and joining, leaving, saving and removing at once ([`engine_request`]). With the proxy off and a
//! network joined, an engine runs for the mesh alone (mesh-status.md, 只开组网) on its own adapter, which carries only
//! the mesh's routes as the engine names them ([`settle`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use client_core::backend::mesh;
use serde_json::{json, Value};

use super::actions::connection::DesktopCore;
use super::capture_sessions;

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

/// Counts the starts of the engine for the mesh alone; a route watch ends once a newer start began.
static WATCH: AtomicU64 = AtomicU64::new(0);

/// Whether the engine should run for the mesh alone: this build runs mesh networks and a saved one is joined.
fn wanted() -> bool {
    mesh::supported()
        && dir()
            .and_then(|dir| mesh::load(&dir))
            .is_ok_and(|networks| networks.iter().any(|network| network.joined))
}

/// Brings the engine for the mesh alone in line with the rest, after a connection started or ended and after a
/// network was saved, joined, left or removed: it runs while the proxy is off and a network is joined, and stops
/// otherwise. A failure goes to the log; the page shows the network as not running.
pub fn settle(core: &Arc<Mutex<DesktopCore>>) {
    let Ok(mut guard) = core.lock() else {
        return;
    };
    let proxy = guard.is_running().unwrap_or(false);
    if proxy || !wanted() {
        if guard.stop_mesh_only() {
            log("mesh only: stopped");
        }
        return;
    }
    if guard.mesh_only_running() {
        return;
    }
    let started = dir()
        .and_then(|dir| mesh::load(&dir))
        .and_then(|networks| {
            mesh::only_profile(&mesh::outbounds(&networks))
                .ok_or_else(|| "no mesh network is joined".to_string())
        })
        .and_then(|profile| guard.start_mesh_only(profile.to_string().as_bytes()));
    drop(guard);
    match started {
        Ok(()) => {
            log("mesh only: started");
            watch_routes(core.clone());
        }
        Err(error) => log(&format!("mesh only: the engine did not start: {error}")),
    }
}

/// Puts the engine's mesh routes on the adapter every [`ROUTES_POLL`] while the engine for the mesh alone runs;
/// ends with it, or when a newer start began.
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
        Ok(networks) => mesh::offline_status(&networks),
        Err(error) => refusal(&error),
    }
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
    fn an_engine_that_does_not_run_answers_nothing() {
        let core = Mutex::new(DesktopCore::default());
        assert_eq!(
            engine_request(&core, json!({ "cmd": "mesh_leave", "network": "office" })),
            None
        );
    }
}
