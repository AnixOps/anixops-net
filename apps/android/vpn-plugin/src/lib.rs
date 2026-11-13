//! The Android VpnService behind AnixOps Net (Kotlin in `android/`). `start` asks for the VPN consent the first
//! time, brings up the interface and returns its TUN descriptor, which the caller then owns and closes;
//! `stop` ends the service. The app's own traffic is left out of the VPN, so the engine reaches its servers
//! directly.

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

/// The interface to bring up. Addresses and routes are in CIDR form.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub addresses: Vec<String>,
    pub routes: Vec<String>,
    pub dns_servers: Vec<String>,
    pub mtu: u32,
    /// The name the system shows for the VPN.
    pub session: String,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug, Deserialize)]
struct StartResponse {
    fd: i32,
}

/// Managed by the plugin: `app.state::<Vpn<R>>()`.
pub struct Vpn<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<R>,
    _runtime: PhantomData<fn() -> R>,
}

#[cfg(target_os = "android")]
impl<R: Runtime> Vpn<R> {
    /// Blocks until the user has answered the consent dialog and the interface is up.
    pub fn start(&self, request: &StartRequest) -> Result<i32, String> {
        self.handle
            .run_mobile_plugin::<StartResponse>("start", request)
            .map(|response| response.fd)
            .map_err(|error| error.to_string())
    }

    pub fn stop(&self) -> Result<(), String> {
        self.handle
            .run_mobile_plugin::<serde_json::Value>("stop", ())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(target_os = "android"))]
impl<R: Runtime> Vpn<R> {
    pub fn start(&self, _request: &StartRequest) -> Result<i32, String> {
        Err("the VPN service exists only on Android".to_string())
    }

    pub fn stop(&self) -> Result<(), String> {
        Ok(())
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("anixops-vpn")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let vpn = Vpn {
                handle: api.register_android_plugin("io.anixops.net.vpn", "VpnPlugin")?,
                _runtime: PhantomData,
            };
            #[cfg(not(target_os = "android"))]
            let vpn = {
                let _ = api;
                Vpn::<R> {
                    _runtime: PhantomData,
                }
            };
            app.manage(vpn);
            Ok(())
        })
        .build()
}
