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
    bind_socket_to_interface, default_wintun_dll_path, WindowsTun, WindowsTunSettings,
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
        let config = RuntimeConfig::from_json(envelope)
            .map_err(|error| format!("TUN mode configuration was rejected: {}", error.message))?;
        let settings = WindowsTunSettings {
            mtu: u32::from(config.mtu),
            ..WindowsTunSettings::default()
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
