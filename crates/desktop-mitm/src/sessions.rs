//! Saved capture sessions on Windows, Linux and Android (计划 F1/E11). The engine seals what capture holds into
//! files in a directory the client names, with a 32-byte key the client keeps (`set_capture_storage`, then
//! `save_capture`, `capture_sessions`, `load_capture`, `delete_capture`). This module builds those requests, keeps
//! the key sealed by the platform's key store, and deletes expired sessions while the engine is not running.
//!
//! The key never reaches the window: the backends send `set_capture_storage` themselves before every session
//! command, and refuse it when the window asks.

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

/// Whether the window may send `request` itself: everything but [`STORAGE_COMMAND`].
pub fn window_may_send(request: &Value) -> bool {
    request.get("cmd").and_then(Value::as_str) != Some(STORAGE_COMMAND)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
