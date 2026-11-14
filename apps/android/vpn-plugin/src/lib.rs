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

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequest<'a> {
    name: &'a str,
    contents: &'a str,
    /// `contents` is base64 of a binary file.
    base64: bool,
    mime_type: &'a str,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug, Deserialize)]
struct SaveResponse {
    location: String,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyResponse {
    key_base64: String,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct NetworkRequest {
    ask: bool,
}

/// The network the device is on: `kind` is wifi, cellular, other or offline; `ssid` the Wi-Fi name when Android
/// gives it.
#[derive(Debug, Clone, Deserialize)]
pub struct NetworkResponse {
    pub kind: String,
    #[serde(default)]
    pub ssid: Option<String>,
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

    /// Saves a text file to the shared Downloads folder and returns where it went ("Download/<name>").
    pub fn save_to_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        self.handle
            .run_mobile_plugin::<SaveResponse>(
                "saveToDownloads",
                SaveRequest {
                    name,
                    contents,
                    base64: false,
                    mime_type: text_mime_type(name),
                },
            )
            .map(|response| response.location)
            .map_err(|error| error.to_string())
    }

    /// Saves a binary file, given as base64, to the shared Downloads folder and returns where it went.
    pub fn save_base64_to_downloads(
        &self,
        name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<String, String> {
        self.handle
            .run_mobile_plugin::<SaveResponse>(
                "saveToDownloads",
                SaveRequest {
                    name,
                    contents: data_base64,
                    base64: true,
                    mime_type,
                },
            )
            .map(|response| response.location)
            .map_err(|error| error.to_string())
    }

    /// The key that seals saved capture sessions, as base64. The Android Keystore wraps it; the plugin makes it
    /// on first use and never replaces one it cannot unwrap.
    pub fn capture_session_key(&self) -> Result<String, String> {
        self.handle
            .run_mobile_plugin::<KeyResponse>("captureSessionKey", ())
            .map(|response| response.key_base64)
            .map_err(|error| error.to_string())
    }

    /// The current network; `ask` asks for the location permission when the Wi-Fi name is withheld.
    pub fn current_network(&self, ask: bool) -> Result<NetworkResponse, String> {
        self.handle
            .run_mobile_plugin::<NetworkResponse>("currentNetwork", NetworkRequest { ask })
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

    pub fn save_to_downloads(&self, _name: &str, _contents: &str) -> Result<String, String> {
        Err("Downloads is an Android folder".to_string())
    }

    pub fn save_base64_to_downloads(
        &self,
        _name: &str,
        _mime_type: &str,
        _data_base64: &str,
    ) -> Result<String, String> {
        Err("Downloads is an Android folder".to_string())
    }

    pub fn capture_session_key(&self) -> Result<String, String> {
        Err("the Android Keystore exists only on Android".to_string())
    }

    pub fn current_network(&self, _ask: bool) -> Result<NetworkResponse, String> {
        Err("the network is read on Android".to_string())
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

/// The type a text file is saved with: a report or log is plain text, anything else the root certificate.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn text_mime_type(name: &str) -> &'static str {
    if name.ends_with(".log") || name.ends_with(".txt") {
        "text/plain"
    } else {
        "application/x-x509-ca-cert"
    }
}
