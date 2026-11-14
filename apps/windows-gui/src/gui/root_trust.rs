//! The HTTPS decryption root is trusted by hand (TERMS.md section 4): the app never adds it to a Windows
//! trust store. It opens the certificate in Windows' own certificate dialog, where the user chooses
//! "Install Certificate" and the "Trusted Root Certification Authorities" store, and Windows asks once
//! more before it adds a root. Before decryption starts the app only reads whether that happened.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Keeps PowerShell and rundll32 from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The copy Windows' certificate dialog opens; `.crt` is the extension Explorer knows.
const DIALOG_FILE_NAME: &str = "AnixOps-Net-HTTPS-root.crt";

/// Reads the certificate's thumbprint and looks for it in the current user's and the machine's ROOT
/// stores. Nothing is changed.
const TRUST_CHECK_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$c = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $env:ANIXOPS_ROOT_CERTIFICATE
$t = $c.Thumbprint
if ((Test-Path "Cert:\CurrentUser\Root\$t") -or (Test-Path "Cert:\LocalMachine\Root\$t")) { 'trusted' } else { 'untrusted' }"#;

/// Whether the root at `certificate` is in a Windows ROOT store.
pub fn is_trusted(certificate: &Path) -> Result<bool, String> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            TRUST_CHECK_SCRIPT,
        ])
        .env("ANIXOPS_ROOT_CERTIFICATE", certificate)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("the certificate trust check could not run: {error}"))?;
    let answer = String::from_utf8_lossy(&output.stdout);
    match answer.trim() {
        "trusted" => Ok(true),
        "untrusted" => Ok(false),
        _ => Err(format!(
            "the certificate trust check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

/// Opens the root in Windows' certificate dialog. Returns the file it opened.
pub fn open_dialog(certificate: &Path) -> Result<PathBuf, String> {
    let pem = fs::read(certificate)
        .map_err(|error| format!("the root certificate could not be read: {error}"))?;
    let copy = certificate
        .parent()
        .map(|directory| directory.join(DIALOG_FILE_NAME))
        .ok_or("the root certificate has no folder")?;
    fs::write(&copy, pem)
        .map_err(|error| format!("the root certificate could not be copied: {error}"))?;
    Command::new("rundll32.exe")
        .arg("cryptext.dll,CryptExtOpenCER")
        .arg(&copy)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("the Windows certificate dialog could not open: {error}"))?;
    Ok(copy)
}

/// What the user does in the dialog, for the window.
pub const STEPS: &str = "In the certificate window choose Install Certificate, then Current User, \
    then \"Place all certificates in the following store\" and Trusted Root Certification Authorities, \
    and confirm the Windows security warning";
