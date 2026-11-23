//! Trusting the HTTPS decryption root on a Linux desktop. The app only does this when the user asks for it
//! with the separate "trust root certificate" action, never when decryption is turned on (TERMS.md
//! section 4). Browsers read NSS databases (Chrome and Chromium the
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

fn certutil_command(database: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new("certutil");
    command
        .arg("-d")
        .arg(format!("sql:{}", database.display()))
        .args(arguments);
    command
}

fn certutil(database: &Path, arguments: &[&str]) -> Result<bool, std::io::Error> {
    succeeded(&mut certutil_command(database, arguments))
}

/// What `certutil` prints, when it succeeds.
fn certutil_output(database: &Path, arguments: &[&str]) -> Option<String> {
    let output = certutil_command(database, arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Most certificates under the root's name one database is cleared of: several keys generated or imported
/// over time leave several roots with the same subject, and `certutil -D` takes one at a time.
const MAX_SAME_NAME: usize = 16;

/// Deletes every certificate under the root's name from `database`; answers how many there were.
fn delete_all(database: &Path) -> usize {
    (0..MAX_SAME_NAME)
        .take_while(|_| {
            matches!(
                certutil(database, &["-D", "-n", ROOT_COMMON_NAME]),
                Ok(true)
            )
        })
        .count()
}

/// Whether `pem`, one or more PEM certificates (`certutil -L -a` prints every certificate under a name, with
/// Windows or Unix line ends), holds the certificate whose SHA-1 thumbprint is `thumbprint`.
pub fn holds(pem: &str, thumbprint: &str) -> bool {
    let mut block: Option<String> = None;
    for line in pem.lines().map(str::trim) {
        if line == "-----BEGIN CERTIFICATE-----" {
            block = Some(format!("{line}\n"));
        } else if let Some(open) = block.as_mut() {
            open.push_str(line);
            open.push('\n');
            if line == "-----END CERTIFICATE-----" {
                let done = block.take().unwrap_or_default();
                if desktop_mitm::thumbprint(&done).is_ok_and(|found| found == thumbprint) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether the anchor file at `anchor` is this root, by content: a file left there for an earlier root
/// does not make a new one trusted.
pub fn anchor_holds(anchor: &Path, thumbprint: &str) -> bool {
    fs::read_to_string(anchor).is_ok_and(|text| holds(&text, thumbprint))
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
        // Roots left by earlier keys would shadow the new one under the same name.
        delete_all(database);
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

/// Whether a browser database or the system store already holds this root, `certificate_pem`. Compared by
/// content, not by name: after the root is replaced (`import_mitm_root`, or a new key) the stores still hold
/// the old one under the same name, which does not make the new one trusted. Nothing is changed.
pub fn is_trusted(certificate_pem: &str) -> bool {
    let Ok(thumbprint) = desktop_mitm::thumbprint(certificate_pem) else {
        return false;
    };
    let databases = home().map(|home| nss_databases(&home)).unwrap_or_default();
    databases.iter().any(|database| {
        certutil_output(database, &["-L", "-n", ROOT_COMMON_NAME, "-a"])
            .is_some_and(|listed| holds(&listed, &thumbprint))
    }) || system_anchor(Path::new("/"))
        .is_some_and(|(anchor, _)| anchor_holds(&anchor, &thumbprint))
}

/// Takes the root out of every browser database and, when it was installed there, the system store.
pub fn remove() -> Outcome {
    let mut outcome = Outcome::default();
    let databases = home().map(|home| nss_databases(&home)).unwrap_or_default();
    for database in &databases {
        if delete_all(database) > 0 {
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

    /// The trust state compares the certificate, not the name it is stored under (client-core-ipc-13).
    #[test]
    fn a_store_trusts_the_root_only_when_it_holds_that_certificate() {
        let dir = scratch("content");
        let old = desktop_mitm::Root::ensure(&dir.join("old")).unwrap();
        let new = desktop_mitm::Root::ensure(&dir.join("new")).unwrap();
        let old_pem = fs::read_to_string(&old.certificate_path).unwrap();
        let new_pem = fs::read_to_string(&new.certificate_path).unwrap();
        let new_thumbprint = desktop_mitm::thumbprint(&new_pem).unwrap();
        assert_ne!(desktop_mitm::thumbprint(&old_pem).unwrap(), new_thumbprint);

        assert!(holds(&new_pem, &new_thumbprint));
        assert!(!holds(&old_pem, &new_thumbprint));
        assert!(!holds("", &new_thumbprint));
        // `certutil -L -a` lists every certificate under the name, here with Windows line ends.
        let listed = format!("{old_pem}\n{new_pem}").replace('\n', "\r\n");
        assert!(holds(&listed, &new_thumbprint));

        // The system anchor left for the earlier root does not count for the new one.
        let anchor = dir.join(ANCHOR_FILE);
        assert!(!anchor_holds(&anchor, &new_thumbprint));
        fs::write(&anchor, &old_pem).unwrap();
        assert!(!anchor_holds(&anchor, &new_thumbprint));
        fs::write(&anchor, &new_pem).unwrap();
        assert!(anchor_holds(&anchor, &new_thumbprint));

        // Not a certificate: nothing is trusted.
        assert!(!is_trusted("not a certificate"));
        let _ = fs::remove_dir_all(dir);
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
