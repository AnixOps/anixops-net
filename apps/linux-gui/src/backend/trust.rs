//! Trusting the HTTPS decryption root on a Linux desktop. Browsers read NSS databases (Chrome and Chromium the
//! user's `~/.pki/nssdb`, Firefox one per profile), which `certutil` changes without privileges; command line
//! tools read the system store, which only root can change, so that part asks through `pkexec`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use desktop_mitm::ROOT_COMMON_NAME;

/// The file name of the root in a system anchor folder.
const ANCHOR_FILE: &str = "anixops-net-https-decryption.crt";

/// A distribution's folder for local trust anchors and the command that rebuilds the store from it.
const SYSTEM_STORES: [(&str, &str); 3] = [
    ("usr/local/share/ca-certificates", "update-ca-certificates"),
    ("etc/pki/ca-trust/source/anchors", "update-ca-trust"),
    (
        "etc/ca-certificates/trust-source/anchors",
        "update-ca-trust",
    ),
];

/// What installing or removing the root reached.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// NSS databases changed.
    pub browsers: usize,
    pub system: bool,
    pub notes: Vec<String>,
}

impl Outcome {
    /// One sentence for the window.
    pub fn summary(&self, verb: &str) -> String {
        let mut parts = Vec::new();
        if self.browsers > 0 {
            parts.push(format!("{} browser certificate stores", self.browsers));
        }
        if self.system {
            parts.push("the system store".to_string());
        }
        let mut summary = if parts.is_empty() {
            format!("Root certificate not {verb} anywhere")
        } else {
            format!("Root certificate {verb}: {}", parts.join(" and "))
        };
        for note in &self.notes {
            summary.push_str("; ");
            summary.push_str(note);
        }
        summary
    }
}

/// The NSS databases under `home`: the shared one Chrome and Chromium use and every Firefox profile's
/// (also Firefox as a snap or flatpak).
pub fn nss_databases(home: &Path) -> Vec<PathBuf> {
    let mut databases = Vec::new();
    let shared = home.join(".pki/nssdb");
    if shared.join("cert9.db").is_file() {
        databases.push(shared);
    }
    for profiles in [
        ".mozilla/firefox",
        "snap/firefox/common/.mozilla/firefox",
        ".var/app/org.mozilla.firefox/.mozilla/firefox",
    ] {
        let Ok(entries) = fs::read_dir(home.join(profiles)) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join("cert9.db").is_file())
            .collect();
        found.sort();
        databases.extend(found);
    }
    databases
}

/// Where the root goes in the system store under `root` (`/` outside tests) and the command to run after.
pub fn system_anchor(root: &Path) -> Option<(PathBuf, &'static str)> {
    SYSTEM_STORES
        .iter()
        .find(|(folder, _)| root.join(folder).is_dir())
        .map(|(folder, command)| (root.join(folder).join(ANCHOR_FILE), *command))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

fn succeeded(command: &mut Command) -> Result<bool, std::io::Error> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
}

fn certutil(database: &Path, arguments: &[&str]) -> Result<bool, std::io::Error> {
    succeeded(
        Command::new("certutil")
            .arg("-d")
            .arg(format!("sql:{}", database.display()))
            .args(arguments),
    )
}

/// Runs `script` as root with `arguments` as `$1`, `$2`…; polkit asks the user.
fn as_root(script: &str, arguments: &[&str]) -> Result<bool, std::io::Error> {
    succeeded(
        Command::new("pkexec")
            .args(["sh", "-c", script, "sh"])
            .args(arguments),
    )
}

/// Adds the root to every browser database and to the system store.
pub fn install(certificate: &Path) -> Outcome {
    let mut outcome = Outcome::default();
    let certificate_text = certificate.to_string_lossy();
    let databases = home().map(|home| nss_databases(&home)).unwrap_or_default();
    for database in &databases {
        // A root left by an earlier key would shadow the new one under the same name.
        let _ = certutil(database, &["-D", "-n", ROOT_COMMON_NAME]);
        match certutil(
            database,
            &[
                "-A",
                "-n",
                ROOT_COMMON_NAME,
                "-t",
                "C,,",
                "-i",
                &certificate_text,
            ],
        ) {
            Ok(true) => outcome.browsers += 1,
            Ok(false) => {}
            Err(_) => {
                outcome.notes.push(
                    "browsers need the certutil tool (package libnss3-tools or nss-tools), or import the \
                     certificate in the browser's settings"
                        .to_string(),
                );
                break;
            }
        }
    }
    match system_anchor(Path::new("/")) {
        Some((anchor, update)) => {
            let anchor_text = anchor.to_string_lossy();
            match as_root(
                r#"install -m 644 "$1" "$2" && "$3""#,
                &[&certificate_text, &anchor_text, update],
            ) {
                Ok(true) => outcome.system = true,
                Ok(false) => outcome.notes.push(
                    "the system store was not changed (authorization was refused)".to_string(),
                ),
                Err(_) => outcome
                    .notes
                    .push("the system store needs pkexec to be changed".to_string()),
            }
        }
        None => outcome
            .notes
            .push("this distribution's system store is not known".to_string()),
    }
    outcome
}

/// Takes the root out of every browser database and, when it was installed there, the system store.
pub fn remove() -> Outcome {
    let mut outcome = Outcome::default();
    let databases = home().map(|home| nss_databases(&home)).unwrap_or_default();
    for database in &databases {
        if let Ok(true) = certutil(database, &["-D", "-n", ROOT_COMMON_NAME]) {
            outcome.browsers += 1;
        }
    }
    if let Some((anchor, update)) =
        system_anchor(Path::new("/")).filter(|(anchor, _)| anchor.exists())
    {
        match as_root(
            r#"rm -f "$1" && "$2""#,
            &[&anchor.to_string_lossy(), update],
        ) {
            Ok(true) => outcome.system = true,
            _ => outcome.notes.push(format!(
                "the system store still trusts it; remove {} as root",
                anchor.display()
            )),
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("anixops-net-trust-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_the_shared_database_and_firefox_profiles() {
        let home = scratch("nss");
        assert!(nss_databases(&home).is_empty());
        for database in [
            ".pki/nssdb",
            ".mozilla/firefox/abc.default-release",
            "snap/firefox/common/.mozilla/firefox/xyz.default",
        ] {
            fs::create_dir_all(home.join(database)).unwrap();
            fs::write(home.join(database).join("cert9.db"), b"").unwrap();
        }
        // A profile folder without a database is not one.
        fs::create_dir_all(home.join(".mozilla/firefox/Crash Reports")).unwrap();
        assert_eq!(
            nss_databases(&home),
            [
                home.join(".pki/nssdb"),
                home.join(".mozilla/firefox/abc.default-release"),
                home.join("snap/firefox/common/.mozilla/firefox/xyz.default"),
            ]
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn picks_the_distribution_anchor_folder() {
        let root = scratch("anchors");
        assert_eq!(system_anchor(&root), None);
        fs::create_dir_all(root.join("etc/pki/ca-trust/source/anchors")).unwrap();
        assert_eq!(
            system_anchor(&root),
            Some((
                root.join("etc/pki/ca-trust/source/anchors")
                    .join(ANCHOR_FILE),
                "update-ca-trust"
            ))
        );
        fs::create_dir_all(root.join("usr/local/share/ca-certificates")).unwrap();
        assert_eq!(
            system_anchor(&root).map(|(_, command)| command),
            Some("update-ca-certificates")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn summaries_say_what_was_reached() {
        let outcome = Outcome {
            browsers: 2,
            system: true,
            notes: Vec::new(),
        };
        assert_eq!(
            outcome.summary("trusted"),
            "Root certificate trusted: 2 browser certificate stores and the system store"
        );
        let outcome = Outcome {
            notes: vec!["the system store needs pkexec to be changed".to_string()],
            ..Outcome::default()
        };
        assert_eq!(
            outcome.summary("trusted"),
            "Root certificate not trusted anywhere; the system store needs pkexec to be changed"
        );
    }
}
