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
use platform_windows::tun::{
    bind_socket_to_interface, default_wintun_dll_path, CaptureRoute, WindowsTun, WindowsTunSettings,
};
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub struct DesktopTun {
    runtime: Arc<Mutex<EmbeddedRuntime>>,
    tun: Option<WindowsTun>,
}

impl DesktopTun {
    /// Creates the adapter, pins the engine's sockets to the physical
    /// interface and starts the packet engine on `envelope`.
    pub fn start(envelope: &[u8]) -> Result<Self, String> {
        Self::start_with(envelope, WindowsTunSettings::default())
    }

    /// The same with the adapter's own `settings` (the MTU is the
    /// envelope's): for the mesh alone, an adapter that carries only the
    /// mesh's routes ([`Self::set_routes`]).
    pub fn start_with(envelope: &[u8], settings: WindowsTunSettings) -> Result<Self, String> {
        let config = RuntimeConfig::from_json(envelope)
            .map_err(|error| format!("TUN mode configuration was rejected: {}", error.message))?;
        let settings = WindowsTunSettings {
            mtu: u32::from(config.mtu),
            ..settings
        };
        let dll = default_wintun_dll_path()
            .map_err(|error| format!("TUN mode could not find wintun.dll: {error}"))?;
        let mut tun = WindowsTun::open(&settings, &dll)
            .map_err(|error| format!("TUN adapter could not be created: {error}"))?;

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
            }),
            Err(error) => {
                drop(tun);
                set_socket_protector(None);
                Err(error)
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
