//! What the installer asks of this program when the client is upgraded or removed.
//!
//! The installer runs it as LocalSystem after the service has stopped and before the files are removed
//! (`installer/windows-native/Package.wxs`):
//!
//! - `--close-windows` (upgrade): ends the windows that are still open, so that their files can be replaced.
//! - `--purge` (removal): the same, and then removes the entry that starts the window after login from every
//!   signed-in user's settings. The client's data under `%ProgramData%` is kept, as the user's own.
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
