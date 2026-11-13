//! AnixOps Net for Android. The window and the backend are the Linux client's (`networkcore_linux_gui::app`);
//! this crate adds the VpnService plugin and tells the window where the app keeps its data.

#[cfg(target_os = "android")]
mod android {
    use std::path::PathBuf;

    use networkcore_linux_gui::app::{self, Host, TunnelHost, TunnelInterface};
    use tauri::{AppHandle, Manager, Wry};
    use tauri_plugin_anixops_vpn::{StartRequest, Vpn};

    /// The application id in tauri.conf.json; the private data directory is named after it.
    const PACKAGE: &str = "io.anixops.net.android";

    /// The app's private files directory. The main process is named after its package, which also covers a
    /// build with a different application id.
    fn data_dir() -> PathBuf {
        let package = std::fs::read("/proc/self/cmdline")
            .ok()
            .and_then(|cmdline| {
                let name = cmdline.split(|byte| *byte == 0).next()?.to_vec();
                String::from_utf8(name).ok()
            })
            .map(|name| name.split(':').next().unwrap_or_default().to_string())
            .filter(|name| !name.is_empty() && !name.contains('/'))
            .unwrap_or_else(|| PACKAGE.to_string());
        PathBuf::from("/data/data")
            .join(package)
            .join("files")
            .join("anixops-net")
    }

    struct VpnHost;

    impl TunnelHost for VpnHost {
        fn open(&self, app: &AppHandle, interface: &TunnelInterface) -> Result<i32, String> {
            app.state::<Vpn<Wry>>().start(&StartRequest {
                addresses: interface.addresses.clone(),
                routes: interface.routes.clone(),
                dns_servers: interface.dns_servers.clone(),
                mtu: interface.mtu,
                session: "AnixOps Net".to_string(),
            })
        }

        fn close(&self, app: &AppHandle) {
            let _ = app.state::<Vpn<Wry>>().stop();
        }
    }

    pub fn run() {
        app::mark_process_start();
        let dir = data_dir();
        let host = Host {
            executable: std::env::current_exe().unwrap_or_else(|_| dir.clone()),
            dir,
            plugins: vec![tauri_plugin_anixops_vpn::init()],
            tunnel: Some(Box::new(VpnHost)),
        };
        if let Err(error) = app::run(tauri::generate_context!(), host) {
            eprintln!("anixops-net: {error}");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "android")]
    android::run();
}
