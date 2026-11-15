//! Windows TUN mode: the engine behind a Wintun adapter, so that applications that ignore the system proxy
//! (games, command line tools, UDP) are carried too. The same packet path as on the other systems
//! (`EmbeddedRuntime`: IP packets in, IP packets out).
//!
//! Creating the adapter needs administrator rights. The native client has them where it matters: the core
//! runs in the system service (`apps/windows-service`), and the window never needs them. `wintun.dll` is
//! expected next to the running program, where the installer puts it.
//!
//! Unlike the other systems there is no descriptor to keep between runtimes: [`attach`] only records that
//! the user wants TUN mode for this connection, and every [`TunEngine`] makes its own adapter and removes it
//! (with its routes) when it stops. The engine's own sockets are pinned to the physical interface, or they
//! would loop back into the adapter.

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use engine_embedded::config::RuntimeConfig;
use engine_embedded::protect::{set_socket_protector, RawSocketHandle};
use engine_embedded::{EmbeddedRuntime, RuntimeState};
use platform_windows::tun::{
    bind_socket_to_interface, default_wintun_dll_path, WindowsTun, WindowsTunSettings,
};

/// Whether the connection being made, or the one that runs, is in TUN mode.
static WANTED: AtomicBool = AtomicBool::new(false);

/// The next runtime runs on a Wintun adapter. Call only while no runtime runs.
pub fn attach() {
    WANTED.store(true, Ordering::Release);
}

/// Whether runtimes run on a Wintun adapter.
pub fn attached() -> bool {
    WANTED.load(Ordering::Acquire)
}

/// Back to the local proxy for the next runtime. Call only while no runtime runs.
pub fn detach() {
    WANTED.store(false, Ordering::Release);
}

/// One runtime on its own Wintun adapter.
pub struct TunEngine {
    runtime: Arc<Mutex<EmbeddedRuntime>>,
    tun: Option<WindowsTun>,
}

fn lock(runtime: &Mutex<EmbeddedRuntime>) -> MutexGuard<'_, EmbeddedRuntime> {
    runtime.lock().unwrap_or_else(PoisonError::into_inner)
}

impl TunEngine {
    /// Creates the adapter, pins the engine's sockets to the physical interface and starts the engine on
    /// `envelope`.
    pub fn start(envelope: &str) -> Result<Self, String> {
        let config = RuntimeConfig::from_json(envelope.as_bytes())
            .map_err(|error| error.message.to_string())?;
        let settings = WindowsTunSettings {
            mtu: u32::from(config.mtu),
            ..WindowsTunSettings::default()
        };
        let dll = default_wintun_dll_path()
            .map_err(|error| format!("wintun.dll could not be located: {error}"))?;
        if !dll.is_file() {
            return Err(
                "TUN mode needs wintun.dll next to the client; install the client again"
                    .to_string(),
            );
        }
        let mut tun = WindowsTun::open(&settings, &dll)
            .map_err(|error| format!("the TUN adapter could not be created: {error}"))?;

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
            let mut runtime = EmbeddedRuntime::from_json(envelope.as_bytes())
                .map_err(|error| error.message.to_string())?;
            let writer = tun.writer();
            runtime
                .start(move |packet: &[u8]| {
                    // A full ring drops the packet, as a congested link would.
                    writer.write(packet);
                })
                .map_err(|error| error.message.to_string())?;
            let runtime = Arc::new(Mutex::new(runtime));
            let reading = runtime.clone();
            tun.start_reader(move |packet| {
                lock(&reading).input_packet(packet);
            })
            .map_err(|error| format!("the TUN adapter's reader could not start: {error}"))?;
            Ok::<_, String>(runtime)
        })();
        match started {
            Ok(runtime) => Ok(Self {
                runtime,
                tun: Some(tun),
            }),
            Err(error) => {
                drop(tun);
                set_socket_protector(None);
                Err(error)
            }
        }
    }

    /// Removes the adapter and its routes first, then stops the runtime and lets its sockets go where the
    /// routing table says again.
    pub fn stop(&mut self) {
        let Some(tun) = self.tun.take() else {
            return;
        };
        drop(tun);
        lock(&self.runtime).stop();
        set_socket_protector(None);
    }

    pub fn is_running(&self) -> bool {
        self.tun.is_some() && matches!(lock(&self.runtime).status().state, RuntimeState::Running)
    }

    pub fn control_json(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        lock(&self.runtime)
            .control_json(request)
            .map_err(|error| error.message.to_string())
    }
}

impl Drop for TunEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
