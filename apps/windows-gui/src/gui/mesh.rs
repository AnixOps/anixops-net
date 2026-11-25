//! 组网 (mesh) in this window (docs/mesh-ui.md; docs/architecture/mesh-client.md): the saved networks, sealed for
//! this Windows account under the local application data folder (`client_core::backend::mesh`), and what the page
//! reads while no engine runs. The engine this window runs takes the joined networks with the profile
//! ([`with_joined`]) and joining, leaving, saving and removing at once ([`engine_request`]). The engine for the mesh
//! alone, with the proxy off, is not run on Windows yet: a network joined then is joined from the next connection.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    if !core.is_running().unwrap_or(false) {
        return None;
    }
    let answer = serde_json::to_vec(&request)
        .map_err(|error| error.to_string())
        .and_then(|bytes| core.control(&bytes))
        .and_then(|answer| serde_json::from_slice(&answer).map_err(|error| error.to_string()));
    Some(answer.unwrap_or_else(|error| refusal(&error)))
}

/// `mesh_status` while no engine runs: the saved networks as the page shows them offline
/// ([`mesh::offline_status`]) on a build whose engine runs mesh networks, else the engine's own refusal, which
/// hides the entry.
pub fn offline_status() -> Value {
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
