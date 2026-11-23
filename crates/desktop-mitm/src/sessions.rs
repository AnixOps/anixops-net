//! Saved capture sessions on Windows, Linux and Android (计划 F1/E11). The engine seals what capture holds into
//! files in a directory the client names, with a 32-byte key the client keeps (`set_capture_storage`, then
//! `save_capture`, `capture_sessions`, `load_capture`, `delete_capture`). This module builds those requests, keeps
//! the key sealed by the platform's key store, and deletes expired sessions while the engine is not running.
//!
//! The key never reaches the window: the backends send `set_capture_storage` themselves before every session
//! command, and refuse it when the window asks.
//!
//! With the engine stopped the same commands go to the kernel's capture viewer ([`viewer_control`]): the session
//! saved at the stop is opened there ([`open_in_viewer`]), so what was captured stays on screen and can be
//! exported after a disconnect.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use serde_json::{json, Value};

/// Days a saved session is kept (the engine's default; the plan's 7-day cleanup).
pub const RETENTION_DAYS: u32 = 7;
/// Length of the session key the engine expects.
pub const KEY_LEN: usize = 32;
/// The engine's file extension for a session.
const EXTENSION: &str = "anxcap";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Session names are cut to this many characters, as the engine does.
const MAX_NAME_CHARS: usize = 120;

/// The command that names the directory and the key; the window must never send it.
pub const STORAGE_COMMAND: &str = "set_capture_storage";

/// What keeps the session key at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protection {
    /// Windows Data Protection (DPAPI), bound to the signed-in user.
    Dpapi,
    /// The Android Keystore.
    Keystore,
    /// The desktop's Secret Service (GNOME Keyring, KWallet) on Linux.
    SecretService,
    /// A file only this user can read; Linux without a Secret Service.
    File,
}

/// The key and what keeps it.
#[derive(Clone)]
pub struct SessionKey {
    key: [u8; KEY_LEN],
    pub protection: Protection,
}

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key never reaches a log line.
        f.debug_struct("SessionKey")
            .field("protection", &self.protection)
            .finish_non_exhaustive()
    }
}

impl SessionKey {
    pub fn new(key: &[u8], protection: Protection) -> Result<Self, String> {
        let key: [u8; KEY_LEN] = key
            .try_into()
            .map_err(|_| "the capture session key has the wrong length".to_string())?;
        Ok(Self { key, protection })
    }
}

/// A new random key.
pub fn new_key() -> Result<[u8; KEY_LEN], String> {
    let mut key = [0u8; KEY_LEN];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| "a capture session key could not be generated".to_string())?;
    Ok(key)
}

/// The engine's `set_capture_storage` request.
pub fn storage_request(dir: &Path, key: &SessionKey) -> Value {
    json!({
        "cmd": STORAGE_COMMAND,
        "dir": dir.display().to_string(),
        "key_base64": crate::capture::encode_base64(&key.key),
        "retention_days": RETENTION_DAYS,
    })
}

/// One session command from the window, checked: `list`, `save` (optional `name`), `load` and `delete` (`session`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SessionAction {
    List,
    Save {
        #[serde(default)]
        name: Option<String>,
    },
    Load {
        session: String,
    },
    Delete {
        session: String,
    },
}

impl SessionAction {
    /// The engine request, or why the action is refused.
    pub fn request(&self) -> Result<Value, String> {
        Ok(match self {
            Self::List => json!({ "cmd": "capture_sessions" }),
            Self::Save { name } => {
                let name: String = name
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(MAX_NAME_CHARS)
                    .collect();
                if name.is_empty() {
                    json!({ "cmd": "save_capture" })
                } else {
                    json!({ "cmd": "save_capture", "name": name })
                }
            }
            Self::Load { session } => {
                json!({ "cmd": "load_capture", "session": checked(session)? })
            }
            Self::Delete { session } => {
                json!({ "cmd": "delete_capture", "session": checked(session)? })
            }
        })
    }
}

fn checked(session: &str) -> Result<&str, String> {
    if is_session_id(session) {
        Ok(session)
    } else {
        Err("no such capture session".to_string())
    }
}

/// A session id is `<saved_at_ms>-<hex>`, as the engine names its files; nothing else can name a path.
pub fn is_session_id(id: &str) -> bool {
    saved_at(id).is_some()
}

fn saved_at(id: &str) -> Option<u64> {
    let (millis, tag) = id.split_once('-')?;
    if millis.is_empty()
        || tag.is_empty()
        || !millis.bytes().all(|byte| byte.is_ascii_digit())
        || !tag.bytes().all(|byte| byte.is_ascii_hexdigit())
        || millis.len() > 20
        || tag.len() > 32
    {
        return None;
    }
    millis.parse().ok()
}

/// Deletes sessions in `dir` saved more than [`RETENTION_DAYS`] ago, by the time in their names, as the engine
/// does whenever it is told where sessions are. The clients call this at start so sessions expire even when the
/// engine does not run. Returns how many went.
pub fn purge(dir: &Path, now_ms: u64) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let retention = u64::from(RETENTION_DAYS) * DAY_MS;
    let mut purged = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(saved) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(&format!(".{EXTENSION}")))
            .and_then(saved_at)
        else {
            continue;
        };
        if now_ms.saturating_sub(saved) > retention && fs::remove_file(&path).is_ok() {
            purged += 1;
        }
    }
    purged
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The key sealed by the platform (`seal` / `open`, e.g. DPAPI) in the file at `path`, made on first use. A file
/// that is there but cannot be opened is an error, never replaced: a new key would make every saved session
/// unreadable.
pub fn load_or_create_sealed(
    path: &Path,
    seal: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
    open: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<[u8; KEY_LEN], String> {
    match fs::read(path) {
        Ok(sealed) => {
            let key = open(&sealed)?;
            key.as_slice()
                .try_into()
                .map_err(|_| "the capture session key is damaged".to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key = new_key()?;
            write_private(path, &seal(&key)?)?;
            Ok(key)
        }
        Err(error) => Err(format!(
            "the capture session key could not be read: {error}"
        )),
    }
}

/// Writes `data` to `path` readable only by this user (0600 on Unix), replacing it in one step.
pub fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or("the capture session key has no folder")?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary: PathBuf = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("key")
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The engine's answer to a session command, with what the window shows about the key.
pub fn with_protection(mut answer: Value, protection: Protection) -> Value {
    if let Value::Object(map) = &mut answer {
        map.insert("protection".to_string(), json!(protection));
        map.insert("retention_days".to_string(), json!(RETENTION_DAYS));
    }
    answer
}

/// The capture budget the desktop windows (Windows, Linux) ask for when they turn capture on: the engine's
/// largest. The engine's own default is sized for the iOS Network Extension's memory limit; a desktop has the
/// room, and with the small default ordinary traffic pushes decrypted requests out within seconds.
pub const DESKTOP_CAPTURE_BUDGET: u64 = 16 * 1024 * 1024;

/// Why the client saved what capture held without being asked: the engine is about to stop, and capture only
/// lives as long as the engine does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoSave {
    /// A changed setting restarts the engine (decryption hosts, mappings, plugins, TUN mode, ...).
    Reconnect,
    /// The user disconnects or quits.
    Disconnect,
}

impl AutoSave {
    /// The session's name in the window's language (`zh`, `zh-Hant` or `en`; anything else reads Simplified
    /// Chinese, as the notifications do). "Saved on disconnect" is the name the iOS and macOS app uses.
    pub fn name(self, language: &str) -> &'static str {
        match (self, language) {
            (Self::Reconnect, "en") => "Saved before reconnecting",
            (Self::Reconnect, "zh-Hant") => "重新連線前自動儲存",
            (Self::Reconnect, _) => "重连前自动保存",
            (Self::Disconnect, "en") => "Saved on disconnect",
            (Self::Disconnect, "zh-Hant") => "斷開時自動儲存",
            (Self::Disconnect, _) => "断开时自动保存",
        }
    }
}

/// What capture was doing when the engine stopped, so the next engine can carry on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureCarry {
    /// Live capture was on: the next engine turns it on again.
    pub was_on: bool,
    /// The budget it ran with, given again so a restart does not fall back to the engine's default.
    pub max_total_bytes: Option<u64>,
    /// The id of the session it was saved as, when it held anything.
    pub saved: Option<String>,
    /// Why it could not be saved, when saving was needed and failed. Never stops the engine from stopping.
    pub save_error: Option<String>,
}

impl CaptureCarry {
    /// What the user is told about capture across the stop, in the window's language (as [`AutoSave::name`]),
    /// or `None` when there was nothing to save.
    pub fn note(&self, reason: AutoSave, language: &str) -> Option<String> {
        if let Some(error) = self.save_error.as_deref() {
            return Some(match language {
                "en" => format!("The capture could not be saved: {error}."),
                "zh-Hant" => format!("抓包未能儲存：{error}。"),
                _ => format!("抓包未能保存：{error}。"),
            });
        }
        let name = reason.name(language);
        self.saved.as_ref().map(|_| match language {
            "en" => format!("The capture so far was saved as “{name}”."),
            "zh-Hant" => format!("目前的抓包已儲存為「{name}」。"),
            _ => format!("当前抓包已保存为“{name}”。"),
        })
    }

    /// The `set_capture` request that turns capture on again in the next engine, when it was on.
    pub fn resume_request(&self) -> Option<Value> {
        if !self.was_on {
            return None;
        }
        Some(match self.max_total_bytes {
            Some(budget) => {
                json!({ "cmd": "set_capture", "enabled": true, "max_total_bytes": budget })
            }
            None => json!({ "cmd": "set_capture", "enabled": true }),
        })
    }
}

/// Saves what live capture holds before the engine stops, as the iOS and macOS app does on disconnect: asks the
/// engine for the capture status and, only while live capture is on and holds connections, names the session
/// directory with `key` (read only then) and sends `save_capture` with `name`. A session loaded for viewing
/// turns live capture off, so it is never saved again. Nothing here fails: an engine that does not answer or a
/// key that cannot be read leaves `saved` empty (with `save_error` when saving was due), and the caller stops
/// the engine all the same.
pub fn preserve<C, K>(mut control: C, dir: &Path, key: K, name: &str) -> CaptureCarry
where
    C: FnMut(&Value) -> Result<Value, String>,
    K: FnOnce() -> Result<SessionKey, String>,
{
    let Ok(status) = control(&json!({ "cmd": "capture" })) else {
        return CaptureCarry::default();
    };
    let was_on = status.get("enabled").and_then(Value::as_bool) == Some(true);
    let mut carry = CaptureCarry {
        was_on,
        max_total_bytes: status.get("max_total_bytes").and_then(Value::as_u64),
        saved: None,
        save_error: None,
    };
    let held = status
        .get("connections")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if !was_on || held == 0 {
        return carry;
    }
    let saved = key().and_then(|key| {
        control(&storage_request(dir, &key))?;
        let request = SessionAction::Save {
            name: Some(name.to_string()),
        }
        .request()?;
        control(&request)
    });
    match saved {
        Ok(answer) => {
            carry.saved = answer
                .pointer("/session/session")
                .and_then(Value::as_str)
                .map(str::to_string);
            if carry.saved.is_none() {
                carry.save_error = Some(
                    answer
                        .get("error")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "the engine did not save the capture".to_string()),
                );
            }
        }
        Err(error) => carry.save_error = Some(error),
    }
    carry
}

/// Whether the window may send `request` itself: everything but [`STORAGE_COMMAND`].
pub fn window_may_send(request: &Value) -> bool {
    request.get("cmd").and_then(Value::as_str) != Some(STORAGE_COMMAND)
}

/// The commands the kernel's capture viewer answers while no engine runs (`engine_embedded::capture_viewer`):
/// the saved sessions are listed, opened, read, exported and deleted with the tunnel off, and `connections` lists
/// the connections of the session that is open. Turning capture on and saving need an engine.
const VIEWER_COMMANDS: &[&str] = &[
    "capture",
    "set_capture",
    "clear_capture",
    "capture_requests",
    "capture_connection",
    "capture_exchange",
    "capture_h2_frames",
    "export_capture",
    STORAGE_COMMAND,
    "capture_sessions",
    "load_capture",
    "delete_capture",
    "connections",
];

/// Answers `request` with the kernel's capture viewer, for a backend whose engine is not running; `None` when the
/// viewer does not take the command (the caller then says the engine is not running, as before).
pub fn viewer_control(request: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let command = serde_json::from_slice::<Value>(request).ok()?;
    let command = command.get("cmd").and_then(Value::as_str)?;
    if !VIEWER_COMMANDS.contains(&command) {
        return None;
    }
    Some(
        engine_embedded::capture_viewer::control(request)
            .map_err(|error| error.message.to_string()),
    )
}

/// Opens session `session` in the capture viewer: the engine just stopped and saved it
/// ([`preserve`]), so the request and connection lists keep showing what was captured instead of going empty.
pub fn open_in_viewer(dir: &Path, key: &SessionKey, session: &str) -> Result<(), String> {
    let send = |request: Value| {
        let request = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        engine_embedded::capture_viewer::control(&request)
            .map(|_| ())
            .map_err(|error| error.message.to_string())
    };
    send(storage_request(dir, key))?;
    send(
        SessionAction::Load {
            session: session.to_string(),
        }
        .request()?,
    )
}

/// Empties the capture viewer when an engine starts: the lists read the engine again, and the session the viewer
/// held is not kept in memory beside it.
pub fn close_viewer() {
    let _ = engine_embedded::capture_viewer::control(br#"{"cmd":"clear_capture"}"#);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_viewer_opens_a_saved_session_with_no_engine() {
        let dir = std::env::temp_dir().join(format!("desktop-mitm-viewer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let key = SessionKey::new(&[7u8; KEY_LEN], Protection::File).unwrap();
        // Commands that need an engine are left to the caller.
        assert!(viewer_control(br#"{"cmd":"reload_rules","rules":[]}"#).is_none());
        assert!(viewer_control(br#"{"cmd":"traffic"}"#).is_none());
        assert!(viewer_control(b"not json").is_none());
        // Saving needs an engine: it is left to the caller too.
        let storage = serde_json::to_vec(&storage_request(&dir, &key)).unwrap();
        viewer_control(&storage).unwrap().unwrap();
        assert!(viewer_control(br#"{"cmd":"save_capture"}"#).is_none());
        let list = viewer_control(br#"{"cmd":"capture_sessions"}"#)
            .unwrap()
            .unwrap();
        let list: Value = serde_json::from_slice(&list).unwrap();
        assert_eq!(list["sessions"], json!([]), "{list}");
        // A session that is not there is an error, not a panic.
        assert!(open_in_viewer(&dir, &key, "1700000000000-a1b2c3d4").is_err());
        close_viewer();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_ids_name_no_path() {
        assert!(is_session_id("1700000000000-a1b2c3d4"));
        assert!(!is_session_id("../1700000000000-a1b2"));
        assert!(!is_session_id("1700000000000-"));
        assert!(!is_session_id("-a1b2"));
        assert!(!is_session_id("1700000000000-zz"));
        assert!(!is_session_id("17/00-a1"));
        assert!(SessionAction::Load {
            session: "../../etc/passwd".to_string()
        }
        .request()
        .is_err());
    }

    #[test]
    fn requests_match_the_engine() {
        assert_eq!(
            SessionAction::List.request().unwrap(),
            json!({ "cmd": "capture_sessions" })
        );
        assert_eq!(
            SessionAction::Save {
                name: Some("  login bug \n".to_string())
            }
            .request()
            .unwrap(),
            json!({ "cmd": "save_capture", "name": "login bug" })
        );
        assert_eq!(
            SessionAction::Save {
                name: Some("   ".to_string())
            }
            .request()
            .unwrap(),
            json!({ "cmd": "save_capture" })
        );
        assert_eq!(
            SessionAction::Delete {
                session: "1-ab".to_string()
            }
            .request()
            .unwrap(),
            json!({ "cmd": "delete_capture", "session": "1-ab" })
        );
        let action: SessionAction =
            serde_json::from_value(json!({ "action": "load", "session": "5-0f" })).unwrap();
        assert_eq!(
            action.request().unwrap(),
            json!({ "cmd": "load_capture", "session": "5-0f" })
        );
    }

    #[test]
    fn storage_request_carries_the_key_and_window_cannot_send_it() {
        let key = SessionKey::new(&[7u8; KEY_LEN], Protection::File).unwrap();
        let request = storage_request(Path::new("/tmp/sessions"), &key);
        assert_eq!(request["cmd"], STORAGE_COMMAND);
        assert_eq!(request["retention_days"], RETENTION_DAYS);
        assert_eq!(
            crate::capture::decode_base64(request["key_base64"].as_str().unwrap()).unwrap(),
            vec![7u8; KEY_LEN]
        );
        assert!(!window_may_send(&request));
        assert!(window_may_send(&json!({ "cmd": "capture" })));
        assert!(!format!("{key:?}").contains('7'));
        assert!(SessionKey::new(&[1u8; 16], Protection::File).is_err());
    }

    #[test]
    fn purge_deletes_only_expired_sessions() {
        let dir = std::env::temp_dir().join(format!("anixops-sessions-{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let now = 100 * DAY_MS;
        let old = dir.join(format!("{}-ab.anxcap", now - 8 * DAY_MS));
        let fresh = dir.join(format!("{}-cd.anxcap", now - DAY_MS));
        let other = dir.join("notes.txt");
        for path in [&old, &fresh, &other] {
            fs::write(path, b"x").unwrap();
        }
        assert_eq!(purge(&dir, now), 1);
        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(other.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sealed_key_is_made_once_and_never_replaced() {
        let dir = std::env::temp_dir().join(format!("anixops-session-key-{}", now_ms()));
        let path = dir.join("session-key");
        fn flip(data: &[u8]) -> Result<Vec<u8>, String> {
            Ok(data.iter().map(|byte| byte ^ 0x5a).collect())
        }
        fn refuse(_: &[u8]) -> Result<Vec<u8>, String> {
            Err("locked".to_string())
        }
        let (seal, open) = (flip, flip);
        let first = load_or_create_sealed(&path, seal, open).unwrap();
        let second = load_or_create_sealed(&path, seal, open).unwrap();
        assert_eq!(first, second);
        assert!(load_or_create_sealed(&path, seal, refuse).is_err());
        assert_eq!(fs::read(&path).unwrap().len(), KEY_LEN);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn answers_say_what_keeps_the_key() {
        let answer = with_protection(json!({ "sessions": [] }), Protection::Dpapi);
        assert_eq!(answer["protection"], "dpapi");
        assert_eq!(answer["retention_days"], RETENTION_DAYS);
        assert_eq!(
            serde_json::to_value(Protection::SecretService).unwrap(),
            "secret-service"
        );
    }

    /// A fake engine for [`preserve`]: answers `capture` with `status` and records every request.
    fn engine(status: Value) -> (Vec<Value>, impl FnMut(&Value) -> Result<Value, String>) {
        let answer = move |request: &Value| {
            Ok(match request["cmd"].as_str() {
                Some("capture") => status.clone(),
                Some("save_capture") => {
                    json!({ "session": { "session": "1700000000000-ab", "name": request["name"] } })
                }
                _ => json!({}),
            })
        };
        (Vec::new(), answer)
    }

    fn key() -> Result<SessionKey, String> {
        SessionKey::new(&[3u8; KEY_LEN], Protection::Dpapi)
    }

    #[test]
    fn live_capture_is_saved_before_the_engine_stops_and_resumed_with_its_budget() {
        let (mut sent, mut answer) = engine(json!({
            "enabled": true, "max_total_bytes": DESKTOP_CAPTURE_BUDGET, "used_bytes": 900, "connections": 4
        }));
        let carry = preserve(
            |request: &Value| {
                sent.push(request.clone());
                answer(request)
            },
            Path::new("/tmp/sessions"),
            key,
            AutoSave::Reconnect.name("zh"),
        );
        let commands: Vec<&str> = sent.iter().filter_map(|r| r["cmd"].as_str()).collect();
        assert_eq!(commands, ["capture", STORAGE_COMMAND, "save_capture"]);
        assert_eq!(sent[2]["name"], "重连前自动保存");
        assert_eq!(carry.saved.as_deref(), Some("1700000000000-ab"));
        assert_eq!(carry.save_error, None);
        assert_eq!(
            carry.resume_request(),
            Some(
                json!({ "cmd": "set_capture", "enabled": true, "max_total_bytes": DESKTOP_CAPTURE_BUDGET })
            )
        );
    }

    #[test]
    fn nothing_is_saved_while_capture_is_off_or_empty() {
        for status in [
            json!({ "enabled": false, "max_total_bytes": 2097152, "used_bytes": 900, "connections": 4 }),
            json!({ "enabled": true, "max_total_bytes": 2097152, "used_bytes": 0, "connections": 0 }),
        ] {
            let (mut sent, mut answer) = engine(status.clone());
            let carry = preserve(
                |request: &Value| {
                    sent.push(request.clone());
                    answer(request)
                },
                Path::new("/tmp/sessions"),
                || -> Result<SessionKey, String> {
                    panic!("the key is read only when a save is due")
                },
                "x",
            );
            assert_eq!(sent.len(), 1, "{status}");
            assert_eq!(carry.saved, None);
            assert_eq!(carry.save_error, None);
            assert_eq!(carry.resume_request().is_some(), status["enabled"] == true);
        }
    }

    #[test]
    fn a_failed_save_is_reported_and_never_blocks() {
        let status = json!({ "enabled": true, "max_total_bytes": 2097152, "used_bytes": 9, "connections": 1 });
        let carry = preserve(
            |request: &Value| match request["cmd"].as_str() {
                Some("capture") => Ok(status.clone()),
                _ => Err("disk full".to_string()),
            },
            Path::new("/tmp/sessions"),
            key,
            "x",
        );
        assert!(carry.was_on);
        assert_eq!(carry.save_error.as_deref(), Some("disk full"));
        let locked = preserve(
            |_: &Value| Ok(status.clone()),
            Path::new("/tmp/sessions"),
            || Err("locked".to_string()),
            "x",
        );
        assert_eq!(locked.save_error.as_deref(), Some("locked"));
        // No engine to ask: nothing to carry.
        let gone = preserve(
            |_: &Value| Err("not running".to_string()),
            Path::new("/tmp"),
            key,
            "x",
        );
        assert_eq!(gone, CaptureCarry::default());
        assert_eq!(gone.resume_request(), None);
    }

    #[test]
    fn notes_name_the_saved_session() {
        let saved = CaptureCarry {
            was_on: true,
            max_total_bytes: None,
            saved: Some("1-ab".to_string()),
            save_error: None,
        };
        assert_eq!(
            saved.note(AutoSave::Reconnect, "zh").as_deref(),
            Some("当前抓包已保存为“重连前自动保存”。")
        );
        assert_eq!(
            saved.note(AutoSave::Disconnect, "en").as_deref(),
            Some("The capture so far was saved as “Saved on disconnect”.")
        );
        let failed = CaptureCarry {
            save_error: Some("locked".to_string()),
            ..CaptureCarry::default()
        };
        assert_eq!(
            failed.note(AutoSave::Reconnect, "zh-Hant").as_deref(),
            Some("抓包未能儲存：locked。")
        );
        assert!(CaptureCarry::default()
            .note(AutoSave::Reconnect, "en")
            .is_none());
    }

    #[test]
    fn auto_saved_sessions_are_named_in_the_window_language() {
        assert_eq!(AutoSave::Disconnect.name("en"), "Saved on disconnect");
        assert_eq!(AutoSave::Disconnect.name("zh-Hant"), "斷開時自動儲存");
        assert_eq!(AutoSave::Disconnect.name(""), "断开时自动保存");
        assert_eq!(AutoSave::Reconnect.name("en"), "Saved before reconnecting");
        assert_eq!(AutoSave::Reconnect.name("zh-Hant"), "重新連線前自動儲存");
    }
}
