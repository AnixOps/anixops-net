#[cfg(all(feature = "gui", target_os = "linux"))]
fn main() {
    use std::path::PathBuf;

    use networkcore_linux_gui::app::{self, Host};
    use networkcore_linux_gui::backend::state::Store;
    use networkcore_linux_gui::backend::tun_helper;

    // TUN mode's helper: this program again, started through pkexec; it makes the device and exits.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(tun_helper::HELPER_FLAG) {
        std::process::exit(tun_helper::run_helper(&args[1..]));
    }

    client_health::startup::mark_process_start();
    let Some(dir) = Store::default_dir() else {
        eprintln!("anixops-net: HOME is not set");
        std::process::exit(1);
    };
    // Inside an AppImage the running file is a temporary mount; autostart must point at the image itself.
    let executable = match std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
    {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("anixops-net: {error}");
            std::process::exit(1);
        }
    };
    let host = Host {
        dir,
        executable,
        plugins: Vec::new(),
        tunnel: None,
    };
    if let Err(error) = app::run(tauri::generate_context!(), host) {
        eprintln!("anixops-net: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(all(feature = "gui", target_os = "linux")))]
fn main() {
    eprintln!("anixops-net: the window is built on Linux with `--features gui`");
    std::process::exit(1);
}
