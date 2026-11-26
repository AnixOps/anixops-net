#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("networkcore-windows-gui requires Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    client_health::startup::mark_process_start();
    // Run by the MSI as LocalSystem, where no one can see a message box: the
    // outcome goes to the log only.
    // Run by the new package during an upgrade, around the earlier version's removal.
    if std::env::args().any(|argument| argument == "--note-upgrade") {
        gui::note_upgrade();
        return;
    }
    if std::env::args().any(|argument| argument == "--clear-upgrade-note") {
        gui::clear_upgrade_note();
        return;
    }
    if std::env::args().any(|argument| argument == "--purge") {
        if gui::removal_is_an_upgrade() {
            gui::log_upgrade_skip("--purge");
            return;
        }
        if gui::purge_managed_network_state().is_err() {
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|argument| argument == "--cleanup-startup") {
        if gui::removal_is_an_upgrade() {
            gui::log_upgrade_skip("--cleanup-startup");
            return;
        }
        if let Err(error) = gui::cleanup_current_user_startup() {
            gui::log_error(&format!("sign-in startup cleanup failed: {error}"));
            std::process::exit(1);
        }
        return;
    }
    // A second start (another click on the shortcut, or the client started for a link while it already runs)
    // hands its link, or a request to show the window, to the running client and leaves before it opens anything:
    // the running client owns the data folder, the system proxy and the connection. The single instance plugin
    // still catches a second start that comes before the running client's link receiver is up.
    if anixops_net_link::os::hand_over(&anixops_net_link::request_from_args(std::env::args_os())) {
        return;
    }
    // Before anything reads or writes the data folder: it holds subscriptions,
    // node credentials and keys, and must be closed to the other users of the
    // computer. If it cannot be, the client does not start (and does not log,
    // since the log is in that folder).
    if let Err(error) = gui::secure_data_directory() {
        gui::show_fatal_error_unlogged(&error);
        std::process::exit(1);
    }
    let debug = std::env::args().any(|argument| argument == "--debug" || argument == "-d");
    if let Err(error) = gui::run_tauri(debug) {
        gui::show_fatal_error(&error);
    }
}

#[cfg(windows)]
#[allow(dead_code)]
mod gui {
    mod actions;
    mod backdrop;
    mod capture_sessions;
    mod linked_sources;
    mod mesh;
    mod mesh_dns;
    mod mesh_firewall;
    mod mitm_plugins;
    mod owned_proxy;
    mod reconnect;
    mod root_trust;
    mod runtime_status;
    mod session_end;
    mod startup;
    mod system_programs;
    mod tauri_ui;
    mod tun_mode;
    mod ui_state;

    use self::startup::{DesktopProfileNode, DesktopState};
    use control_domain::{NodeDescriptor, NODE_METADATA_PATH, NODE_PATH_TRUNK};
    use platform_windows::managed::{
        append_managed_log, read_managed_config, read_managed_state, windows_managed_config_path,
        windows_managed_data_directory, windows_managed_log_directory, windows_managed_state_path,
        write_managed_state, write_managed_text_atomic, WindowsManagedConfig, WindowsManagedState,
        WindowsSystemProxyOwner, WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
    };
    use platform_windows::mitm_security::{
        protect_windows_managed_mitm_private_key, remove_windows_managed_mitm_private_key,
        windows_managed_mitm_private_key_path,
    };
    use platform_windows::system_integration::{
        disable_current_user_startup, migrate_legacy_current_user_startup,
        remove_all_startup_tasks, NativeWindowsSystemIntegration, WindowsSystemIntegration,
    };
    use std::env;
    use std::fs;
    use std::mem::zeroed;
    use std::path::{Path, PathBuf};
    use std::ptr::null_mut;
    use windows_data::trusted_roots;
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    const APP_TITLE: &str = "AnixOps NetworkCore";

    /// The MSI's UpgradeCode (`installer/windows/Package.wxs`), which every
    /// version of the package shares.
    const UPGRADE_CODE: &str = "{8B5A5235-4D18-4AD5-9C64-37A29C6E2D1C}";

    pub fn run_tauri(debug: bool) -> Result<(), String> {
        migrate_legacy_startup();
        tauri_ui::run(debug)
    }

    pub fn show_fatal_error(message: &str) {
        let _ = append_managed_log("gui", &format!("fatal: {message}"));
        show_fatal_error_unlogged(message);
    }

    /// For a failure before the data folder, where the log is, may be used.
    pub fn show_fatal_error_unlogged(message: &str) {
        let title = wide(APP_TITLE);
        let message = wide(message);
        unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    /// Earlier builds started from the HKCU Run key, which Windows skips now
    /// that the GUI requires administrator rights; move it to the elevated
    /// logon task. Best effort: a failure only leaves sign-in startup off.
    fn migrate_legacy_startup() {
        let Ok(executable) = env::current_exe() else {
            return;
        };
        match migrate_legacy_current_user_startup(&executable) {
            Ok(true) => {
                let _ = append_managed_log("gui", "sign-in startup moved to the logon task");
            }
            Ok(false) => {}
            Err(error) => {
                let _ = append_managed_log(
                    "gui",
                    &format!("sign-in startup migration failed: {}", error.message),
                );
            }
        }
    }

    /// Closes the data folder to everyone but the system and administrators,
    /// with what earlier versions left in it open to all users
    /// (`windows_data::data_dir::close_with_contents`). An error means the
    /// folder must not be used.
    pub fn secure_data_directory() -> Result<(), String> {
        let directory = windows_managed_data_directory();
        let closed = windows_data::data_dir::close_with_contents(&directory).map_err(|error| {
            format!(
                "The data folder {} could not be closed to the other users of this computer, \
                     so it is not used: {error}",
                directory.display()
            )
        })?;
        if closed.walked {
            let _ = append_managed_log(
                "gui",
                &format!(
                    "data folder closed to other users: {} entries reset, removed {:?}",
                    closed.reset, closed.removed
                ),
            );
            // The reset gave the HTTPS decryption key the folder's access list;
            // it keeps its own (its owner and the system only).
            let key = windows_managed_mitm_private_key_path();
            if key.is_file() {
                if let Err(error) = protect_windows_managed_mitm_private_key(&key) {
                    let _ = append_managed_log(
                        "gui",
                        &format!("HTTPS decryption key protection failed: {}", error.message),
                    );
                }
            }
        }
        Ok(())
    }

    /// Whether the MSI runs this program for the old version's removal inside
    /// an upgrade. The package schedules that removal after the new version is
    /// installed (`MajorUpgrade Schedule="afterInstallExecute"`), and the old
    /// package's uninstall actions, which before 1.0 did not tell an upgrade
    /// from a removal, run this, the new version's program. An upgrade keeps
    /// the user's sign-in startup, trusted roots and keys.
    ///
    /// The registered versions do not show it: during the old version's
    /// removal Windows lists only one. So the new package notes the upgrade
    /// in the registry while it installs its files (`--note-upgrade`) and
    /// clears the note once the old version is gone (`--clear-upgrade-note`,
    /// Package.wxs). A note left by an installation that failed is ignored
    /// once it is older than [`UPGRADE_NOTE_LIFETIME`].
    pub fn removal_is_an_upgrade() -> bool {
        use windows_sys::Win32::System::ApplicationInstallationAndServicing::MsiEnumRelatedProductsW;

        let code = wide(UPGRADE_CODE);
        // A product code is 38 characters in braces.
        let mut product = [0_u16; 39];
        let installed: Vec<String> = (0..16)
            .map_while(|index| {
                // SAFETY: `code` is NUL-terminated and `product` holds the 39 characters the call writes.
                let result = unsafe {
                    MsiEnumRelatedProductsW(code.as_ptr(), 0, index, product.as_mut_ptr())
                };
                (result == 0).then(|| String::from_utf16_lossy(&product[..38]))
            })
            .collect();
        let noted = read_upgrade_note();
        let upgrade = is_upgrade(installed.len(), noted, unix_now());
        // A failed upgrade can leave two versions registered, and every later removal is then taken
        // for an upgrade too; the log says which ones were found.
        let _ = append_managed_log(
            "gui",
            &format!(
                "versions of the package registered during removal: {}; upgrade noted at: {}",
                installed.join(", "),
                noted.map_or_else(|| "none".to_string(), |at| at.to_string())
            ),
        );
        upgrade
    }

    /// Where the new package notes an upgrade: `HKLM\Software\AnixOps\NetworkCore`, which only
    /// administrators can write, value `UpgradeStartedAt` (Unix seconds).
    const UPGRADE_NOTE_KEY: &str = r"Software\AnixOps\NetworkCore";
    const UPGRADE_NOTE_VALUE: &str = "UpgradeStartedAt";
    /// How long a note counts. The old version's removal follows the note within seconds; a
    /// note this old was left by an installation that did not finish.
    const UPGRADE_NOTE_LIFETIME: u64 = 15 * 60;

    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }

    /// Notes that this package is upgrading an earlier version.
    pub fn note_upgrade() {
        use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_LOCAL_MACHINE, REG_SZ};

        let key = wide(UPGRADE_NOTE_KEY);
        let name = wide(UPGRADE_NOTE_VALUE);
        let value = wide(&unix_now().to_string());
        // SAFETY: `key`, `name` and `value` are NUL-terminated, and the length given is `value`'s,
        // in bytes, with its NUL.
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        let _ = append_managed_log(
            "gui",
            &if result == 0 {
                "upgrade noted".to_string()
            } else {
                format!("the upgrade could not be noted: error {result}")
            },
        );
    }

    /// Clears the note once the earlier version is removed.
    pub fn clear_upgrade_note() {
        use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_LOCAL_MACHINE};

        let key = wide(UPGRADE_NOTE_KEY);
        let name = wide(UPGRADE_NOTE_VALUE);
        // SAFETY: `key` and `name` are NUL-terminated.
        let result = unsafe { RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), name.as_ptr()) };
        // 2 is ERROR_FILE_NOT_FOUND: there was no note.
        if result != 0 && result != 2 {
            let _ = append_managed_log(
                "gui",
                &format!("the upgrade note could not be cleared: error {result}"),
            );
        }
    }

    /// When the note was written, if there is one.
    fn read_upgrade_note() -> Option<u64> {
        use windows_sys::Win32::System::Registry::{
            RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
        };

        let key = wide(UPGRADE_NOTE_KEY);
        let name = wide(UPGRADE_NOTE_VALUE);
        let mut value = [0_u16; 32];
        let mut size = (value.len() * 2) as u32;
        // SAFETY: `key` and `name` are NUL-terminated, `value` holds `size` bytes, and the call
        // writes at most that many (a NUL-terminated string, as RRF_RT_REG_SZ asks).
        let result = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                value.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if result != 0 {
            return None;
        }
        let length = value.iter().position(|&unit| unit == 0).unwrap_or(0);
        String::from_utf16_lossy(&value[..length])
            .trim()
            .parse()
            .ok()
    }

    /// A removal is an upgrade when another version of the package is
    /// registered beside the one being removed, or when the new package
    /// noted an upgrade a short while ago (and not in the future, as a
    /// clock set back would make it seem).
    fn is_upgrade(installed_versions: usize, noted_at: Option<u64>, now: u64) -> bool {
        installed_versions >= 2
            || noted_at.is_some_and(|at| at <= now && now - at <= UPGRADE_NOTE_LIFETIME)
    }

    pub fn log_upgrade_skip(action: &str) {
        let _ = append_managed_log(
            "gui",
            &format!("{action} skipped: the package is being upgraded, not removed"),
        );
    }

    pub fn log_error(message: &str) {
        let _ = append_managed_log("gui", message);
    }

    /// Uninstall cleanup. The MSI runs it as LocalSystem (deferred, not
    /// impersonated) because the elevated GUI cannot start from an
    /// unelevated custom action, so it removes every user's logon task and
    /// then this account's own legacy Run entry, if any.
    pub fn cleanup_current_user_startup() -> Result<(), String> {
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        remove_all_startup_tasks().map_err(|error| error.to_string())?;
        disable_current_user_startup(&executable).map_err(|error| error.to_string())
    }

    /// What earlier versions and the settings page recorded as installed
    /// machine-wide: root certificates in the machine's store (by thumbprint)
    /// and driver packages (by INF). Older versions kept them in the managed
    /// state, the window keeps them in the desktop state; each once.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct PurgeRecords {
        machine_roots: Vec<String>,
        drivers: Vec<PathBuf>,
    }

    fn purge_records(
        managed: Option<&WindowsManagedState>,
        desktop: Option<&DesktopState>,
    ) -> PurgeRecords {
        let mut records = PurgeRecords::default();
        let roots = managed
            .into_iter()
            .flat_map(|state| {
                [
                    state.certificate_sha1.clone(),
                    state.native_mitm_certificate_sha1.clone(),
                ]
            })
            .chain(desktop.map(|state| state.certificate_sha1.clone()))
            .flatten();
        for thumbprint in roots {
            let thumbprint = thumbprint.trim().to_string();
            if !thumbprint.is_empty()
                && !records
                    .machine_roots
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(&thumbprint))
            {
                records.machine_roots.push(thumbprint);
            }
        }
        let drivers = managed
            .and_then(|state| state.driver_inf_path.clone())
            .into_iter()
            .chain(desktop.and_then(|state| state.driver_inf_path.clone()));
        for driver in drivers {
            if !records.drivers.contains(&driver) {
                records.drivers.push(driver);
            }
        }
        records
    }

    /// The HTTPS decryption roots to take out of the users' own stores: those
    /// noted when a user trusted one, and the current root, which a version
    /// before the list was kept may have had trusted.
    fn decryption_root_thumbprints(errors: &mut Vec<String>) -> Vec<String> {
        let data = windows_managed_data_directory();
        let list = fs::read_to_string(data.join(trusted_roots::LIST_FILE)).unwrap_or_default();
        let mut thumbprints = trusted_roots::thumbprints(&list);
        let certificate = data.join("mitm").join("root-ca.pem");
        if certificate.is_file() {
            match root_trust::thumbprint(&certificate) {
                Ok(thumbprint) => {
                    let thumbprint = thumbprint.to_ascii_uppercase();
                    if trusted_roots::is_thumbprint(&thumbprint)
                        && !thumbprints.contains(&thumbprint)
                    {
                        thumbprints.push(thumbprint);
                    }
                }
                Err(error) => errors.push(format!("decryption root: {error}")),
            }
        }
        thumbprints
    }

    /// Uninstall cleanup, run by the MSI as LocalSystem on a full removal (not
    /// on an upgrade): removes the Windows service earlier versions
    /// registered; the root certificates and drivers recorded as installed
    /// machine-wide; the HTTPS decryption roots out of the own certificate
    /// stores of the users signed in now (a user who is not signed in keeps
    /// it); the 组网 name resolution rules; and the decryption root's private key. It also puts back the
    /// system proxy the window set and did not put back itself
    /// (`owned_proxy::restore_for_removal`): the machine's WinHTTP proxy, and
    /// the WinINet settings of each recorded user who is signed in, when they
    /// still are the ones the window applied. The rest of the data folder
    /// stays, closed to ordinary users.
    pub fn purge_managed_network_state() -> Result<(), String> {
        let integration = NativeWindowsSystemIntegration::new();
        let state_path = windows_managed_state_path();
        let mut errors = Vec::new();
        if let Err(error) = integration.uninstall_service() {
            errors.push(format!("service: {error}"));
        }
        // The name resolution rules for the 组网 internal domain names that a window left behind (a crash, or a
        // window the removal ended), with the adapter's connection suffix.
        if let Err(error) = mesh_dns::apply(None, None) {
            errors.push(format!("mesh name resolution rules: {error}"));
        }
        let mut managed = read_managed_state(&state_path).ok();
        let mut desktop = startup::load_desktop_state().ok();
        // The system proxy the window set and could not put back (it was killed, or is connected now and goes
        // with the program): the WinHTTP proxy, and the settings of each recorded user who is signed in.
        let proxy = owned_proxy::restore_for_removal_now(desktop.as_ref(), &mut errors);
        if proxy.desktop_restored {
            if let Some(state) = desktop.as_mut() {
                state.proxy_snapshot = None;
                state.applied_proxy = None;
                if let Err(error) = startup::save_desktop_state(state) {
                    errors.push(format!("desktop state: {error}"));
                }
            }
        }
        let records = purge_records(managed.as_ref(), desktop.as_ref());
        for thumbprint in &records.machine_roots {
            if let Err(error) = integration.remove_root_certificate(thumbprint) {
                errors.push(format!("certificate {thumbprint}: {error}"));
            }
        }
        let mut reboot = false;
        for inf_path in &records.drivers {
            match integration.uninstall_driver(inf_path) {
                Ok(required) => reboot |= required,
                Err(error) => errors.push(format!("driver {}: {error}", inf_path.display())),
            }
        }
        let thumbprints = decryption_root_thumbprints(&mut errors);
        let removed_roots = if thumbprints.is_empty() {
            0
        } else {
            let users = trusted_roots::signed_in_users(&mut errors);
            trusted_roots::remove_from_users(&users, &thumbprints, &mut errors)
        };
        if let Err(error) =
            remove_windows_managed_mitm_private_key(&windows_managed_mitm_private_key_path())
        {
            errors.push(format!("decryption key: {}", error.message));
        }
        if let Some(state) = managed.as_mut() {
            state.certificate_sha1 = None;
            state.native_mitm_certificate_sha1 = None;
            state.driver_inf_path = None;
            state.driver_reboot_required = reboot;
            state.last_transition = "purged".to_string();
            if let Err(error) = write_managed_state(&state_path, state) {
                errors.push(format!("state: {error}"));
            }
        }
        if let Some(state) = desktop.as_mut() {
            if state.certificate_sha1.is_some() || state.driver_inf_path.is_some() {
                state.certificate_sha1 = None;
                state.driver_inf_path = None;
                if let Err(error) = startup::save_desktop_state(state) {
                    errors.push(format!("desktop state: {error}"));
                }
            }
        }
        // The firewall rules of the joined networks that a window left behind. Another process that holds the host
        // changes (a window that still runs) is an error line: it may be forced to end, and then its rules stay.
        // Last, because the firewall and translation cmdlets can take tens of seconds,
        // and the steps above must not wait on them.
        if let Err(error) = mesh_firewall::recover() {
            errors.push(format!("mesh firewall rules: {error}"));
        }
        let _ = append_managed_log(
            "gui",
            &format!(
                "purged: machine_roots={} drivers={} user_roots={removed_roots} \
                 proxy_users={} winhttp_proxy={} errors={errors:?}",
                records.machine_roots.len(),
                records.drivers.len(),
                proxy.users_restored,
                proxy.winhttp_restored
            ),
        );
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub(super) fn load_validated_managed_configuration(
        path: &Path,
    ) -> Result<WindowsManagedConfig, String> {
        read_managed_config(path).map_err(|error| error.to_string())
    }

    /// `health` is the client health section (integrity, cold start, redacted crash reports), appended as it is.
    fn write_diagnostic_report_at(config_path: &Path, health: &str) -> Result<PathBuf, String> {
        let mut report = String::from("AnixOps NetworkCore Windows diagnostics\n");
        report.push_str("managed_config_path=redacted\n");

        match NativeWindowsSystemIntegration::new().service_status() {
            Ok(status) => report.push_str(&format!(
                "service_state={:?} service_process_id={}\n",
                status.state, status.process_id
            )),
            Err(error) => report.push_str(&format!(
                "service_status_error={}\n",
                redact_diagnostic_text(&error.to_string())
            )),
        }

        let config = match read_managed_config(config_path) {
            Ok(config) => {
                report.push_str(&format!(
                    "managed_config_schema_version={} embedded_enabled={} native_mitm_enabled={}\n",
                    config.schema_version,
                    config
                        .embedded
                        .as_ref()
                        .is_some_and(|embedded| embedded.enabled),
                    config
                        .native_mitm
                        .as_ref()
                        .is_some_and(|native_mitm| native_mitm.enabled)
                ));
                Some(config)
            }
            Err(error) => {
                report.push_str(&format!(
                    "managed_config_error={}\n",
                    redact_diagnostic_text(&error.to_string())
                ));
                None
            }
        };

        match read_managed_state(&windows_managed_state_path()) {
            Ok(managed) => report.push_str(&format!(
                "runtime_transition={} runtime_error={} embedded_running={} embedded_listener={} embedded_error={} native_mitm_running={} native_mitm_listener={} native_mitm_error={}\n",
                managed.last_transition,
                redact_diagnostic_text(
                    &managed.last_error.unwrap_or_else(|| "none".to_string()),
                ),
                managed.embedded_running,
                managed.embedded_listener.unwrap_or_else(|| "none".to_string()),
                redact_diagnostic_text(
                    &managed
                        .embedded_last_error
                        .unwrap_or_else(|| "none".to_string()),
                ),
                managed.native_mitm_running,
                managed.native_mitm_listener.unwrap_or_else(|| "none".to_string()),
                redact_diagnostic_text(
                    &managed
                        .native_mitm_last_error
                        .unwrap_or_else(|| "none".to_string()),
                )
            )),
            Err(error) => report.push_str(&format!(
                "managed_state_error={}\n",
                redact_diagnostic_text(&error.to_string())
            )),
        }

        let mut logs = vec![
            windows_managed_log_directory().join("gui.log"),
            windows_managed_log_directory().join("tauri-gui.log"),
            windows_managed_log_directory().join("service.log"),
        ];
        if let Some(native_mitm) = config.and_then(|config| config.native_mitm) {
            logs.push(native_mitm.log_path);
        }
        logs.sort();
        logs.dedup();
        for path in logs {
            let log_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("managed-log");
            report.push_str(&format!("\n--- log: {log_name} ---\n"));
            match read_log_tail(&path, 80) {
                Ok(content) if content.is_empty() => report.push_str("(empty)\n"),
                Ok(content) => report.push_str(&redact_diagnostic_text(&content)),
                Err(error) => report.push_str(&format!(
                    "(unavailable: {})\n",
                    redact_diagnostic_text(&error)
                )),
            }
        }

        report.push_str("\n--- client health ---\n");
        report.push_str(health);

        let path = windows_managed_log_directory().join("diagnostics.log");
        write_managed_text_atomic(&path, &report).map_err(|error| error.to_string())?;
        Ok(path)
    }

    fn read_log_tail(path: &Path, line_limit: usize) -> Result<String, String> {
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let lines = content.lines().collect::<Vec<_>>();
        let start = lines.len().saturating_sub(line_limit);
        if start == lines.len() {
            return Ok(String::new());
        }
        Ok(format!("{}\n", lines[start..].join("\n")))
    }

    fn redact_diagnostic_text(value: &str) -> String {
        let mut redacted = value.to_string();
        for scheme in [
            "https://",
            "http://",
            "ss://",
            "trojan://",
            "vless://",
            "vmess://",
            "hysteria2://",
            "hy2://",
            "tuic://",
            "mierus://",
        ] {
            redacted = redact_diagnostic_url(&redacted, scheme);
        }
        for marker in [
            "password=",
            "password:",
            "token=",
            "token:",
            "access_token=",
            "access_token:",
            "refresh_token=",
            "refresh_token:",
            "secret=",
            "secret:",
            "credential=",
            "credential:",
            "authorization:",
            "api_key=",
            "api_key:",
            "private_key=",
            "private_key:",
            "uuid=",
            "uuid:",
            "\"password\":",
            "\"token\":",
            "\"access_token\":",
            "\"refresh_token\":",
            "\"secret\":",
            "\"credential\":",
            "\"authorization\":",
            "\"api_key\":",
            "\"private_key\":",
            "\"uuid\":",
        ] {
            redacted = redact_diagnostic_line_value(&redacted, marker);
        }
        redacted
    }

    fn redact_diagnostic_url(value: &str, scheme: &str) -> String {
        let mut redacted = String::new();
        let mut remaining = value;
        loop {
            let lowered = remaining.to_ascii_lowercase();
            let Some(offset) = lowered.find(scheme) else {
                redacted.push_str(remaining);
                return redacted;
            };
            redacted.push_str(&remaining[..offset]);
            redacted.push_str("[redacted-url]");
            let url_end = remaining[offset..]
                .find(|character: char| {
                    character.is_ascii_whitespace() || matches!(character, '\"' | '\'' | '<' | '>')
                })
                .unwrap_or(remaining[offset..].len());
            remaining = &remaining[offset + url_end..];
        }
    }

    fn redact_diagnostic_line_value(value: &str, marker: &str) -> String {
        let mut redacted = String::new();
        let mut remaining = value;
        loop {
            let lowered = remaining.to_ascii_lowercase();
            let Some(offset) = lowered.find(marker) else {
                redacted.push_str(remaining);
                return redacted;
            };
            let marker_end = offset + marker.len();
            redacted.push_str(&remaining[..marker_end]);
            redacted.push_str("[redacted]");
            let line_end = remaining[marker_end..]
                .find('\n')
                .map(|index| marker_end + index)
                .unwrap_or(remaining.len());
            remaining = &remaining[line_end..];
        }
    }

    /// Nodes are addressed by id, which is also their outbound in the
    /// embedded engine's profile.
    fn profile_node_options(nodes: &[NodeDescriptor]) -> Vec<DesktopProfileNode> {
        nodes
            .iter()
            .map(|node| DesktopProfileNode {
                id: node.id.clone(),
                label: profile_node_label(node),
                protocol: format!("{:?}", node.protocol),
                outbound_tag: node.id.clone(),
                trunk: is_trunk_node(node),
            })
            .collect()
    }

    /// Whether the subscription marks the node as a domestic ingress on a
    /// shared trunk (`#anixops:path=trunk`, anx FUTURE-IMPROVEMENTS F-51).
    fn is_trunk_node(node: &NodeDescriptor) -> bool {
        node.metadata
            .iter()
            .any(|entry| entry.key == NODE_METADATA_PATH && entry.value.trim() == NODE_PATH_TRUNK)
    }

    fn profile_node_label(node: &NodeDescriptor) -> String {
        let name = node.name.replace(['\r', '\n'], " ");
        format!("{name} [{}] ({:?})", node.id, node.protocol)
    }

    fn managed_config_or_default() -> Result<WindowsManagedConfig, String> {
        let path = windows_managed_config_path();
        if path.exists() {
            return read_managed_config(&path).map_err(|error| error.to_string());
        }
        Ok(WindowsManagedConfig {
            schema_version: WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
            system_proxy: None,
            system_proxy_owner: WindowsSystemProxyOwner::Desktop,
            root_certificate_path: None,
            driver_package: None,
            native_mitm: None,
            embedded: None,
        })
    }

    fn current_local_timestamp() -> String {
        let mut local_time: SYSTEMTIME = unsafe { zeroed() };
        unsafe {
            GetLocalTime(&mut local_time);
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            local_time.wYear,
            local_time.wMonth,
            local_time.wDay,
            local_time.wHour,
            local_time.wMinute,
            local_time.wSecond
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use control_domain::{Endpoint, MetadataEntry, Protocol};

        #[test]
        fn only_a_removal_beside_another_version_is_an_upgrade() {
            let now = 1_800_000_000;
            // A plain removal: one version registered, no note.
            assert!(!is_upgrade(0, None, now));
            assert!(!is_upgrade(1, None, now));
            // Two versions registered.
            assert!(is_upgrade(2, None, now));
            // The new package noted the upgrade seconds ago.
            assert!(is_upgrade(1, Some(now - 5), now));
            assert!(is_upgrade(1, Some(now - UPGRADE_NOTE_LIFETIME), now));
            // A note left by an installation that failed long ago, or dated in the future.
            assert!(!is_upgrade(1, Some(now - UPGRADE_NOTE_LIFETIME - 1), now));
            assert!(!is_upgrade(1, Some(now + 60), now));
        }

        #[test]
        fn the_purge_removes_what_either_state_file_recorded_once() {
            let managed = WindowsManagedState {
                certificate_sha1: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string()),
                native_mitm_certificate_sha1: Some(
                    "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_string(),
                ),
                driver_inf_path: Some(PathBuf::from(r"C:\Windows\INF\oem7.inf")),
                ..WindowsManagedState::default()
            };
            // The settings page records its driver in the desktop state.
            let desktop = DesktopState {
                certificate_sha1: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
                driver_inf_path: Some(PathBuf::from(r"C:\Windows\INF\oem9.inf")),
                ..DesktopState::default()
            };
            let records = purge_records(Some(&managed), Some(&desktop));
            assert_eq!(
                records.machine_roots,
                [
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
                ]
            );
            assert_eq!(
                records.drivers,
                [
                    PathBuf::from(r"C:\Windows\INF\oem7.inf"),
                    PathBuf::from(r"C:\Windows\INF\oem9.inf")
                ]
            );
            // Only the window's records (no managed state from the removed service).
            let records = purge_records(None, Some(&desktop));
            assert_eq!(records.drivers, [PathBuf::from(r"C:\Windows\INF\oem9.inf")]);
            assert_eq!(records.machine_roots.len(), 1);
            assert_eq!(purge_records(None, None), PurgeRecords::default());
        }

        #[test]
        fn diagnostics_redact_urls_and_credential_values() {
            let report = redact_diagnostic_text(
                "fetch https://user:password@example.test/sub?token=never-store\nAuthorization: Bearer never-store\nprivate_key=never-store\n{\"access_token\":\"never-store\",\"uuid\":\"never-store\"}\n",
            );

            assert!(report.contains("[redacted-url]"));
            assert!(report.contains("Authorization:[redacted]"));
            assert!(report.contains("private_key=[redacted]"));
            assert!(report.contains("\"access_token\":[redacted]"));
            assert!(!report.contains("never-store"));
            assert!(!report.contains("user:password@example.test"));
        }

        #[test]
        fn profile_nodes_are_labelled_and_addressed_by_stable_id() {
            let nodes = vec![NodeDescriptor {
                id: "primary-node".to_string(),
                name: "Primary\nnode".to_string(),
                protocol: Protocol::Shadowsocks,
                endpoint: Endpoint {
                    host: "edge.example.test".to_string(),
                    port: 443,
                },
                tags: Vec::new(),
                metadata: Vec::new(),
            }];
            let options = profile_node_options(&nodes);

            assert_eq!(
                options[0].label,
                "Primary node [primary-node] (Shadowsocks)"
            );
            assert_eq!(options[0].outbound_tag, "primary-node");
            assert!(!options[0].trunk);
        }

        #[test]
        fn subscription_trunk_mark_makes_a_trunk_node() {
            let node = |id: &str, path: Option<&str>| NodeDescriptor {
                id: id.to_string(),
                name: id.to_string(),
                protocol: Protocol::Shadowsocks,
                endpoint: Endpoint {
                    host: "edge.example.test".to_string(),
                    port: 443,
                },
                tags: Vec::new(),
                metadata: path
                    .map(|value| {
                        vec![MetadataEntry {
                            key: NODE_METADATA_PATH.to_string(),
                            value: value.to_string(),
                        }]
                    })
                    .unwrap_or_default(),
            };
            let options = profile_node_options(&[
                node("direct", None),
                node("ingress", Some("trunk")),
                node("other", Some("future-kind")),
            ]);
            let trunk: Vec<_> = options.iter().map(|node| node.trunk).collect();
            assert_eq!(trunk, [false, true, false]);
        }
    }
}
