//! The Windows programs the window starts (PowerShell for the root trust check and the name resolution rules,
//! rundll32 for the certificate dialog), by their absolute paths in the system directory as Windows itself reports
//! it (`GetSystemDirectoryW`). A bare `powershell.exe` is looked up in the program's own folder first, and
//! `%SystemRoot%` comes from the environment, which the same user can change at will: either would let a program
//! that user put in place run with the window's administrator rights.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

/// Keeps the programs from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const POWERSHELL: &str = r"WindowsPowerShell\v1.0\powershell.exe";
const RUNDLL32: &str = "rundll32.exe";

/// The system directory (`C:\Windows\System32`), from Windows rather than from the environment.
fn system_directory() -> Result<PathBuf, String> {
    let mut buffer = vec![0u16; 260];
    loop {
        let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: the buffer holds `capacity` UTF-16 units; the call writes at most that many.
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), capacity) } as usize;
        if length == 0 {
            return Err(format!(
                "the system directory could not be read: {}",
                std::io::Error::last_os_error()
            ));
        }
        if length < buffer.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buffer[..length])));
        }
        // Too small: `length` is the size needed, terminator included.
        buffer.resize(length, 0);
    }
}

fn program(relative: &str) -> Result<Command, String> {
    let mut command = Command::new(system_directory()?.join(relative));
    command.creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}

/// A command that runs `script` in Windows PowerShell with no profile, no prompts and no window. Values go to the
/// script through the environment, never through its text.
pub fn powershell(script: &str) -> Result<Command, String> {
    let mut command = program(POWERSHELL)?;
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    Ok(command)
}

/// `rundll32.exe` with no console window.
pub fn rundll32() -> Result<Command, String> {
    program(RUNDLL32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_programs_are_started_from_the_system_directory() {
        let directory = system_directory().expect("the system directory");
        assert!(directory.is_absolute(), "{}", directory.display());
        assert!(
            directory
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("System32")),
            "{}",
            directory.display()
        );
        for relative in [POWERSHELL, RUNDLL32] {
            let path = directory.join(relative);
            assert!(path.is_file(), "{} exists", path.display());
        }
        assert_eq!(
            powershell("'ok'").unwrap().get_program(),
            directory.join(POWERSHELL).as_os_str()
        );
    }
}
