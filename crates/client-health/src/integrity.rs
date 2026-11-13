//! Install integrity self-check.
//!
//! A release writes `integrity.sha256` beside the installed files, in the `sha256sum` format (`<hex>  <path>`,
//! paths relative to the directory of the client executable, `/` as separator). At start the client hashes
//! each listed file on a background thread and reports whether the install is complete and unmodified. It is a
//! check against damaged or partly replaced installs, not a defence against someone who can also rewrite the
//! list; the operating system's code signature covers that where there is one.
//!
//! Where the list lives:
//! - Windows (MSI and portable ZIP): `<exe dir>/integrity.sha256`.
//! - Linux (tarball and .deb): `<exe dir>/../share/anixops-net/integrity.sha256`.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub const MANIFEST_NAME: &str = "integrity.sha256";

/// The result of one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Integrity {
    /// Every listed file is present with the listed hash.
    Verified { files: usize },
    /// No list beside the executable: a development build or a package that does not ship one.
    Unverified,
    /// Files that are missing or differ, relative paths.
    Damaged {
        missing: Vec<String>,
        modified: Vec<String>,
    },
    /// The list itself could not be read or parsed.
    Invalid(String),
}

impl Integrity {
    /// `verified`, `unverified`, `damaged` or `invalid`, for the settings page and metrics.
    pub fn state(&self) -> &'static str {
        match self {
            Self::Verified { .. } => "verified",
            Self::Unverified => "unverified",
            Self::Damaged { .. } => "damaged",
            Self::Invalid(_) => "invalid",
        }
    }

    /// One line for people.
    pub fn detail(&self) -> String {
        match self {
            Self::Verified { files } => format!("{files} files match the release"),
            Self::Unverified => "no integrity list (development build)".to_string(),
            Self::Damaged { missing, modified } => {
                let mut parts = Vec::new();
                if !missing.is_empty() {
                    parts.push(format!("missing: {}", missing.join(", ")));
                }
                if !modified.is_empty() {
                    parts.push(format!("modified: {}", modified.join(", ")));
                }
                format!("reinstall the client; {}", parts.join("; "))
            }
            Self::Invalid(error) => format!("integrity list unreadable: {error}"),
        }
    }
}

/// Where the list is looked for, for the executable at `exe`.
pub fn manifest_candidates(exe: &Path) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    vec![
        dir.join(MANIFEST_NAME),
        dir.join("..")
            .join("share")
            .join("anixops-net")
            .join(MANIFEST_NAME),
    ]
}

/// Checks the install of the executable at `exe`.
pub fn check(exe: &Path) -> Integrity {
    let Some(manifest) = manifest_candidates(exe)
        .into_iter()
        .find(|path| path.is_file())
    else {
        return Integrity::Unverified;
    };
    let Some(base) = exe.parent() else {
        return Integrity::Unverified;
    };
    match std::fs::read_to_string(&manifest) {
        Ok(text) => verify(base, &text),
        Err(error) => Integrity::Invalid(error.to_string()),
    }
}

/// Checks the files listed in `manifest` (the list's text) under `base`.
pub fn verify(base: &Path, manifest: &str) -> Integrity {
    let entries = match parse(manifest) {
        Ok(entries) => entries,
        Err(error) => return Integrity::Invalid(error),
    };
    let mut missing = Vec::new();
    let mut modified = Vec::new();
    for (expected, relative) in &entries {
        let path = relative
            .split('/')
            .fold(base.to_path_buf(), |path, part| path.join(part));
        match sha256_file(&path) {
            Ok(actual) if actual == *expected => {}
            Ok(_) => modified.push(relative.clone()),
            Err(_) => missing.push(relative.clone()),
        }
    }
    if missing.is_empty() && modified.is_empty() {
        Integrity::Verified {
            files: entries.len(),
        }
    } else {
        Integrity::Damaged { missing, modified }
    }
}

/// `(lower-case hex, relative path)` pairs. Paths may not leave the base directory except through the one
/// `../` prefix the Linux layout needs.
fn parse(manifest: &str) -> Result<Vec<(String, String)>, String> {
    let mut entries = Vec::new();
    for (number, line) in manifest.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (hash, path) = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .ok_or_else(|| format!("line {} is not `<sha256>  <path>`", number + 1))?;
        let hash = hash.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("line {} has no sha256", number + 1));
        }
        let path = path.trim().replace('\\', "/");
        let inner = path.strip_prefix("../").unwrap_or(&path);
        if path.is_empty()
            || path.starts_with('/')
            || inner.split('/').any(|part| part == ".." || part.is_empty())
        {
            return Err(format!("line {} names an unsafe path", number + 1));
        }
        entries.push((hash, path));
    }
    if entries.is_empty() {
        return Err("the list is empty".to_string());
    }
    Ok(entries)
}

/// Lower-case hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        }))
}

#[cfg(test)]
mod tests {
    use super::{check, sha256_file, verify, Integrity, MANIFEST_NAME};
    use std::fs;

    #[test]
    fn listed_files_are_checked() {
        let dir =
            std::env::temp_dir().join(format!("client-health-integrity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("lib")).unwrap();
        fs::write(dir.join("app"), b"binary").unwrap();
        fs::write(dir.join("lib").join("helper.dll"), b"helper").unwrap();
        let app = sha256_file(&dir.join("app")).unwrap();
        assert_eq!(
            app,
            "9a3a45d01531a20e89ac6ae10b0b0beb0492acd7216a368aa062d1a5fecaf9cd"
        );
        let helper = sha256_file(&dir.join("lib").join("helper.dll")).unwrap();
        let list = format!("{app}  app\n{helper}  lib/helper.dll\n");

        assert_eq!(check(&dir.join("app")), Integrity::Unverified);
        fs::write(dir.join(MANIFEST_NAME), &list).unwrap();
        assert_eq!(check(&dir.join("app")), Integrity::Verified { files: 2 });

        fs::write(dir.join("lib").join("helper.dll"), b"changed").unwrap();
        fs::remove_file(dir.join("app")).unwrap();
        assert_eq!(
            verify(&dir, &list),
            Integrity::Damaged {
                missing: vec!["app".to_string()],
                modified: vec!["lib/helper.dll".to_string()]
            }
        );

        for bad in [
            "",
            "nothash  app",
            &format!("{app}  ../../etc/passwd"),
            &format!("{app}  /etc/passwd"),
        ] {
            assert!(matches!(verify(&dir, bad), Integrity::Invalid(_)), "{bad}");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
