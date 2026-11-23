//! The system proxy this window set, recorded so that it is put back even when the window never gets to do it
//! (a crash, the process killed, a sign-out or shutdown while connected, the client removed).
//!
//! Connecting writes a record to `owned-proxy.json` in the data folder *before* the proxy changes: the account
//! whose settings change, the process that owns them (its id and creation time, so a later process given the
//! same id is not taken for it), the settings applied and the settings to put back (the user's WinINet settings
//! and the machine's WinHTTP proxy). Disconnecting removes it. A record whose process is gone is an orphan:
//!
//! - the window's next start ([`recover_at_start`], also when the sign-in task starts it) puts this user's
//!   settings back when they are still the ones the record applied, and otherwise only forgets the record, so a
//!   proxy the user changed since is never overwritten;
//! - the window's session-end guard (`session_end`) puts them back when Windows signs out, shuts down or asks
//!   the window to close for an installer;
//! - the uninstaller's `--purge`, which runs as LocalSystem, puts back the WinHTTP proxy and the WinINet
//!   settings of every recorded user who is signed in ([`restore_for_removal`]).
//!
//! The record is a file of its own rather than part of `desktop-state.json`, so a desktop state that cannot be
//! read (and is set aside) does not lose it. The desktop state keeps its own copy of the snapshot
//! (`proxy_snapshot` / `applied_proxy`) for the settings page; an earlier version only had that copy, and
//! [`recover_at_start`] uses it when there is no record.

use super::startup::{owns_current_proxy, set_aside, DesktopState};
use platform_windows::managed::{
    windows_managed_data_directory, write_managed_text_atomic, WindowsProxySettings,
    WindowsProxySnapshot,
};
use platform_windows::system_integration::WindowsWinHttpProxy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const OWNED_PROXY_FILE: &str = "owned-proxy.json";

/// `WINHTTP_ACCESS_TYPE_NO_PROXY` and `WINHTTP_ACCESS_TYPE_NAMED_PROXY` (winhttp.h).
pub const WINHTTP_NO_PROXY: u32 = 1;
pub const WINHTTP_NAMED_PROXY: u32 = 3;

/// Serializes every read-modify-write of the record file in this process.
static RECORDS_LOCK: Mutex<()> = Mutex::new(());

/// The process that set the proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyOwner {
    pub pid: u32,
    /// The process's creation time (`FILETIME` ticks); 0 when it could not be read.
    #[serde(default)]
    pub started: u64,
}

/// One user's proxy, set by the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedProxy {
    /// The SID of the account whose WinINet settings were changed; `None` when it could not be read.
    #[serde(default)]
    pub user: Option<String>,
    pub owner: ProxyOwner,
    /// What the window set.
    pub applied: WindowsProxySettings,
    /// What to put back: the user's WinINet settings and the machine's WinHTTP proxy before connecting.
    pub snapshot: WindowsProxySnapshot,
}

/// The file: at most one record per user.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedProxies {
    #[serde(default)]
    pub records: Vec<OwnedProxy>,
}

impl OwnedProxies {
    /// Puts `record` in place of the same user's earlier one.
    pub fn put(&mut self, record: OwnedProxy) {
        self.records
            .retain(|known| !same_user(known.user.as_deref(), record.user.as_deref()));
        self.records.push(record);
    }

    /// Removes and returns `user`'s record.
    pub fn take_user(&mut self, user: Option<&str>) -> Option<OwnedProxy> {
        let index = self
            .records
            .iter()
            .position(|known| same_user(known.user.as_deref(), user))?;
        Some(self.records.remove(index))
    }
}

/// Whether a record written for `recorded` is the one of `user`. A record whose account could not be read
/// belongs to whoever starts the window next, the only one it can be put back for.
pub fn same_user(recorded: Option<&str>, user: Option<&str>) -> bool {
    match (recorded, user) {
        (None, _) => true,
        (Some(recorded), Some(user)) => recorded.eq_ignore_ascii_case(user),
        (Some(_), None) => false,
    }
}

/// Whether the user's WinINet settings are still exactly the ones the window applied.
pub fn still_applied(current: &WindowsProxySettings, applied: &WindowsProxySettings) -> bool {
    current.enabled == applied.enabled
        && current.server == applied.server
        && current.bypass == applied.bypass
}

/// Whether the machine's WinHTTP proxy is still the one the window applied.
pub fn winhttp_still_applied(
    current: &WindowsWinHttpProxy,
    applied: &WindowsProxySettings,
) -> bool {
    applied.enabled
        && current.access_type == WINHTTP_NAMED_PROXY
        && current.server == applied.server
}

/// `snapshot` without the proxy the window applies: a snapshot taken while an earlier connection's proxy was
/// still in place (left by a crash, or recorded by an earlier version) would otherwise put the dead local port
/// back. Those parts become "no proxy".
pub fn sanitized(
    mut snapshot: WindowsProxySnapshot,
    applied: &WindowsProxySettings,
) -> WindowsProxySnapshot {
    if applied.enabled && snapshot.enabled && snapshot.server == applied.server {
        snapshot.enabled = false;
        snapshot.server.clear();
        snapshot.bypass.clear();
    }
    if applied.enabled
        && snapshot.winhttp_access_type == WINHTTP_NAMED_PROXY
        && snapshot.winhttp_server == applied.server
    {
        snapshot.winhttp_access_type = WINHTTP_NO_PROXY;
        snapshot.winhttp_server.clear();
        snapshot.winhttp_bypass.clear();
    }
    snapshot
}

/// The user's WinINet part of `snapshot`.
fn user_settings(snapshot: &WindowsProxySnapshot) -> WindowsProxySettings {
    WindowsProxySettings {
        enabled: snapshot.enabled,
        server: snapshot.server.clone(),
        bypass: snapshot.bypass.clone(),
    }
}

/// The WinHTTP part of `snapshot`.
fn winhttp_settings(snapshot: &WindowsProxySnapshot) -> WindowsWinHttpProxy {
    WindowsWinHttpProxy {
        access_type: snapshot.winhttp_access_type,
        server: snapshot.winhttp_server.clone(),
        bypass: snapshot.winhttp_bypass.clone(),
    }
}

/// What a record found at start-up calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orphan {
    /// Its process still runs (another window of this user): leave it alone.
    OwnerRunning,
    /// Its process is gone and the settings are still the ones it applied: put them back.
    Restore,
    /// Its process is gone and the settings were changed since: only forget the record.
    Forget,
}

pub fn orphan_verdict(
    owner_running: bool,
    current: &WindowsProxySettings,
    record: &OwnedProxy,
) -> Orphan {
    if owner_running {
        Orphan::OwnerRunning
    } else if still_applied(current, &record.applied) {
        Orphan::Restore
    } else {
        Orphan::Forget
    }
}

/// What the start-up check needs from the system, so that it can be tested without touching the real settings.
pub trait ProxySystem {
    /// The SID of the account the window runs as.
    fn current_user(&self) -> Option<String>;
    /// Whether `owner` still runs (and is not a later process given the same id).
    fn owner_running(&self, owner: &ProxyOwner) -> bool;
    /// The current user's WinINet settings.
    fn current_proxy(&self) -> Result<WindowsProxySettings, String>;
    /// Puts the current user's WinINet settings and the machine's WinHTTP proxy back to `snapshot`.
    fn restore(&mut self, snapshot: &WindowsProxySnapshot) -> Result<(), String>;
}

/// What the start-up check did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StartRecovery {
    /// A proxy left by a window that did not stop properly was put back.
    pub restored: bool,
    /// A record was dropped without changing anything, the settings having been changed since.
    pub forgotten: bool,
    /// What could not be done; the record is kept so that the next start tries again.
    pub errors: Vec<String>,
}

/// The start-up check, before automatic connection: puts back the proxy a window of this user left behind when
/// its process is gone, and clears the ownership kept in `desktop`, so that the next connection takes the user's
/// own settings as the ones to put back rather than the dead local port. Leaves everything alone while the
/// owning process runs. Other users' records are left for their own next start and for the uninstaller.
pub fn recover_at_start(
    records: &mut OwnedProxies,
    desktop: &mut DesktopState,
    system: &mut impl ProxySystem,
) -> StartRecovery {
    let mut outcome = StartRecovery::default();
    let user = system.current_user();
    // A window of another user that is connected now shares the desktop state's copy of the ownership.
    let other_running = records.records.iter().any(|record| {
        !same_user(record.user.as_deref(), user.as_deref()) && system.owner_running(&record.owner)
    });
    if let Some(index) = records
        .records
        .iter()
        .position(|record| same_user(record.user.as_deref(), user.as_deref()))
    {
        let record = records.records[index].clone();
        if system.owner_running(&record.owner) {
            return outcome;
        }
        let current = match system.current_proxy() {
            Ok(current) => current,
            Err(error) => {
                outcome.errors.push(error);
                return outcome;
            }
        };
        match orphan_verdict(false, &current, &record) {
            Orphan::Restore => {
                if let Err(error) = system.restore(&sanitized(record.snapshot, &record.applied)) {
                    outcome.errors.push(error);
                    return outcome;
                }
                outcome.restored = true;
            }
            Orphan::Forget | Orphan::OwnerRunning => outcome.forgotten = true,
        }
        records.records.remove(index);
        if !other_running {
            desktop.proxy_snapshot = None;
            desktop.applied_proxy = None;
        }
        return outcome;
    }
    // No record: an earlier version kept the ownership only in the desktop state.
    if other_running {
        return outcome;
    }
    let Some(snapshot) = desktop.proxy_snapshot.clone() else {
        return outcome;
    };
    let current = match system.current_proxy() {
        Ok(current) => current,
        Err(error) => {
            outcome.errors.push(error);
            return outcome;
        }
    };
    if owns_current_proxy(desktop, &current) {
        let snapshot = match desktop.applied_proxy.as_ref() {
            Some(applied) => sanitized(snapshot, applied),
            None => snapshot,
        };
        if let Err(error) = system.restore(&snapshot) {
            outcome.errors.push(error);
            return outcome;
        }
        outcome.restored = true;
    } else {
        outcome.forgotten = true;
    }
    desktop.proxy_snapshot = None;
    desktop.applied_proxy = None;
    outcome
}

/// What the uninstaller needs from the system: it runs as LocalSystem, so each user's settings are reached
/// under `HKEY_USERS`, and only while that user is signed in.
pub trait MachineProxy {
    fn winhttp(&self) -> Result<WindowsWinHttpProxy, String>;
    fn restore_winhttp(&mut self, previous: &WindowsWinHttpProxy) -> Result<(), String>;
    /// `Ok(None)` when the user is not signed in (their settings are not loaded).
    fn user_proxy(&self, sid: &str) -> Result<Option<WindowsProxySettings>, String>;
    fn set_user_proxy(&mut self, sid: &str, settings: &WindowsProxySettings) -> Result<(), String>;
}

/// What the uninstaller put back.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RemovalRestore {
    pub users_restored: usize,
    pub winhttp_restored: bool,
    /// The WinHTTP proxy was put back from the desktop state's copy of the ownership (an earlier version's),
    /// which can be cleared.
    pub desktop_restored: bool,
}

/// The uninstaller's part: the client is going away, so whoever owns them, the settings that still are the
/// ones the window applied are put back. Records of users who are not signed in are kept in `records` (the data
/// folder stays), and a later installation's start puts theirs back; the rest are removed.
pub fn restore_for_removal(
    records: &mut OwnedProxies,
    desktop: Option<&DesktopState>,
    system: &mut impl MachineProxy,
    problems: &mut Vec<String>,
) -> RemovalRestore {
    let mut outcome = RemovalRestore::default();
    // The machine-wide WinHTTP proxy, once: from the record that applied the value in place now.
    match system.winhttp() {
        Ok(current) => {
            let from_record = records
                .records
                .iter()
                .find(|record| winhttp_still_applied(&current, &record.applied))
                .map(|record| sanitized(record.snapshot.clone(), &record.applied));
            let from_desktop = || {
                let desktop = desktop?;
                let applied = desktop.applied_proxy.as_ref()?;
                let snapshot = desktop.proxy_snapshot.clone()?;
                winhttp_still_applied(&current, applied).then(|| sanitized(snapshot, applied))
            };
            let (snapshot, desktop_copy) = match from_record {
                Some(snapshot) => (Some(snapshot), false),
                None => (from_desktop(), true),
            };
            if let Some(snapshot) = snapshot {
                match system.restore_winhttp(&winhttp_settings(&snapshot)) {
                    Ok(()) => {
                        outcome.winhttp_restored = true;
                        outcome.desktop_restored = desktop_copy;
                    }
                    Err(error) => problems.push(format!("WinHTTP proxy: {error}")),
                }
            }
        }
        Err(error) => problems.push(format!("WinHTTP proxy: {error}")),
    }
    let mut kept = Vec::new();
    for record in std::mem::take(&mut records.records) {
        let Some(sid) = record.user.clone().filter(|sid| is_sid(sid)) else {
            // No account to reach: only the window of that user could put it back, and it is going away.
            continue;
        };
        match system.user_proxy(&sid) {
            Ok(Some(current)) => {
                if still_applied(&current, &record.applied) {
                    let snapshot = sanitized(record.snapshot.clone(), &record.applied);
                    match system.set_user_proxy(&sid, &user_settings(&snapshot)) {
                        Ok(()) => outcome.users_restored += 1,
                        Err(error) => {
                            problems.push(format!("proxy settings of {sid}: {error}"));
                            kept.push(record);
                        }
                    }
                }
            }
            Ok(None) => kept.push(record),
            Err(error) => {
                problems.push(format!("proxy settings of {sid}: {error}"));
                kept.push(record);
            }
        }
    }
    records.records = kept;
    outcome
}

/// Whether `value` looks like a SID string (`S-1-…`), the only text that may become part of a registry path.
pub fn is_sid(value: &str) -> bool {
    value.len() > 4
        && value.starts_with("S-1-")
        && value[4..]
            .chars()
            .all(|character| character.is_ascii_digit() || character == '-')
}

pub fn records_path() -> PathBuf {
    windows_managed_data_directory().join(OWNED_PROXY_FILE)
}

/// The records at `path`; none when there is no file. A file that cannot be read is set aside
/// (`owned-proxy.json.broken-<unix seconds>`) and counts as none, so it never blocks a connection.
pub fn load_at(path: &Path) -> OwnedProxies {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return OwnedProxies::default(),
        Err(_) => {
            let _ = set_aside(path);
            return OwnedProxies::default();
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(records) => records,
        Err(_) => {
            let _ = set_aside(path);
            OwnedProxies::default()
        }
    }
}

/// Writes `records` to `path`, or removes the file when there are none.
pub fn save_at(path: &Path, records: &OwnedProxies) -> Result<(), String> {
    if records.records.is_empty() {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("{} could not be removed: {error}", path.display())),
        };
    }
    let content = serde_json::to_string_pretty(records)
        .map_err(|error| format!("proxy owner record could not be serialized: {error}"))?;
    write_managed_text_atomic(path, &content).map_err(|error| error.to_string())
}

/// Loads, changes and saves the records at `path`, one change at a time in this process.
pub fn update_at<T>(path: &Path, change: impl FnOnce(&mut OwnedProxies) -> T) -> Result<T, String> {
    let _guard = RECORDS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut records = load_at(path);
    let before = records.clone();
    let answer = change(&mut records);
    if records != before {
        save_at(path, &records)?;
    }
    Ok(answer)
}

/// The record of this process for the current user, written before the proxy changes. An error means the
/// proxy must not be changed: nothing could put it back after a crash.
pub fn record_before_apply(
    applied: &WindowsProxySettings,
    snapshot: &WindowsProxySnapshot,
) -> Result<(), String> {
    let record = OwnedProxy {
        user: windows_data::account::current_user_sid(),
        owner: native::current_owner(),
        applied: applied.clone(),
        snapshot: snapshot.clone(),
    };
    update_at(&records_path(), |records| records.put(record))
}

/// Drops the current user's record once their proxy was put back, or is no longer the window's.
pub fn forget_current_user() -> Result<(), String> {
    let user = windows_data::account::current_user_sid();
    update_at(&records_path(), |records| {
        records.take_user(user.as_deref());
    })
}

/// What to put back when connecting: the user's WinINet settings `current` and the machine's WinHTTP proxy now.
pub fn snapshot_now(current: &WindowsProxySettings) -> Result<WindowsProxySnapshot, String> {
    let winhttp = native::read_winhttp()?;
    Ok(WindowsProxySnapshot {
        enabled: current.enabled,
        server: current.server.clone(),
        bypass: current.bypass.clone(),
        winhttp_access_type: winhttp.access_type,
        winhttp_server: winhttp.server,
        winhttp_bypass: winhttp.bypass,
    })
}

/// The start-up check against the real system, on the record file.
pub fn recover_at_start_now(desktop: &mut DesktopState) -> Result<StartRecovery, String> {
    update_at(&records_path(), |records| {
        recover_at_start(records, desktop, &mut native::NativeProxySystem)
    })
}

/// The session-end guard's part: puts back the proxy this process set, when it is still in place, and drops the
/// record. Returns the record, so that a cancelled sign-out or shutdown can apply the proxy again.
pub fn release_for_session_end() -> Option<OwnedProxy> {
    use platform_windows::system_integration::{
        read_current_user_system_proxy, NativeWindowsSystemIntegration, WindowsSystemIntegration,
    };

    let me = native::current_owner();
    update_at(&records_path(), |records| {
        let index = records
            .records
            .iter()
            .position(|record| record.owner.pid == me.pid)?;
        let record = records.records[index].clone();
        let current = read_current_user_system_proxy().ok()?;
        if still_applied(&current, &record.applied) {
            NativeWindowsSystemIntegration::new()
                .restore_system_proxy(&sanitized(record.snapshot.clone(), &record.applied))
                .ok()?;
            records.records.remove(index);
            Some(record)
        } else {
            records.records.remove(index);
            None
        }
    })
    .ok()
    .flatten()
}

/// Applies `record`'s proxy again after Windows cancelled the sign-out or shutdown it was put back for, recording
/// it first as on any connection.
pub fn resume_after_cancelled_session_end(record: OwnedProxy) -> Result<(), String> {
    use platform_windows::system_integration::{
        NativeWindowsSystemIntegration, WindowsSystemIntegration,
    };

    let applied = record.applied.clone();
    update_at(&records_path(), |records| records.put(record))?;
    if let Err(error) = NativeWindowsSystemIntegration::new().apply_system_proxy(&applied) {
        let _ = forget_current_user();
        return Err(error.to_string());
    }
    Ok(())
}

/// The uninstaller's part against the real system, on the record file.
pub fn restore_for_removal_now(
    desktop: Option<&DesktopState>,
    problems: &mut Vec<String>,
) -> RemovalRestore {
    match update_at(&records_path(), |records| {
        restore_for_removal(records, desktop, &mut native::NativeMachineProxy, problems)
    }) {
        Ok(outcome) => outcome,
        Err(error) => {
            problems.push(format!("proxy owner record: {error}"));
            RemovalRestore::default()
        }
    }
}

mod native {
    use super::{MachineProxy, ProxyOwner, ProxySystem};
    use platform_windows::managed::{WindowsProxySettings, WindowsProxySnapshot};
    use platform_windows::system_integration::{
        read_current_user_system_proxy, restore_winhttp_proxy, NativeWindowsSystemIntegration,
        WindowsSystemIntegration, WindowsWinHttpProxy,
    };
    use std::ffi::c_void;
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, GlobalFree, FILETIME, HANDLE};
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpGetDefaultProxyConfiguration, WINHTTP_PROXY_INFO,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_USERS,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const INTERNET_SETTINGS_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    /// `STILL_ACTIVE`, what `GetExitCodeProcess` reports for a process that runs.
    const STILL_ACTIVE: u32 = 259;
    const ERROR_FILE_NOT_FOUND: u32 = 2;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcess and is closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// The creation time of the process behind `process`, in `FILETIME` ticks; 0 when it cannot be read.
    fn creation_time(process: HANDLE) -> u64 {
        // SAFETY: FILETIME is plain data; the call writes the four times it is given.
        unsafe {
            let mut created: FILETIME = zeroed();
            let mut exited: FILETIME = zeroed();
            let mut kernel: FILETIME = zeroed();
            let mut user: FILETIME = zeroed();
            if GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) == 0 {
                return 0;
            }
            (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
        }
    }

    pub fn current_owner() -> ProxyOwner {
        ProxyOwner {
            pid: std::process::id(),
            // SAFETY: the pseudo handle of this process needs no closing.
            started: creation_time(unsafe { GetCurrentProcess() }),
        }
    }

    /// Whether `owner` still runs. A process that cannot be opened is not the window's (the elevated window can
    /// open any other window of the client), and one created at another time was only given the same id.
    pub fn owner_running(owner: &ProxyOwner) -> bool {
        if owner.pid == 0 {
            return false;
        }
        // SAFETY: OpenProcess takes plain values and returns null on failure.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, owner.pid) };
        if process.is_null() {
            return false;
        }
        let process = Handle(process);
        let mut code = 0_u32;
        // SAFETY: the handle is open with the right the call needs; `code` is valid for the write.
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 || code != STILL_ACTIVE {
            return false;
        }
        owner.started == 0 || creation_time(process.0) == owner.started
    }

    /// The machine's WinHTTP proxy.
    pub fn read_winhttp() -> Result<WindowsWinHttpProxy, String> {
        // SAFETY: WINHTTP_PROXY_INFO is plain data the call fills; the strings it allocates are freed below.
        unsafe {
            let mut info: WINHTTP_PROXY_INFO = zeroed();
            if WinHttpGetDefaultProxyConfiguration(&mut info) == 0 {
                return Err(format!(
                    "the WinHTTP proxy could not be read: error {}",
                    GetLastError()
                ));
            }
            Ok(WindowsWinHttpProxy {
                access_type: info.dwAccessType,
                server: take_global_string(info.lpszProxy),
                bypass: take_global_string(info.lpszProxyBypass),
            })
        }
    }

    /// Reads and frees a string WinHTTP allocated with `GlobalAlloc`.
    unsafe fn take_global_string(text: *mut u16) -> String {
        if text.is_null() {
            return String::new();
        }
        let mut length = 0;
        // SAFETY: WinHTTP returns NUL-terminated strings.
        while unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
        unsafe {
            GlobalFree(text.cast::<c_void>());
        }
        value
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key came from RegOpenKeyExW and is closed once.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    /// The Internet Settings key of the user `sid`; `None` when their settings are not loaded.
    fn user_key(sid: &str, access: u32) -> Result<Option<Key>, String> {
        if !super::is_sid(sid) {
            return Err("not an account SID".to_string());
        }
        let path = wide(&format!(r"{sid}\{INTERNET_SETTINGS_KEY}"));
        let mut key: HKEY = null_mut();
        // SAFETY: `path` is NUL-terminated and `key` is valid for the write.
        let result = unsafe { RegOpenKeyExW(HKEY_USERS, path.as_ptr(), 0, access, &mut key) };
        match result {
            0 => Ok(Some(Key(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            error => Err(format!("the settings could not be opened: error {error}")),
        }
    }

    fn read_dword(key: &Key, name: &str) -> Result<u32, String> {
        let name = wide(name);
        let mut value = 0_u32;
        let mut size = 4_u32;
        // SAFETY: `name` is NUL-terminated and `value` holds the four bytes the call may write.
        let result = unsafe {
            RegGetValueW(
                key.0,
                null(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                (&mut value as *mut u32).cast::<c_void>(),
                &mut size,
            )
        };
        match result {
            0 => Ok(value),
            ERROR_FILE_NOT_FOUND => Ok(0),
            error => Err(format!("a proxy setting could not be read: error {error}")),
        }
    }

    fn read_string(key: &Key, name: &str) -> Result<String, String> {
        let name = wide(name);
        let mut size = 0_u32;
        // SAFETY: with no buffer the call only reports the size needed.
        let result = unsafe {
            RegGetValueW(
                key.0,
                null(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                null_mut(),
                &mut size,
            )
        };
        match result {
            0 => {}
            ERROR_FILE_NOT_FOUND => return Ok(String::new()),
            error => return Err(format!("a proxy setting could not be read: error {error}")),
        }
        let mut buffer = vec![0_u16; (size as usize).div_ceil(2) + 1];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` holds `size` bytes, and the call writes at most that many.
        let result = unsafe {
            RegGetValueW(
                key.0,
                null(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut size,
            )
        };
        if result != 0 {
            return Err(format!("a proxy setting could not be read: error {result}"));
        }
        let length = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..length]))
    }

    fn write_dword(key: &Key, name: &str, value: u32) -> Result<(), String> {
        let name = wide(name);
        // SAFETY: `name` is NUL-terminated and the data is the four bytes of `value`.
        let result = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_DWORD,
                (&value as *const u32).cast::<u8>(),
                4,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(format!(
                "a proxy setting could not be written: error {result}"
            ))
        }
    }

    fn write_string(key: &Key, name: &str, value: &str) -> Result<(), String> {
        let name = wide(name);
        let value = wide(value);
        // SAFETY: both are NUL-terminated and the length given is `value`'s, in bytes, with its NUL.
        let result = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast::<u8>(),
                (value.len() * 2) as u32,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(format!(
                "a proxy setting could not be written: error {result}"
            ))
        }
    }

    /// The window, as the signed-in user it runs for.
    pub struct NativeProxySystem;

    impl ProxySystem for NativeProxySystem {
        fn current_user(&self) -> Option<String> {
            windows_data::account::current_user_sid()
        }

        fn owner_running(&self, owner: &ProxyOwner) -> bool {
            owner_running(owner)
        }

        fn current_proxy(&self) -> Result<WindowsProxySettings, String> {
            read_current_user_system_proxy().map_err(|error| error.to_string())
        }

        fn restore(&mut self, snapshot: &WindowsProxySnapshot) -> Result<(), String> {
            NativeWindowsSystemIntegration::new()
                .restore_system_proxy(snapshot)
                .map_err(|error| error.to_string())
        }
    }

    /// The uninstaller, as LocalSystem.
    pub struct NativeMachineProxy;

    impl MachineProxy for NativeMachineProxy {
        fn winhttp(&self) -> Result<WindowsWinHttpProxy, String> {
            read_winhttp()
        }

        fn restore_winhttp(&mut self, previous: &WindowsWinHttpProxy) -> Result<(), String> {
            restore_winhttp_proxy(previous).map_err(|error| error.to_string())
        }

        fn user_proxy(&self, sid: &str) -> Result<Option<WindowsProxySettings>, String> {
            let Some(key) = user_key(sid, KEY_QUERY_VALUE)? else {
                return Ok(None);
            };
            Ok(Some(WindowsProxySettings {
                enabled: read_dword(&key, "ProxyEnable")? != 0,
                server: read_string(&key, "ProxyServer")?,
                bypass: read_string(&key, "ProxyOverride")?,
            }))
        }

        fn set_user_proxy(
            &mut self,
            sid: &str,
            settings: &WindowsProxySettings,
        ) -> Result<(), String> {
            let key = user_key(sid, KEY_QUERY_VALUE | KEY_SET_VALUE)?
                .ok_or_else(|| "the user signed out".to_string())?;
            write_dword(&key, "ProxyEnable", u32::from(settings.enabled))?;
            write_string(&key, "ProxyServer", &settings.server)?;
            write_string(&key, "ProxyOverride", &settings.bypass)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const ALICE: &str = "S-1-5-21-1-2-3-1001";
    const BOB: &str = "S-1-5-21-1-2-3-1002";

    fn applied() -> WindowsProxySettings {
        WindowsProxySettings {
            enabled: true,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
        }
    }

    fn original() -> WindowsProxySnapshot {
        WindowsProxySnapshot {
            enabled: true,
            server: "corp.example:8080".to_string(),
            bypass: "*.corp.example".to_string(),
            winhttp_access_type: WINHTTP_NO_PROXY,
            winhttp_server: String::new(),
            winhttp_bypass: String::new(),
        }
    }

    fn record(user: Option<&str>, pid: u32) -> OwnedProxy {
        OwnedProxy {
            user: user.map(str::to_string),
            owner: ProxyOwner { pid, started: 42 },
            applied: applied(),
            snapshot: original(),
        }
    }

    fn user_proxy(snapshot: &WindowsProxySnapshot) -> WindowsProxySettings {
        user_settings(snapshot)
    }

    struct FakeSystem {
        user: Option<String>,
        running: Vec<u32>,
        current: Result<WindowsProxySettings, String>,
        restore_error: Option<String>,
        restored: Vec<WindowsProxySnapshot>,
    }

    impl FakeSystem {
        fn new(current: WindowsProxySettings) -> Self {
            Self {
                user: Some(ALICE.to_string()),
                running: Vec::new(),
                current: Ok(current),
                restore_error: None,
                restored: Vec::new(),
            }
        }
    }

    impl ProxySystem for FakeSystem {
        fn current_user(&self) -> Option<String> {
            self.user.clone()
        }

        fn owner_running(&self, owner: &ProxyOwner) -> bool {
            self.running.contains(&owner.pid)
        }

        fn current_proxy(&self) -> Result<WindowsProxySettings, String> {
            self.current.clone()
        }

        fn restore(&mut self, snapshot: &WindowsProxySnapshot) -> Result<(), String> {
            if let Some(error) = &self.restore_error {
                return Err(error.clone());
            }
            self.restored.push(snapshot.clone());
            self.current = Ok(user_settings(snapshot));
            Ok(())
        }
    }

    fn owned_desktop() -> DesktopState {
        DesktopState {
            proxy_snapshot: Some(original()),
            applied_proxy: Some(applied()),
            ..DesktopState::default()
        }
    }

    #[test]
    fn an_orphaned_proxy_is_put_back_at_start_and_the_ownership_cleared() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 100));
        let mut desktop = owned_desktop();
        let mut system = FakeSystem::new(applied());
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert!(outcome.restored);
        assert!(outcome.errors.is_empty());
        assert_eq!(system.restored, [original()]);
        assert!(records.records.is_empty());
        // Automatic connection then takes the user's own settings as the ones to put back.
        assert!(desktop.proxy_snapshot.is_none());
        assert!(desktop.applied_proxy.is_none());
        assert!(!owns_current_proxy(&desktop, &user_proxy(&original())));
    }

    #[test]
    fn a_proxy_the_user_changed_since_is_left_alone() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 100));
        let mut desktop = owned_desktop();
        let changed = WindowsProxySettings {
            enabled: true,
            server: "other.example:3128".to_string(),
            bypass: String::new(),
        };
        let mut system = FakeSystem::new(changed);
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert!(!outcome.restored);
        assert!(outcome.forgotten);
        assert!(system.restored.is_empty());
        assert!(records.records.is_empty());
        assert!(desktop.proxy_snapshot.is_none());
    }

    #[test]
    fn a_window_that_still_runs_keeps_its_proxy() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 100));
        let mut desktop = owned_desktop();
        let mut system = FakeSystem::new(applied());
        system.running.push(100);
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert_eq!(outcome, StartRecovery::default());
        assert!(system.restored.is_empty());
        assert_eq!(records.records.len(), 1);
        assert!(desktop.proxy_snapshot.is_some());
    }

    #[test]
    fn a_failed_restore_keeps_the_record_for_the_next_start() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 100));
        let mut desktop = owned_desktop();
        let mut system = FakeSystem::new(applied());
        system.restore_error = Some("access denied".to_string());
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert!(!outcome.restored);
        assert_eq!(outcome.errors, ["access denied"]);
        assert_eq!(records.records.len(), 1);
        assert!(desktop.proxy_snapshot.is_some());
    }

    #[test]
    fn another_users_record_waits_for_them() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(BOB), 100));
        let mut desktop = DesktopState::default();
        let mut system = FakeSystem::new(applied());
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert_eq!(outcome, StartRecovery::default());
        assert!(system.restored.is_empty());
        assert_eq!(records.records.len(), 1);
    }

    #[test]
    fn an_earlier_versions_snapshot_is_put_back_without_a_record() {
        let mut records = OwnedProxies::default();
        let mut desktop = owned_desktop();
        let mut system = FakeSystem::new(applied());
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert!(outcome.restored);
        assert_eq!(system.restored, [original()]);
        assert!(desktop.proxy_snapshot.is_none());
        assert!(desktop.applied_proxy.is_none());

        // Not while a connected window of another user shares the desktop state.
        let mut records = OwnedProxies::default();
        records.put(record(Some(BOB), 200));
        let mut desktop = owned_desktop();
        let mut system = FakeSystem::new(applied());
        system.running.push(200);
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert_eq!(outcome, StartRecovery::default());
        assert!(desktop.proxy_snapshot.is_some());
    }

    #[test]
    fn nothing_is_done_without_any_ownership() {
        let mut records = OwnedProxies::default();
        let mut desktop = DesktopState::default();
        let mut system = FakeSystem::new(applied());
        let outcome = recover_at_start(&mut records, &mut desktop, &mut system);
        assert_eq!(outcome, StartRecovery::default());
        assert!(system.restored.is_empty());
    }

    #[test]
    fn a_snapshot_holding_the_applied_proxy_puts_back_no_proxy() {
        let polluted = WindowsProxySnapshot {
            enabled: true,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
            winhttp_access_type: WINHTTP_NAMED_PROXY,
            winhttp_server: "127.0.0.1:7890".to_string(),
            winhttp_bypass: "<local>".to_string(),
        };
        let clean = sanitized(polluted, &applied());
        assert!(!clean.enabled);
        assert!(clean.server.is_empty());
        assert_eq!(clean.winhttp_access_type, WINHTTP_NO_PROXY);
        assert!(clean.winhttp_server.is_empty());
        // The user's own settings pass through.
        assert_eq!(sanitized(original(), &applied()), original());
    }

    #[test]
    fn records_are_kept_one_per_user() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 1));
        records.put(record(Some(BOB), 2));
        records.put(record(Some(ALICE.to_ascii_lowercase().as_str()), 3));
        assert_eq!(records.records.len(), 2);
        assert_eq!(records.take_user(Some(ALICE)).unwrap().owner.pid, 3);
        assert!(records.take_user(Some(ALICE)).is_none());
        // A record whose account was not known belongs to whoever starts the window next.
        assert!(same_user(None, Some(BOB)));
        assert!(!same_user(Some(ALICE), None));
    }

    #[test]
    fn only_account_sids_reach_a_registry_path() {
        assert!(is_sid(ALICE));
        assert!(is_sid("S-1-12-1-123456789-1234567890"));
        assert!(!is_sid("S-1-"));
        assert!(!is_sid(r"S-1-5-21-1\..\Software"));
        assert!(!is_sid(".DEFAULT"));
    }

    #[test]
    fn the_record_file_round_trips_and_an_unreadable_one_is_set_aside() {
        let directory = std::env::temp_dir().join(format!(
            "anixops-owned-proxy-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(OWNED_PROXY_FILE);
        assert_eq!(load_at(&path), OwnedProxies::default());

        update_at(&path, |records| records.put(record(Some(ALICE), 7))).unwrap();
        assert_eq!(load_at(&path).records, [record(Some(ALICE), 7)]);
        // The last record gone, the file goes too.
        update_at(&path, |records| {
            records.take_user(Some(ALICE));
        })
        .unwrap();
        assert!(!path.exists());

        fs::write(&path, b"{\"records\": [ {\"user\": ").unwrap();
        assert_eq!(load_at(&path), OwnedProxies::default());
        assert!(!path.exists());
        let set_aside = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("owned-proxy.json.broken-")
            })
            .count();
        assert_eq!(set_aside, 1);
        let _ = fs::remove_dir_all(&directory);
    }

    struct FakeMachine {
        winhttp: WindowsWinHttpProxy,
        users: Vec<(String, WindowsProxySettings)>,
    }

    impl MachineProxy for FakeMachine {
        fn winhttp(&self) -> Result<WindowsWinHttpProxy, String> {
            Ok(self.winhttp.clone())
        }

        fn restore_winhttp(&mut self, previous: &WindowsWinHttpProxy) -> Result<(), String> {
            self.winhttp = previous.clone();
            Ok(())
        }

        fn user_proxy(&self, sid: &str) -> Result<Option<WindowsProxySettings>, String> {
            Ok(self
                .users
                .iter()
                .find(|(user, _)| user == sid)
                .map(|(_, settings)| settings.clone()))
        }

        fn set_user_proxy(
            &mut self,
            sid: &str,
            settings: &WindowsProxySettings,
        ) -> Result<(), String> {
            let entry = self
                .users
                .iter_mut()
                .find(|(user, _)| user == sid)
                .ok_or("signed out")?;
            entry.1 = settings.clone();
            Ok(())
        }
    }

    fn applied_winhttp() -> WindowsWinHttpProxy {
        WindowsWinHttpProxy {
            access_type: WINHTTP_NAMED_PROXY,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
        }
    }

    #[test]
    fn removal_puts_back_winhttp_and_signed_in_users_and_keeps_the_others() {
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 1));
        records.put(record(Some(BOB), 2));
        let changed = WindowsProxySettings {
            enabled: false,
            server: String::new(),
            bypass: String::new(),
        };
        let mut machine = FakeMachine {
            winhttp: applied_winhttp(),
            users: vec![(ALICE.to_string(), applied())],
        };
        let mut problems = Vec::new();
        let outcome = restore_for_removal(&mut records, None, &mut machine, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(outcome.winhttp_restored);
        assert_eq!(outcome.users_restored, 1);
        assert_eq!(machine.winhttp.access_type, WINHTTP_NO_PROXY);
        assert_eq!(machine.users[0].1, user_proxy(&original()));
        // Bob is not signed in: his record waits.
        assert_eq!(records.records.len(), 1);
        assert_eq!(records.records[0].user.as_deref(), Some(BOB));

        // Settings changed by the user are not touched.
        let mut records = OwnedProxies::default();
        records.put(record(Some(ALICE), 1));
        let mut machine = FakeMachine {
            winhttp: WindowsWinHttpProxy {
                access_type: WINHTTP_NAMED_PROXY,
                server: "corp.example:8080".to_string(),
                bypass: String::new(),
            },
            users: vec![(ALICE.to_string(), changed.clone())],
        };
        let outcome = restore_for_removal(&mut records, None, &mut machine, &mut problems);
        assert_eq!(outcome, RemovalRestore::default());
        assert_eq!(machine.users[0].1, changed);
        assert_eq!(machine.winhttp.server, "corp.example:8080");
        assert!(records.records.is_empty());
    }

    #[test]
    fn removal_uses_an_earlier_versions_snapshot_for_winhttp() {
        let mut records = OwnedProxies::default();
        let desktop = owned_desktop();
        let mut machine = FakeMachine {
            winhttp: applied_winhttp(),
            users: Vec::new(),
        };
        let mut problems = Vec::new();
        let outcome =
            restore_for_removal(&mut records, Some(&desktop), &mut machine, &mut problems);
        assert!(outcome.winhttp_restored);
        assert!(outcome.desktop_restored);
        assert_eq!(machine.winhttp.access_type, WINHTTP_NO_PROXY);
    }

    #[test]
    fn the_verdict_follows_the_owner_and_the_settings() {
        let record = record(Some(ALICE), 1);
        assert_eq!(
            orphan_verdict(true, &applied(), &record),
            Orphan::OwnerRunning
        );
        assert_eq!(orphan_verdict(false, &applied(), &record), Orphan::Restore);
        assert_eq!(
            orphan_verdict(false, &user_proxy(&original()), &record),
            Orphan::Forget
        );
        assert!(winhttp_still_applied(&applied_winhttp(), &applied()));
        assert!(!winhttp_still_applied(
            &WindowsWinHttpProxy {
                access_type: WINHTTP_NO_PROXY,
                server: String::new(),
                bypass: String::new(),
            },
            &applied()
        ));
    }
}
