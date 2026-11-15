//! What the installer asks of this program when the client is upgraded or removed.
//!
//! The installer runs it as LocalSystem after the service has stopped and before the files are removed
//! (`installer/windows-native/Package.wxs`):
//!
//! - `--close-windows` (upgrade): ends the windows that are still open, so that their files can be replaced.
//! - `--purge` (removal): the same, and then removes the entry that starts the window after login from every
//!   signed-in user's settings, and the TUN mode's driver (Wintun) from the machine unless another program
//!   still uses it. The client's data under `%ProgramData%` is kept, as the user's own.
//!
//! The service has stopped by then and, while stopping, has had the open window put the user's proxy
//! settings back; only after that are the windows ended.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The window's program, as the installer names it.
pub const WINDOW_PROGRAM: &str = "AnixOpsNet.exe";

/// The name of the window's value under the user's Run key (`AnixOps.Net.Session.RegistryLoginEntry`).
pub const LOGIN_VALUE: &str = "AnixOpsNet";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// What the installer asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    /// End the open windows.
    CloseWindows,
    /// End the open windows and remove what the client left in the users' settings.
    Purge,
}

fn system_program(name: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join(name)
}

/// The settings of the users signed in at the machine, out of `reg query HKU`'s lines: one key per user,
/// without the system's own accounts and the keys that only hold file associations.
pub fn user_hives(listing: &str) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| {
            let name = line.rsplit('\\').next().unwrap_or_default();
            // Accounts of people (local and domain) and of Entra ID; S-1-5-18 and its like are the system's.
            (name.starts_with("S-1-5-21-") || name.starts_with("S-1-12-1-"))
                && !name.ends_with("_Classes")
        })
        .map(str::to_string)
        .collect()
}

/// Whether `reg query`'s output for the login value starts a window installed in `folder`. An entry for
/// another copy of the client (a portable one, a later installation elsewhere) is not this installation's
/// to remove.
pub fn starts_window_in(output: &str, folder: &Path) -> bool {
    let folder = folder.to_string_lossy().to_lowercase();
    let output = output.to_lowercase();
    !folder.is_empty()
        && output.contains(&format!(
            r"{}\{}",
            folder.trim_end_matches('\\'),
            WINDOW_PROGRAM.to_lowercase()
        ))
}

fn close_windows(problems: &mut Vec<String>) {
    match Command::new(system_program("taskkill.exe"))
        .args(["/F", "/IM", WINDOW_PROGRAM])
        .output()
    {
        // 128: no such process, which is the usual case.
        Ok(output) if output.status.success() || output.status.code() == Some(128) => {}
        Ok(output) => problems.push(format!(
            "the open windows could not be ended: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => problems.push(format!("taskkill could not be run: {error}")),
    }
}

fn remove_login_entries(folder: &Path, problems: &mut Vec<String>) {
    let reg = system_program("reg.exe");
    let listing = match Command::new(&reg).args(["query", "HKU"]).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => {
            problems.push(format!(
                "the users' settings could not be listed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return;
        }
        Err(error) => {
            problems.push(format!("reg could not be run: {error}"));
            return;
        }
    };
    for hive in user_hives(&listing) {
        let key = format!(r"{hive}\{RUN_KEY}");
        // A user without the value is the usual case and not a problem.
        let Ok(found) = Command::new(&reg)
            .args(["query", &key, "/v", LOGIN_VALUE])
            .output()
        else {
            continue;
        };
        if !found.status.success()
            || !starts_window_in(&String::from_utf8_lossy(&found.stdout), folder)
        {
            continue;
        }
        match Command::new(&reg)
            .args(["delete", &key, "/v", LOGIN_VALUE, "/f"])
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => problems.push(format!(
                "the login entry under {hive} could not be removed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => problems.push(format!("reg could not be run: {error}")),
        }
    }
}

/// Takes the TUN mode's driver out of the machine. Wintun itself leaves it in place while any adapter of
/// any program still uses it, which is then not a problem of this removal. Without `wintun.dll` in `folder`
/// there is nothing to ask.
fn remove_tun_driver(folder: &Path, problems: &mut Vec<String>) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{FreeLibrary, GetLastError, ERROR_SUCCESS};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
    };

    // In use by another program's adapter, or busy being installed.
    const ERROR_SHARING_VIOLATION: u32 = 32;
    const ERROR_BUSY: u32 = 170;

    let library = folder.join("wintun.dll");
    if !library.is_absolute() || !library.is_file() {
        return;
    }
    let wide: Vec<u16> = library
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated absolute path; the library is the one the installer put beside
    // this program, loaded by that path and never from the search path.
    let module = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_WITH_ALTERED_SEARCH_PATH,
        )
    };
    if module.is_null() {
        problems.push(format!(
            "wintun.dll could not be loaded: {}",
            std::io::Error::last_os_error()
        ));
        return;
    }
    // SAFETY: `module` is loaded and the name is NUL-terminated.
    match unsafe { GetProcAddress(module, c"WintunDeleteDriver".as_ptr().cast()) } {
        Some(function) => {
            // SAFETY: the Wintun export has this signature (wintun.h, Wintun 0.14): `BOOL WintunDeleteDriver(void)`.
            let delete = unsafe {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn() -> i32,
                >(function)
            };
            // SAFETY: takes nothing and reports through its result and the thread's last error.
            if unsafe { delete() } == 0 {
                // SAFETY: reads this thread's last error.
                let code = unsafe { GetLastError() };
                if !matches!(code, ERROR_SUCCESS | ERROR_SHARING_VIOLATION | ERROR_BUSY) {
                    problems.push(format!(
                        "the TUN driver could not be removed: {}",
                        std::io::Error::from_raw_os_error(code as i32)
                    ));
                }
            }
        }
        None => problems.push("wintun.dll has no WintunDeleteDriver".to_string()),
    }
    // SAFETY: `module` came from LoadLibraryExW above and nothing of it is used afterwards.
    unsafe { FreeLibrary(module) };
}

/// Does what the installer asked and says what could not be done. Nothing here stops an upgrade or a
/// removal: the installer goes on either way.
pub fn run(cleanup: Cleanup) -> Vec<String> {
    let mut problems = Vec::new();
    close_windows(&mut problems);
    if cleanup == Cleanup::Purge {
        match std::env::current_exe() {
            Ok(program) => {
                if let Some(folder) = program.parent() {
                    remove_login_entries(folder, &mut problems);
                    remove_tun_driver(folder, &mut problems);
                }
            }
            Err(error) => problems.push(format!("this program's folder is not known: {error}")),
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_settings_of_signed_in_people_are_touched() {
        let listing =
            "\r\nHKEY_USERS\\.DEFAULT\r\nHKEY_USERS\\S-1-5-19\r\nHKEY_USERS\\S-1-5-20\r\n\
            HKEY_USERS\\S-1-5-21-1004336348-1177238915-682003330-1001\r\n\
            HKEY_USERS\\S-1-5-21-1004336348-1177238915-682003330-1001_Classes\r\n\
            HKEY_USERS\\S-1-12-1-123456789-1234567890-123456789-1234567890\r\n\
            HKEY_USERS\\S-1-5-18\r\n";
        assert_eq!(
            user_hives(listing),
            [
                r"HKEY_USERS\S-1-5-21-1004336348-1177238915-682003330-1001",
                r"HKEY_USERS\S-1-12-1-123456789-1234567890-123456789-1234567890",
            ]
        );
        assert!(user_hives("").is_empty());
    }

    #[test]
    fn only_this_installations_login_entry_is_removed() {
        let folder = Path::new(r"C:\Program Files\AnixOps\Net");
        let ours = "\r\nHKEY_USERS\\S-1-5-21-1-2-3-1001\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    \
            AnixOpsNet    REG_SZ    \"C:\\Program Files\\AnixOps\\Net\\AnixOpsNet.exe\" --start-after-login\r\n";
        assert!(starts_window_in(ours, folder));
        // Windows paths do not tell upper from lower case.
        assert!(starts_window_in(&ours.to_uppercase(), folder));
        assert!(starts_window_in(
            ours,
            Path::new(r"C:\Program Files\AnixOps\Net\")
        ));
        let portable = ours.replace(r"C:\Program Files\AnixOps\Net", r"D:\Tools\AnixOps");
        assert!(!starts_window_in(&portable, folder));
        assert!(!starts_window_in("", folder));
        assert!(!starts_window_in(ours, Path::new("")));
    }
}
