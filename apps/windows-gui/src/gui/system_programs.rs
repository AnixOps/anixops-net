//! The Windows programs the window starts (PowerShell for the root trust check and the name resolution rules,
//! rundll32 for the certificate dialog), by their absolute paths under the system folder: a bare `powershell.exe`
//! is looked up in the program's own folder first, where a portable copy's user could put another program that
//! would then run with the window's administrator rights.

use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// Keeps the programs from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `%SystemRoot%\System32\<relative>`.
fn system32(relative: &str) -> PathBuf {
    system32_under(std::env::var_os("SystemRoot"), relative)
}

fn system32_under(system_root: Option<OsString>, relative: &str) -> PathBuf {
    PathBuf::from(
        system_root
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| OsString::from(r"C:\Windows")),
    )
    .join("System32")
    .join(relative)
}

const POWERSHELL: &str = r"WindowsPowerShell\v1.0\powershell.exe";

/// A command that runs `script` in Windows PowerShell with no profile, no prompts and no window. Values go to the
/// script through the environment, never through its text.
pub fn powershell(script: &str) -> Command {
    let mut command = Command::new(system32(POWERSHELL));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// `rundll32.exe` with no console window.
pub fn rundll32() -> Command {
    let mut command = Command::new(system32("rundll32.exe"));
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_programs_are_started_from_the_system_folder() {
        assert_eq!(
            system32_under(Some(OsString::from(r"D:\Win")), POWERSHELL),
            PathBuf::from(r"D:\Win\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
        assert_eq!(
            system32_under(None, "rundll32.exe"),
            PathBuf::from(r"C:\Windows\System32\rundll32.exe")
        );
        assert_eq!(
            system32_under(Some(OsString::new()), POWERSHELL),
            system32_under(None, POWERSHELL)
        );
        for program in [POWERSHELL, "rundll32.exe"] {
            let path = system32(program);
            assert!(
                path.is_absolute() && path.is_file(),
                "{} exists",
                path.display()
            );
        }
    }
}
