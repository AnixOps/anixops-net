//! The HTTPS decryption root is trusted by hand (TERMS.md section 4): the app never adds it to a Windows
//! trust store. It opens the certificate in Windows' own certificate dialog, where the user chooses
//! "Install Certificate" and the "Trusted Root Certification Authorities" store, and Windows asks once
//! more before it adds a root. Before decryption starts the app only reads whether that happened, and notes
//! the root's thumbprint once it is trusted (`windows_data::trusted_roots`), so that removing the client can
//! take it out of the user's store again.

use std::fs;
use std::path::{Path, PathBuf};

use platform_windows::managed::{append_managed_log, windows_managed_data_directory};
use windows_data::trusted_roots;

/// The copy Windows' certificate dialog opens; `.crt` is the extension Explorer knows.
const DIALOG_FILE_NAME: &str = "AnixOps-Net-HTTPS-root.crt";

/// Reads the certificate's thumbprint and looks for it in the current user's and the machine's ROOT
/// stores. Nothing is changed. Answers `trusted <thumbprint>` or `untrusted <thumbprint>`.
const TRUST_CHECK_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$c = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $env:ANIXOPS_ROOT_CERTIFICATE
$t = $c.Thumbprint
if ((Test-Path "Cert:\CurrentUser\Root\$t") -or (Test-Path "Cert:\LocalMachine\Root\$t")) { "trusted $t" } else { "untrusted $t" }"#;

/// Reads the certificate's thumbprint only.
const THUMBPRINT_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
(New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $env:ANIXOPS_ROOT_CERTIFICATE).Thumbprint"#;

fn run_script(script: &str, certificate: &Path, what: &str) -> Result<String, String> {
    let output = super::system_programs::powershell(script)?
        .env("ANIXOPS_ROOT_CERTIFICATE", certificate)
        .output()
        .map_err(|error| format!("the {what} could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "the {what} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The trust check's answer: whether the root is trusted, and its thumbprint.
fn parse_trust_answer(answer: &str) -> Option<(bool, String)> {
    let (state, thumbprint) = answer.trim().split_once(' ')?;
    let trusted = match state {
        "trusted" => true,
        "untrusted" => false,
        _ => return None,
    };
    let thumbprint = thumbprint.trim();
    trusted_roots::is_thumbprint(thumbprint).then(|| (trusted, thumbprint.to_ascii_uppercase()))
}

/// The SHA-1 thumbprint of the certificate at `certificate`, as Windows' stores name it.
pub fn thumbprint(certificate: &Path) -> Result<String, String> {
    let answer = run_script(
        THUMBPRINT_SCRIPT,
        certificate,
        "certificate thumbprint check",
    )?;
    if trusted_roots::is_thumbprint(&answer) {
        Ok(answer.to_ascii_uppercase())
    } else {
        Err(format!("the certificate thumbprint check gave {answer:?}"))
    }
}

/// Notes a root so that removing the client takes it out of the users' stores again. Roots are only ever
/// added to the list: one replaced by another is still removed. A failure is logged; it only means the root
/// may stay trusted after the client is removed.
pub fn note(thumbprint: &str) {
    let list = windows_managed_data_directory().join(trusted_roots::LIST_FILE);
    if let Err(error) = trusted_roots::note(&list, thumbprint) {
        let _ = append_managed_log(
            "gui",
            &format!("the trusted HTTPS decryption root could not be noted: {error}"),
        );
    }
}

/// Whether the root at `certificate` is in a Windows ROOT store. A trusted root is noted (see [`note`]).
pub fn is_trusted(certificate: &Path) -> Result<bool, String> {
    let answer = run_script(TRUST_CHECK_SCRIPT, certificate, "certificate trust check")?;
    let (trusted, thumbprint) = parse_trust_answer(&answer)
        .ok_or_else(|| format!("the certificate trust check gave {answer:?}"))?;
    if trusted {
        note(&thumbprint);
    }
    Ok(trusted)
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
    super::system_programs::rundll32()?
        .arg("cryptext.dll,CryptExtOpenCER")
        .arg(&copy)
        .spawn()
        .map_err(|error| format!("the Windows certificate dialog could not open: {error}"))?;
    Ok(copy)
}

/// What the user does in the dialog, for the window.
pub const STEPS: &str = "In the certificate window choose Install Certificate, then Current User, \
    then \"Place all certificates in the following store\" and Trusted Root Certification Authorities, \
    and confirm the Windows security warning";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trust_check_gives_the_state_and_the_thumbprint() {
        let thumbprint = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            parse_trust_answer(&format!("trusted {thumbprint}\r\n")),
            Some((true, thumbprint.to_ascii_uppercase()))
        );
        assert_eq!(
            parse_trust_answer(&format!("untrusted {thumbprint}")),
            Some((false, thumbprint.to_ascii_uppercase()))
        );
        assert_eq!(parse_trust_answer("trusted"), None);
        assert_eq!(parse_trust_answer("trusted ..\\Run"), None);
        assert_eq!(parse_trust_answer(&format!("maybe {thumbprint}")), None);
    }
}
