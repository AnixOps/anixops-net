use std::env;

fn main() {
    // The target, not the host: the Windows client is also cross-compiled from
    // Linux, and the manifest (requireAdministrator) and icon must still be embedded.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=windows-app.manifest");
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app.manifest"));
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("tauri build configuration failed");
    }
}
