use platform_windows::managed::{
    windows_managed_data_directory, write_managed_text_atomic, WindowsProxySettings,
    WindowsProxySnapshot,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const DESKTOP_STATE_FILE: &str = "desktop-state.json";

/// How traffic is split. Public builds ship no preset list modes (docs/no-presets.md): a state saved by an
/// earlier version with one of the removed preset list modes loads as [`DesktopRoutingMode::Rule`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", from = "String")]
pub enum DesktopRoutingMode {
    /// The subscription's own rules and policy groups and the user's rules; a single-node profile sends
    /// everything through its node.
    #[default]
    Rule,
    Direct,
}

impl From<String> for DesktopRoutingMode {
    fn from(value: String) -> Self {
        if value == "direct" {
            Self::Direct
        } else {
            Self::Rule
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopProfileNode {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub outbound_tag: String,
    /// A domestic ingress on a shared trunk (the subscription's
    /// `#anixops:path=trunk` mark): picked by default and preferred by the
    /// fastest-node search (anx FUTURE-IMPROVEMENTS F-51).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trunk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopSubscriptionSource {
    pub id: String,
    pub location: String,
    #[serde(default)]
    pub last_attempt: Option<String>,
    #[serde(default)]
    pub last_successful_update: Option<String>,
    #[serde(default)]
    pub next_attempt: Option<String>,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub added_node_count: usize,
    #[serde(default)]
    pub removed_node_count: usize,
    #[serde(default)]
    pub changed_node_count: usize,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub last_update_error: Option<String>,
    /// The user's name for it, else the server's title; the UI falls back to the host.
    #[serde(default)]
    pub name: Option<String>,
    /// Traffic and expiry from the last successful download's `subscription-userinfo` header.
    #[serde(default)]
    pub user_info: Option<subscription_meta::UserInfo>,
    #[serde(default)]
    pub node_count: Option<usize>,
    /// Hours between automatic updates while this subscription is in use; 0 updates by hand only. Unset
    /// follows the server's `profile-update-interval`, else the default.
    #[serde(default)]
    pub update_interval_hours: Option<u32>,
    #[serde(default)]
    pub suggested_interval_hours: Option<u32>,
    /// The last successful update and the last attempt, Unix seconds. The automatic update, the time the
    /// interface shows and the time after a restart are all worked out from these
    /// (`subscription_meta::UpdateSchedule`); a source saved before they were kept updates once at the next
    /// check.
    #[serde(default)]
    pub last_success_unix: Option<u64>,
    #[serde(default)]
    pub last_attempt_unix: Option<u64>,
    /// Failed updates since the last successful one.
    #[serde(default)]
    pub failures: u32,
}

impl DesktopSubscriptionSource {
    /// The times its automatic updates follow.
    pub fn schedule(&self) -> subscription_meta::UpdateSchedule {
        subscription_meta::UpdateSchedule {
            last_success: self.last_success_unix,
            last_attempt: self.last_attempt_unix,
            failures: self.failures,
            interval_hours: self.effective_interval_hours(),
        }
    }

    pub fn effective_interval_hours(&self) -> u32 {
        self.update_interval_hours
            .or(self.suggested_interval_hours)
            .unwrap_or(subscription_meta::DEFAULT_UPDATE_INTERVAL_HOURS)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesktopState {
    // `runtime_mode` from versions with the Windows service mode is ignored when read.
    #[serde(default)]
    pub routing_mode: DesktopRoutingMode,
    /// A choice this version does not know (saved by a later one) reads as the default rather than making the
    /// whole state unreadable.
    #[serde(default, deserialize_with = "or_default")]
    pub dns_preset: desktop_dns::DnsPreset,
    /// Refuse UDP to port 443 so apps fall back from QUIC to TCP (the engine's `block_quic`).
    #[serde(default)]
    pub block_quic: bool,
    /// Debug mode's direct egress: the window's engine runs with no node.
    #[serde(default)]
    pub direct_egress: bool,
    /// Desktop mode also captures system traffic with a Wintun adapter.
    #[serde(default)]
    pub tun_mode_enabled: bool,
    pub proxy_snapshot: Option<WindowsProxySnapshot>,
    #[serde(default)]
    pub applied_proxy: Option<WindowsProxySettings>,
    pub certificate_sha1: Option<String>,
    pub driver_inf_path: Option<PathBuf>,
    #[serde(default)]
    pub profile_source_path: Option<PathBuf>,
    #[serde(default)]
    pub profile_source_url: Option<String>,
    #[serde(default)]
    pub profile_node_id: Option<String>,
    #[serde(default)]
    pub profile_node_catalog: Vec<DesktopProfileNode>,
    #[serde(default)]
    pub delay_test_url: Option<String>,
    #[serde(default)]
    pub debug_enabled: bool,
    #[serde(default)]
    pub dark_theme: bool,
    #[serde(default)]
    pub profile_last_successful_update: Option<String>,
    #[serde(default)]
    pub profile_last_attempt: Option<String>,
    #[serde(default)]
    pub profile_next_attempt: Option<String>,
    #[serde(default)]
    pub profile_refresh_result: String,
    #[serde(default)]
    pub profile_added_node_count: usize,
    #[serde(default)]
    pub profile_removed_node_count: usize,
    #[serde(default)]
    pub profile_changed_node_count: usize,
    #[serde(default)]
    pub profile_refresh_error_code: Option<String>,
    #[serde(default)]
    pub profile_last_update_error: Option<String>,
    /// The current user's "start after sign-in" switch, as their logon task stands. The file is shared by
    /// every user of the machine while the task is each user's own, so what each one chose is kept in
    /// `start_after_login_users`; this field only matters for a state saved before that list was kept.
    #[serde(default)]
    pub start_after_login: bool,
    /// The SIDs of the users who turned on "start after sign-in". The start-up check registers a user's
    /// logon task again when it is missing (an upgrade from a beta removed every user's task).
    #[serde(default)]
    pub start_after_login_users: Vec<String>,
    #[serde(default)]
    pub auto_connect: bool,
    #[serde(default)]
    pub auto_recover_core: bool,
    #[serde(default)]
    pub auto_subscription_refresh: bool,
    #[serde(default)]
    pub auto_select_fastest_node: bool,
    #[serde(default)]
    pub subscription_sources: Vec<DesktopSubscriptionSource>,
}

/// "我的规则": the user's own rule lines as written (`desktop_dns::rules`), next to the desktop state.
pub fn my_rules_path() -> PathBuf {
    windows_managed_data_directory().join("my-rules.txt")
}

/// Where the engine keeps the remote rule sets' last good downloads between runs (`routing.rule_set_cache_dir`):
/// a folder of its own inside the data folder, which is closed to everyone but the system and administrators.
pub fn rule_set_cache_dir() -> PathBuf {
    windows_managed_data_directory().join(desktop_dns::rules::RULE_SET_CACHE_DIR)
}

/// The "自定义" DNS text and the Fake-IP switch (`desktop_dns::custom`), next to the desktop state.
pub fn dns_settings_path() -> PathBuf {
    windows_managed_data_directory().join("dns-settings.json")
}

pub fn desktop_state_path() -> PathBuf {
    windows_managed_data_directory().join(DESKTOP_STATE_FILE)
}

/// A field's value, or the field's default when the value is not one this version reads.
fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// The saved state, strictly: an error when the file cannot be read. The uninstaller uses it, and leaves a
/// broken file alone.
pub fn load_desktop_state() -> Result<DesktopState, String> {
    load_desktop_state_at(&desktop_state_path())
}

fn load_desktop_state_at(path: &Path) -> Result<DesktopState, String> {
    if !path.exists() {
        return Ok(DesktopState::default());
    }
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "desktop state could not be read from {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("desktop state is invalid at {}: {error}", path.display()))
}

/// The state the window starts with.
#[derive(Debug, Default)]
pub struct DesktopStateLoad {
    pub state: DesktopState,
    /// The saved file could not be read: why, and where it was moved. The window starts with the defaults and
    /// tells the user once.
    pub set_aside: Option<String>,
}

/// The saved state, or the defaults when the file cannot be read (cut short, damaged, or written in a form
/// this version does not read): the file is then moved aside to `desktop-state.json.broken-<unix seconds>`
/// for a look, rather than keeping the window from ever starting again. The proxy this window set is
/// recorded in a file of its own (`owned_proxy`), so it is still put back.
pub fn load_desktop_state_or_set_aside() -> DesktopStateLoad {
    load_desktop_state_or_set_aside_at(&desktop_state_path())
}

fn load_desktop_state_or_set_aside_at(path: &Path) -> DesktopStateLoad {
    match load_desktop_state_at(path) {
        Ok(state) => DesktopStateLoad {
            state,
            set_aside: None,
        },
        Err(error) => {
            let moved = match set_aside(path) {
                Ok(target) => format!("moved to {}", target.display()),
                Err(move_error) => format!("could not be moved: {move_error}"),
            };
            DesktopStateLoad {
                state: DesktopState::default(),
                set_aside: Some(format!("{error}; {moved}")),
            }
        }
    }
}

/// Moves a file that cannot be read out of the way (`<name>.broken-<unix seconds>`), keeping it for a look, and
/// returns where it went.
pub fn set_aside(path: &Path) -> io::Result<PathBuf> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".broken-{seconds}"));
    let target = path.with_file_name(name);
    fs::rename(path, &target)?;
    Ok(target)
}

/// Serializes the saves of this process: every save goes through the same temporary file, and two at once
/// could make one of them fail (and a connection be rolled back for it).
static SAVE_LOCK: Mutex<()> = Mutex::new(());

pub fn save_desktop_state(state: &DesktopState) -> Result<(), String> {
    save_desktop_state_at(&desktop_state_path(), state)
}

fn save_desktop_state_at(path: &Path, state: &DesktopState) -> Result<(), String> {
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("desktop state could not be serialized: {error}"))?;
    let _guard = SAVE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_managed_text_atomic(path, &content).map_err(|error| error.to_string())
}

/// Whether the user with `sid` asked for the window to start after they sign in. A state saved before the
/// choice was kept per user has only the shared switch: the first user to start the window after the upgrade
/// takes it over, which is why `state` may change.
pub fn start_after_login_wanted(state: &mut DesktopState, sid: &str) -> bool {
    if state.start_after_login_users.is_empty() && state.start_after_login {
        state.start_after_login_users.push(sid.to_string());
    }
    state
        .start_after_login_users
        .iter()
        .any(|user| user.eq_ignore_ascii_case(sid))
}

/// Records the choice of the user with `sid` (`None` when the account is not known: only the switch).
pub fn set_start_after_login(state: &mut DesktopState, sid: Option<&str>, wanted: bool) {
    if let Some(sid) = sid {
        state
            .start_after_login_users
            .retain(|user| !user.eq_ignore_ascii_case(sid));
        if wanted {
            state.start_after_login_users.push(sid.to_string());
        }
    }
    state.start_after_login = wanted;
}

/// What the start-up check does about the current user's logon task, from what the user chose and whether
/// the task is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupRepair {
    /// Nothing to do.
    Keep,
    /// The user chose it but the task is gone: register it again.
    Register,
}

pub fn startup_repair(wanted: bool, task_present: bool) -> StartupRepair {
    if wanted && !task_present {
        StartupRepair::Register
    } else {
        StartupRepair::Keep
    }
}

pub fn owns_current_proxy(state: &DesktopState, current: &WindowsProxySettings) -> bool {
    state.proxy_snapshot.is_some()
        && state.applied_proxy.as_ref().is_some_and(|expected| {
            current.enabled == expected.enabled
                && current.server == expected.server
                && current.bypass == expected.bypass
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "S-1-5-21-1-2-3-1001";
    const BOB: &str = "S-1-5-21-1-2-3-1002";

    #[test]
    fn a_beta_state_gives_start_after_sign_in_to_the_first_user_after_the_upgrade() {
        let mut state: DesktopState =
            serde_json::from_str(r#"{"start_after_login":true}"#).unwrap();
        assert!(state.start_after_login_users.is_empty());
        assert!(start_after_login_wanted(&mut state, ALICE));
        assert_eq!(state.start_after_login_users, [ALICE]);
        // Another user of the same machine never chose it.
        assert!(!start_after_login_wanted(&mut state, BOB));
        assert!(start_after_login_wanted(
            &mut state,
            &ALICE.to_ascii_lowercase()
        ));
    }

    #[test]
    fn each_user_keeps_their_own_start_after_sign_in_choice() {
        let mut state = DesktopState::default();
        assert!(!start_after_login_wanted(&mut state, ALICE));
        set_start_after_login(&mut state, Some(ALICE), true);
        set_start_after_login(&mut state, Some(BOB), true);
        set_start_after_login(&mut state, Some(BOB), false);
        assert!(start_after_login_wanted(&mut state, ALICE));
        assert!(!start_after_login_wanted(&mut state, BOB));
        set_start_after_login(&mut state, Some(ALICE), true);
        assert_eq!(state.start_after_login_users, [ALICE]);
        set_start_after_login(&mut state, Some(ALICE), false);
        // Nobody wants it: an old switch left on does not hand it to the next user.
        assert!(state.start_after_login_users.is_empty());
        assert!(!state.start_after_login);
        assert!(!start_after_login_wanted(&mut state, BOB));
    }

    #[test]
    fn a_missing_logon_task_is_registered_again_only_when_chosen() {
        assert_eq!(startup_repair(true, false), StartupRepair::Register);
        assert_eq!(startup_repair(true, true), StartupRepair::Keep);
        assert_eq!(startup_repair(false, false), StartupRepair::Keep);
        // A task the user has is theirs: the check never removes one.
        assert_eq!(startup_repair(false, true), StartupRepair::Keep);
    }

    #[test]
    fn old_state_defaults_daily_lifecycle_preferences() {
        let state: DesktopState = serde_json::from_str(r#"{"debug_enabled":true}"#)
            .expect("older desktop state should remain readable");
        assert!(state.debug_enabled);
        assert!(!state.start_after_login);
        assert!(!state.auto_connect);
        assert!(!state.auto_recover_core);
        assert!(!state.auto_subscription_refresh);
        assert!(!state.auto_select_fastest_node);
        assert_eq!(state.routing_mode, DesktopRoutingMode::Rule);
        assert!(state.applied_proxy.is_none());
        assert!(state.profile_node_catalog.is_empty());
    }

    #[test]
    fn preset_list_modes_saved_by_earlier_versions_load_as_rule_mode() {
        let saved = ["bypass_china", "gfw_list", "return_china", "rule"]; // neutral-terms: allow (values saved by earlier versions)
        for mode in saved {
            let state: DesktopState = serde_json::from_str(&format!(
                r#"{{"routing_mode":"{mode}","bilibili_web_ad_block_enabled":true}}"#
            ))
            .unwrap();
            assert_eq!(state.routing_mode, DesktopRoutingMode::Rule, "{mode}");
        }
        let direct: DesktopState = serde_json::from_str(r#"{"routing_mode":"direct"}"#).unwrap();
        assert_eq!(direct.routing_mode, DesktopRoutingMode::Direct);
        assert_eq!(
            serde_json::to_value(DesktopRoutingMode::Rule).unwrap(),
            serde_json::json!("rule")
        );
    }

    #[test]
    fn a_state_from_the_service_mode_era_still_loads() {
        let state: DesktopState =
            serde_json::from_str(r#"{"runtime_mode":"service","auto_connect":true}"#).unwrap();
        assert!(state.auto_connect);
    }

    #[test]
    fn desktop_state_round_trips_the_node_catalog() {
        let state = DesktopState {
            profile_node_catalog: vec![DesktopProfileNode {
                id: "primary".to_string(),
                label: "Primary [primary] (Shadowsocks)".to_string(),
                protocol: "Shadowsocks".to_string(),
                outbound_tag: "primary".to_string(),
                trunk: true,
            }],
            ..DesktopState::default()
        };

        let decoded: DesktopState = serde_json::from_str(
            &serde_json::to_string(&state).expect("desktop state should serialize"),
        )
        .expect("desktop state should deserialize");

        assert_eq!(decoded.profile_node_catalog, state.profile_node_catalog);
    }

    #[test]
    fn legacy_subscription_source_defaults_refresh_status_fields() {
        let source: DesktopSubscriptionSource =
            serde_json::from_str(r#"{"id":"source-1","location":"https://example.invalid/sub"}"#)
                .expect("older saved source should remain readable");

        assert!(source.last_attempt.is_none());
        assert!(source.next_attempt.is_none());
        assert_eq!(source.result, "");
        assert_eq!(source.added_node_count, 0);
        assert!(source.error_code.is_none());
    }

    fn scratch_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "anixops-desktop-state-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn set_aside_files(directory: &Path) -> usize {
        fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("desktop-state.json.broken-")
            })
            .count()
    }

    #[test]
    fn an_unreadable_desktop_state_is_set_aside_and_the_defaults_used() {
        let directory = scratch_directory("broken");
        let path = directory.join(DESKTOP_STATE_FILE);
        fs::write(
            &path,
            br#"{"auto_connect":true,"subscription_sources":[{"id":"#,
        )
        .unwrap();
        // The strict reader, which the uninstaller uses, still refuses it.
        assert!(load_desktop_state_at(&path).is_err());
        let loaded = load_desktop_state_or_set_aside_at(&path);
        assert!(loaded.set_aside.is_some());
        assert!(!loaded.state.auto_connect);
        assert!(!path.exists());
        assert_eq!(set_aside_files(&directory), 1);
        // The next start finds no file and starts with the defaults without a warning.
        let next = load_desktop_state_or_set_aside_at(&path);
        assert!(next.set_aside.is_none());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_dns_choice_of_a_later_version_reads_as_the_default() {
        let state: DesktopState =
            serde_json::from_str(r#"{"dns_preset":"quantum","auto_connect":true}"#)
                .expect("an unknown DNS choice should not make the state unreadable");
        assert_eq!(state.dns_preset, desktop_dns::DnsPreset::default());
        assert!(state.auto_connect);
        let state: DesktopState = serde_json::from_str(r#"{"dns_preset":"encrypted"}"#).unwrap();
        assert_eq!(state.dns_preset, desktop_dns::DnsPreset::Encrypted);
        let directory = scratch_directory("unknown");
        let path = directory.join(DESKTOP_STATE_FILE);
        fs::write(&path, br#"{"dns_preset":"quantum","auto_connect":true}"#).unwrap();
        let loaded = load_desktop_state_or_set_aside_at(&path);
        assert!(loaded.set_aside.is_none());
        assert!(loaded.state.auto_connect);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn saves_from_several_threads_do_not_fail_each_other() {
        let directory = scratch_directory("saves");
        let path = directory.join(DESKTOP_STATE_FILE);
        let threads: Vec<_> = (0..8)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    (0..20).all(|_| {
                        save_desktop_state_at(
                            &path,
                            &DesktopState {
                                auto_connect: index % 2 == 0,
                                ..DesktopState::default()
                            },
                        )
                        .is_ok()
                    })
                })
            })
            .collect();
        for thread in threads {
            assert!(thread.join().unwrap());
        }
        assert!(load_desktop_state_at(&path).is_ok());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn proxy_recovery_requires_the_owned_applied_settings() {
        let state = DesktopState {
            proxy_snapshot: Some(WindowsProxySnapshot {
                enabled: false,
                server: String::new(),
                bypass: String::new(),
                winhttp_access_type: 1,
                winhttp_server: String::new(),
                winhttp_bypass: String::new(),
                winhttp_untouched: false,
                applied: None,
                connections: Vec::new(),
            }),
            applied_proxy: Some(WindowsProxySettings {
                enabled: true,
                server: "127.0.0.1:7890".to_string(),
                bypass: "<local>".to_string(),
            }),
            ..DesktopState::default()
        };
        assert!(owns_current_proxy(
            &state,
            &WindowsProxySettings {
                enabled: true,
                server: "127.0.0.1:7890".to_string(),
                bypass: "<local>".to_string(),
            }
        ));
        assert!(!owns_current_proxy(
            &state,
            &WindowsProxySettings {
                enabled: true,
                server: "proxy.example:8080".to_string(),
                bypass: "<local>".to_string(),
            }
        ));
    }
}
