//! TUN mode: the embedded engine behind a Wintun adapter, so applications
//! that ignore the system proxy (games, command line tools, UDP) are routed
//! too. It runs next to the local proxy: proxy-aware applications still reach
//! the local proxy (and HTTPS decryption) with their domain names.
//!
//! The engine's own sockets are pinned to the physical interface through the
//! kernel's socket protector, or they would loop back into the adapter.

use engine_embedded::config::RuntimeConfig;
use engine_embedded::protect::{set_socket_protector, RawSocketHandle};
use engine_embedded::{EmbeddedRuntime, RuntimeState};
use platform_windows::managed::windows_managed_data_directory;
use platform_windows::tun::{
    bind_socket_to_interface, default_wintun_dll_path, CaptureRoute, WindowsTun, WindowsTunSettings,
};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::net::SocketAddr;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct DesktopTun {
    runtime: Arc<Mutex<EmbeddedRuntime>>,
    tun: Option<WindowsTun>,
    /// Holds the adapter for this process while it runs (dropped after `tun`).
    _lease: AdapterLease,
}

/// Why the adapter or its engine did not start: `code` is the failure the 组网 page words (mesh-status.md
/// `network.failure.code`: `permission_denied`, `tun_in_use`, else `other`), taken from the system error code, never
/// guessed from text; `message` goes to the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunStartError {
    pub code: &'static str,
    pub message: String,
}

impl TunStartError {
    fn other(message: String) -> Self {
        Self {
            code: "other",
            message,
        }
    }
}

impl fmt::Display for TunStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<TunStartError> for String {
    fn from(error: TunStartError) -> Self {
        error.message
    }
}

/// ERROR_ACCESS_DENIED.
const ACCESS_DENIED: i32 = 5;
/// ERROR_SHARING_VIOLATION: another process holds the file open without sharing.
const SHARING_VIOLATION: i32 = 32;

/// The failure code of an adapter that could not be created: `permission_denied` for a refused access, else
/// `other` (an address or route the system refused is not told apart here).
fn adapter_failure_code(error: &io::Error) -> &'static str {
    if error.raw_os_error() == Some(ACCESS_DENIED)
        || error.kind() == io::ErrorKind::PermissionDenied
    {
        "permission_denied"
    } else {
        "other"
    }
}

/// The adapter has one fixed GUID on the machine, and each signed-in user runs a window of their own: a file held
/// open without sharing keeps the adapter to one process at a time, so a second user's window reports `tun_in_use`
/// instead of replacing the first one's adapter and routes. The lock goes with the process, crash included. Within
/// the process the proxy's and the mesh's adapters take turns, so the hold is counted.
pub struct ProcessLock {
    held: Mutex<(Option<File>, usize)>,
}

impl ProcessLock {
    pub const fn new() -> Self {
        Self {
            held: Mutex::new((None, 0)),
        }
    }

    pub(in crate::gui) fn acquire(
        &'static self,
        path: &Path,
    ) -> Result<AdapterLease, TunStartError> {
        let mut held = self
            .held
            .lock()
            .map_err(|_| TunStartError::other("the TUN adapter lock failed".into()))?;
        if held.0.is_none() {
            held.0 = Some(open_exclusive(path).map_err(|error| {
                if error.raw_os_error() == Some(SHARING_VIOLATION) {
                    TunStartError {
                        code: "tun_in_use",
                        message: "the TUN adapter is in use by another signed-in user".into(),
                    }
                } else {
                    TunStartError::other(format!(
                        "the TUN adapter lock could not be opened: {error}"
                    ))
                }
            })?);
        }
        held.1 += 1;
        Ok(AdapterLease { lock: self })
    }

    fn release(&self) {
        if let Ok(mut held) = self.held.lock() {
            held.1 = held.1.saturating_sub(1);
            if held.1 == 0 {
                held.0 = None;
            }
        }
    }
}

/// One hold of a [`ProcessLock`]; the file closes when the last one drops.
pub struct AdapterLease {
    lock: &'static ProcessLock,
}

impl Drop for AdapterLease {
    fn drop(&mut self) {
        self.lock.release();
    }
}

/// FILE_FLAG_OPEN_REPARSE_POINT: open a link itself rather than what it points to.
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
/// FILE_ATTRIBUTE_REPARSE_POINT.
const REPARSE_POINT: u32 = 0x0400;

/// Opens the lock file without sharing. A link planted in its place is opened as the link itself and refused, so the
/// lock never opens, creates or holds a file elsewhere.
pub(in crate::gui) fn open_exclusive(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes() & REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the TUN adapter lock is a link, not a file",
        ));
    }
    Ok(file)
}

static ADAPTER: ProcessLock = ProcessLock::new();

fn adapter_lock_path() -> PathBuf {
    windows_managed_data_directory().join("tun-adapter.lock")
}

/// Holds the adapter for this process without opening it, for a change that must not reach another signed-in
/// user's adapter (the name resolution rules a crash left behind, `mesh_dns::clear_leftovers`): `tun_in_use` while
/// another user's window holds it.
pub fn hold_adapter() -> Result<AdapterLease, TunStartError> {
    ADAPTER.acquire(&adapter_lock_path())
}

impl DesktopTun {
    /// Creates the adapter, pins the engine's sockets to the physical
    /// interface and starts the packet engine on `envelope`.
    pub fn start(envelope: &[u8]) -> Result<Self, String> {
        Self::start_with(envelope, WindowsTunSettings::default()).map_err(String::from)
    }

    /// The same with the adapter's own `settings` (the MTU is the
    /// envelope's): for the mesh alone, an adapter that carries only the
    /// mesh's routes ([`Self::set_routes`]).
    pub fn start_with(
        envelope: &[u8],
        settings: WindowsTunSettings,
    ) -> Result<Self, TunStartError> {
        let config = RuntimeConfig::from_json(envelope).map_err(|error| {
            TunStartError::other(format!(
                "TUN mode configuration was rejected: {}",
                error.message
            ))
        })?;
        let settings = WindowsTunSettings {
            mtu: u32::from(config.mtu),
            ..settings
        };
        let dll = default_wintun_dll_path().map_err(|error| {
            TunStartError::other(format!("TUN mode could not find wintun.dll: {error}"))
        })?;
        let lease = ADAPTER.acquire(&adapter_lock_path())?;
        let mut tun = WindowsTun::open(&settings, &dll).map_err(|error| TunStartError {
            code: adapter_failure_code(&error),
            message: format!("TUN adapter could not be created: {error}"),
        })?;

        let physical = tun.physical_interface();
        set_socket_protector(Some(Arc::new(
            move |socket: RawSocketHandle, target: SocketAddr| {
                let ipv6 = target.is_ipv6();
                match physical.index_for(ipv6) {
                    Some(index) => bind_socket_to_interface(socket, index, ipv6),
                    None => Err(io::Error::new(
                        io::ErrorKind::NetworkUnreachable,
                        "no physical interface for this address family",
                    )),
                }
            },
        )));

        let started = (|| {
            let mut runtime = EmbeddedRuntime::from_json(envelope).map_err(|error| {
                format!("TUN mode engine could not be built: {}", error.message)
            })?;
            let writer = tun.writer();
            runtime
                .start(move |packet: &[u8]| {
                    writer.write(packet);
                })
                .map_err(|error| format!("TUN mode engine could not start: {}", error.message))?;
            let runtime = Arc::new(Mutex::new(runtime));
            let reader_runtime = runtime.clone();
            tun.start_reader(move |packet| {
                if let Ok(runtime) = reader_runtime.lock() {
                    runtime.input_packet(packet);
                }
            })
            .map_err(|error| format!("TUN adapter reader could not start: {error}"))?;
            Ok::<_, String>(runtime)
        })();
        match started {
            Ok(runtime) => Ok(Self {
                runtime,
                tun: Some(tun),
                _lease: lease,
            }),
            Err(error) => {
                drop(tun);
                set_socket_protector(None);
                Err(TunStartError::other(error))
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.tun.is_some()
            && self
                .runtime
                .lock()
                .is_ok_and(|runtime| matches!(runtime.status().state, RuntimeState::Running))
    }

    /// Answers one kernel control request of the packet engine (a policy group choice, so that it routes as the
    /// local proxy does).
    pub fn control(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.runtime
            .lock()
            .map_err(|_| "TUN engine lock failed".to_string())?
            .control_json(request)
            .map_err(|error| error.message.to_string())
    }

    /// Replaces the routes the adapter carries, keeping the ones that stay.
    pub fn set_routes(&mut self, routes: &[CaptureRoute]) -> Result<(), String> {
        self.tun
            .as_mut()
            .ok_or_else(|| "the TUN adapter is closed".to_string())?
            .set_routes(routes)
            .map_err(|error| format!("the TUN routes could not be applied: {error}"))
    }

    /// The adapter's interface index while it is open.
    pub fn interface_index(&self) -> Option<u32> {
        self.tun.as_ref().map(WindowsTun::interface_index)
    }

    /// The routes the adapter carries now.
    pub fn routes(&self) -> Vec<CaptureRoute> {
        self.tun
            .as_ref()
            .map(|tun| tun.routes().to_vec())
            .unwrap_or_default()
    }

    /// Removes the adapter (and its routes) first, then stops the engine and
    /// releases the sockets' interface pinning.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(tun) = self.tun.take() else {
            return;
        };
        drop(tun);
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.stop();
        }
        set_socket_protector(None);
    }
}

impl Drop for DesktopTun {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_process_cannot_take_the_adapter_this_one_holds() {
        static LOCK: ProcessLock = ProcessLock::new();
        let dir = std::env::temp_dir().join(format!("anixops-tun-lock-{}", std::process::id()));
        let path = dir.join("tun-adapter.lock");
        let first = LOCK.acquire(&path).expect("first hold");
        // The proxy's and the mesh's adapters take turns within the process.
        let second = LOCK
            .acquire(&path)
            .expect("second hold in the same process");
        // What another process sees: the file is held without sharing.
        let other = open_exclusive(&path).unwrap_err();
        assert_eq!(other.raw_os_error(), Some(SHARING_VIOLATION));
        drop(first);
        assert!(
            open_exclusive(&path).is_err(),
            "still held by the second lease"
        );
        drop(second);
        drop(open_exclusive(&path).expect("free once the last lease is gone"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_link_in_place_of_the_lock_is_refused() {
        let dir = std::env::temp_dir().join(format!("anixops-tun-link-{}", std::process::id()));
        let target = dir.join("elsewhere");
        let link = dir.join("tun-adapter.lock");
        fs::create_dir_all(&target).unwrap();
        // A directory junction needs no privilege, unlike a symbolic link.
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .expect("cmd runs");
        assert!(made.success(), "the junction was made");
        let refused = open_exclusive(&link).unwrap_err();
        assert!(
            refused.kind() == io::ErrorKind::InvalidInput || refused.raw_os_error().is_some(),
            "{refused:?}"
        );
        assert!(
            fs::read_dir(&target).unwrap().next().is_none(),
            "nothing was created behind the link"
        );
        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_refused_adapter_is_worded_by_its_system_error_code() {
        assert_eq!(
            adapter_failure_code(&io::Error::from_raw_os_error(ACCESS_DENIED)),
            "permission_denied"
        );
        // An address or route the system refused (ERROR_OBJECT_ALREADY_EXISTS) is not a busy adapter.
        assert_eq!(
            adapter_failure_code(&io::Error::from_raw_os_error(5010)),
            "other"
        );
        assert_eq!(
            adapter_failure_code(&io::Error::from_raw_os_error(183)),
            "other"
        );
    }
}
