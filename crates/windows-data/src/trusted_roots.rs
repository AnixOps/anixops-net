//! The HTTPS decryption roots users trusted by hand, so that removing the client can take them out again.
//!
//! The client never adds a root to a trust store itself: the user trusts it in Windows' certificate dialog,
//! into their own store (`Cert:\CurrentUser\Root`). The client notes each root's thumbprint in a list in its
//! data folder, one per line; the removal, which runs as LocalSystem, deletes those certificates from the
//! stores of the users signed in at the time. Deleting needs no confirmation from the user, unlike adding. A
//! user who is not signed in keeps the root.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The list's name in the data folder (`client_core::backend::trusted_roots_path` uses the same).
pub const LIST_FILE: &str = "trusted-roots.txt";

/// Where Windows keeps a user's own trusted roots, one key per certificate, named by its thumbprint.
const USER_ROOTS_KEY: &str = r"Software\Microsoft\SystemCertificates\Root\Certificates";

/// A program of Windows itself, by its full path.
pub fn system_program(name: &str) -> PathBuf {
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

/// Whether `value` is a certificate thumbprint (SHA-1, 40 hexadecimal digits).
pub fn is_thumbprint(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|digit| digit.is_ascii_hexdigit())
}

/// The thumbprints in a list of trusted roots: only lines that are a thumbprint, since each becomes part of
/// a registry key's name. Upper case, each once.
pub fn thumbprints(list: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in list
        .lines()
        .map(str::trim)
        .filter(|line| is_thumbprint(line))
    {
        let thumbprint = line.to_ascii_uppercase();
        if !found.contains(&thumbprint) {
            found.push(thumbprint);
        }
    }
    found
}

/// Adds `thumbprint` to the list at `list` unless it is there already. Lines are only ever added, so a root
/// that was replaced (by the user's own CA, say) is still removed with the client. Returns whether it was
/// added.
pub fn note(list: &Path, thumbprint: &str) -> io::Result<bool> {
    if !is_thumbprint(thumbprint) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a certificate thumbprint",
        ));
    }
    let known = match std::fs::read_to_string(list) {
        Ok(known) => known,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if thumbprints(&known)
        .iter()
        .any(|line| line.eq_ignore_ascii_case(thumbprint))
    {
        return Ok(false);
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(list)?;
    let separator = if known.is_empty() || known.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    file.write_all(format!("{separator}{}\n", thumbprint.to_ascii_uppercase()).as_bytes())?;
    Ok(true)
}

/// The settings of the users signed in now, as `reg` names them.
pub fn signed_in_users(problems: &mut Vec<String>) -> Vec<String> {
    match Command::new(system_program("reg.exe"))
        .args(["query", "HKU"])
        .output()
    {
        Ok(output) if output.status.success() => {
            user_hives(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(output) => {
            problems.push(format!(
                "the users' settings could not be listed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            Vec::new()
        }
        Err(error) => {
            problems.push(format!("reg could not be run: {error}"));
            Vec::new()
        }
    }
}

/// Takes the roots with these thumbprints out of the own certificate stores of `users` (from
/// [`signed_in_users`]). Returns how many were removed.
pub fn remove_from_users(
    users: &[String],
    thumbprints: &[String],
    problems: &mut Vec<String>,
) -> usize {
    let reg = system_program("reg.exe");
    let mut removed = 0;
    for thumbprint in thumbprints.iter().filter(|value| is_thumbprint(value)) {
        for hive in users {
            let key = format!(r"{hive}\{USER_ROOTS_KEY}\{thumbprint}");
            // A user who never trusted it is the usual case and not a problem.
            let Ok(found) = Command::new(&reg).args(["query", &key]).output() else {
                continue;
            };
            if !found.status.success() {
                continue;
            }
            match Command::new(&reg).args(["delete", &key, "/f"]).output() {
                Ok(output) if output.status.success() => removed += 1,
                Ok(output) => problems.push(format!(
                    "the root certificate under {hive} could not be removed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )),
                Err(error) => problems.push(format!("reg could not be run: {error}")),
            }
        }
    }
    removed
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
    fn only_thumbprints_become_registry_key_names() {
        let list = "0123456789ABCDEF0123456789abcdef01234567\r\n\r\n..\\..\\Run\nABC\n             FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF \n0123456789ABCDEF0123456789ABCDEF0123456G\n0123456789abcdef0123456789abcdef01234567\n";
        assert_eq!(
            thumbprints(list),
            [
                "0123456789ABCDEF0123456789ABCDEF01234567",
                "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF",
            ]
        );
        assert!(thumbprints("").is_empty());
    }

    #[test]
    fn noting_only_adds_and_keeps_replaced_roots() {
        let list = std::env::temp_dir().join(format!(
            "anixops-windows-data-roots-{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&list);
        let first = "0123456789abcdef0123456789abcdef01234567";
        let second = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF";
        assert!(note(&list, first).unwrap());
        assert!(!note(&list, &first.to_ascii_uppercase()).unwrap());
        // A root replaced by another: both stay listed.
        assert!(note(&list, second).unwrap());
        assert!(note(&list, r"..\..\Run").is_err());
        assert_eq!(
            thumbprints(&std::fs::read_to_string(&list).unwrap()),
            [first.to_ascii_uppercase(), second.to_string()]
        );
        // A list without a final line break (written by hand) still gets one line per root.
        std::fs::write(&list, first).unwrap();
        assert!(note(&list, second).unwrap());
        assert_eq!(
            thumbprints(&std::fs::read_to_string(&list).unwrap()).len(),
            2
        );
        std::fs::remove_file(&list).unwrap();
    }
}
