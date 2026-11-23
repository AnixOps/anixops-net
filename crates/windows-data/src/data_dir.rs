//! The client's data folder under `%ProgramData%`, closed to everyone but the system and administrators.
//!
//! `%ProgramData%` lets any user make folders in it, and a folder made there is open to all users unless told
//! otherwise: they can read what is in it and add files of their own. The data folder holds subscriptions,
//! node credentials, device keys and the HTTPS decryption key, and the client acts on what it reads there with
//! administrator or system rights. So the folder is made with an access list of its own.
//!
//! [`secure`] is for a folder that only this access list ever applied to (the service's): one that is already
//! there is used only when the system or an administrator owns it. [`close_with_contents`] is for a folder
//! that earlier versions left open (the window client's): it takes the folder and everything in it back,
//! once, and removes what someone else put there.

use std::fs;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
};

use crate::account;
pub use crate::descriptor::{describe, owner};
use crate::descriptor::{last_error, set, wide, Descriptor};

/// Owned by administrators; the system and administrators in full, for the folder and everything made in
/// it later; nothing inherited from the folder above.
const ACCESS: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// The same access list without the owner, for a folder that already has a trusted one.
const ACCESS_ONLY: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// What a file inside the folder is given when the folder is taken back.
const FILE_ACCESS: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)";

/// Written in the folder once everything in it has been taken back, so that later starts only check the
/// folder itself. It counts only when an administrator or the system made it.
pub const CLOSED_MARKER: &str = ".access-closed";

/// What [`close_with_contents`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Closed {
    /// Whether the folder's contents were gone through (the first start after an upgrade, or a folder whose
    /// access had been changed).
    pub walked: bool,
    /// Files and folders given the closed access list.
    pub reset: usize,
    /// What was removed: links out of the folder, and what someone other than the system or an administrator
    /// put in it.
    pub removed: Vec<PathBuf>,
}

/// Whether the system or the administrators group owns what `described` (from [`describe`]) is about.
pub fn trusted_owner(described: &str) -> bool {
    described.starts_with("O:BA") || described.starts_with("O:SY")
}

/// Whether `described` (from [`describe`]) is a folder as [`secure`] leaves it: owned by administrators,
/// nothing inherited from above, and the system and administrators the only ones let in.
pub fn closed(described: &str) -> bool {
    described.starts_with("O:BAD:P")
        && described.contains("(A;OICI;FA;;;SY)")
        && described.contains("(A;OICI;FA;;;BA)")
        && described.matches('(').count() == 2
}

/// Makes `path` closed to ordinary users. `Ok(false)` when something is already there.
fn create_closed(path: &Path) -> io::Result<bool> {
    let name = wide(path);
    let descriptor = Descriptor::new(ACCESS)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: `name` is NUL-terminated and `attributes` with its descriptor outlives the call.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } != 0 {
        return Ok(true);
    }
    // SAFETY: reads this thread's last error code.
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Err(last_error());
    }
    Ok(false)
}

/// Makes `path` (and the folders above it) when missing, closed to ordinary users. A folder that is
/// already there has its access list set again, unless someone other than the system or an administrator
/// owns it, which is an error.
pub fn secure(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        // The folders above are shared with other programs and keep the access the system gives them.
        fs::create_dir_all(parent)?;
    }
    if create_closed(path)? {
        return Ok(());
    }
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not a folder", path.display()),
        ));
    }
    if !trusted_owner(&describe(path)?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} was made by someone other than the system or an administrator and is not used",
                path.display()
            ),
        ));
    }
    set(
        path,
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        ACCESS_ONLY,
    )
}

fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Removes a link (symbolic link or junction) itself, never what it points to.
fn remove_link(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// Gives `path` the owner and access list in `access`. Taking ownership comes first when the access list
/// does not let administrators change it: the owner may always change the access list.
fn take_over(path: &Path, access: &str) -> io::Result<()> {
    let everything = OWNER_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | PROTECTED_DACL_SECURITY_INFORMATION;
    if set(path, everything, access).is_ok() {
        return Ok(());
    }
    set(path, OWNER_SECURITY_INFORMATION, access)?;
    set(
        path,
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        access,
    )
}

/// Owners whose files are the client's own: the system, the administrators group (the owner Windows gives
/// what an administrator's elevated program makes), the built-in administrator and this process's account.
fn trusted_owners() -> Vec<String> {
    let mut owners = vec!["BA".to_string(), "SY".to_string(), "LA".to_string()];
    owners.extend(account::current_user_sid());
    owners
}

fn owned_by(path: &Path, owners: &[String]) -> bool {
    describe(path)
        .ok()
        .as_deref()
        .and_then(owner)
        .is_some_and(|found| {
            owners
                .iter()
                .any(|trusted| trusted.eq_ignore_ascii_case(found))
        })
}

fn not_found(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

/// Makes `path` (and the folders above it) when missing, closed to ordinary users, as [`secure`] does. A
/// folder that is already there is taken back with everything in it, once: the folder and each file and
/// folder in it are owned by administrators and let in only the system and administrators, links are removed
/// (never followed), and what was owned by someone other than the system, an administrator or this account
/// is removed, since an ordinary user could have put it there for the client to read. Later starts find the
/// folder closed with [`CLOSED_MARKER`] in it and only check that.
///
/// Fails when any of it cannot be done: the caller then must not read or write the folder.
pub fn close_with_contents(path: &Path) -> io::Result<Closed> {
    if let Some(parent) = path.parent() {
        // The folders above are shared with other programs and keep the access the system gives them.
        fs::create_dir_all(parent)?;
        if is_link(&fs::symlink_metadata(parent)?) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is a link and is not used", parent.display()),
            ));
        }
    }
    let marker = path.join(CLOSED_MARKER);
    if create_closed(path)? {
        fs::write(&marker, b"")?;
        return Ok(Closed::default());
    }
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not a folder and is not used", path.display()),
        ));
    }
    let owners = trusted_owners();
    if closed(&describe(path)?) && marker.is_file() && owned_by(&marker, &owners) {
        return Ok(Closed::default());
    }

    account::enable_ownership_privileges();
    // The folder first: from here on no one else can add to it while its contents are gone through.
    take_over(path, ACCESS)?;
    let mut outcome = Closed {
        walked: true,
        ..Closed::default()
    };
    let mut folders = vec![path.to_path_buf()];
    let mut foreign_folders = Vec::new();
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if not_found(&error) => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let child = entry?.path();
            let metadata = match fs::symlink_metadata(&child) {
                Ok(metadata) => metadata,
                Err(error) if not_found(&error) => continue,
                Err(error) => return Err(error),
            };
            if is_link(&metadata) {
                remove_link(&child, &metadata)?;
                outcome.removed.push(child);
                continue;
            }
            let is_folder = metadata.is_dir();
            // Read before the owner is replaced below.
            let ours = owned_by(&child, &owners);
            let access = if is_folder { ACCESS } else { FILE_ACCESS };
            if let Err(error) = take_over(&child, access) {
                // What cannot be taken back is removed: its folder belongs to administrators by now.
                let removed = if is_folder {
                    fs::remove_dir_all(&child)
                } else {
                    fs::remove_file(&child)
                };
                if let Err(removal) = removed {
                    if !not_found(&removal) {
                        return Err(io::Error::new(
                            error.kind(),
                            format!(
                                "{} could not be closed ({error}) or removed ({removal})",
                                child.display()
                            ),
                        ));
                    }
                }
                outcome.removed.push(child);
                continue;
            }
            outcome.reset += 1;
            if is_folder {
                // Its contents are taken back too, so that the whole of it can be removed afterwards.
                folders.push(child.clone());
                if !ours {
                    foreign_folders.push(child);
                }
            } else if !ours {
                fs::remove_file(&child)?;
                outcome.removed.push(child);
            }
        }
    }
    for folder in foreign_folders {
        match fs::remove_dir_all(&folder) {
            Ok(()) => outcome.removed.push(folder),
            Err(error) if not_found(&error) => {}
            Err(error) => return Err(error),
        }
    }
    fs::write(&marker, b"")?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anixops-windows-data-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn open_to_users(described: &str) -> bool {
        described.contains(";;;WD)") || described.contains(";;;BU)") || described.contains(";;;AU)")
    }

    #[test]
    fn owners_other_than_system_and_administrators_are_not_trusted() {
        assert!(trusted_owner("O:BAD:P(A;OICI;FA;;;SY)"));
        assert!(trusted_owner("O:SYD:(A;;FA;;;WD)"));
        assert!(!trusted_owner(
            "O:S-1-5-21-1-2-3-1001D:(A;OICI;FA;;;S-1-5-21-1-2-3-1001)"
        ));
        assert!(!trusted_owner("O:BUD:(A;;FA;;;WD)"));
        assert!(!trusted_owner("D:(A;;FA;;;WD)"));
    }

    #[test]
    fn a_folder_that_lets_anyone_else_in_is_not_closed() {
        assert!(closed("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        assert!(closed("O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        assert!(!closed(
            "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)"
        ));
        assert!(!closed("O:BAD:(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        assert!(!closed("O:BAD:P(A;OICI;FA;;;SY)"));
    }

    #[test]
    fn the_folder_is_closed_to_ordinary_users_and_stays_so() {
        let root = scratch("secure");
        let dir = root.join("AnixOps").join("Net");
        secure(&dir).unwrap();
        assert!(closed(&describe(&dir).unwrap()));
        // A second start finds the folder there and leaves it closed.
        secure(&dir).unwrap();
        assert!(closed(&describe(&dir).unwrap()));
        // What is made in it later is closed too.
        let file = dir.join("state.json");
        fs::write(&file, "{}").unwrap();
        let described = describe(&file).unwrap();
        assert!(!open_to_users(&described), "{described}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_in_the_way_is_an_error() {
        let path = scratch("file");
        fs::write(&path, "").unwrap();
        assert!(secure(&path).is_err());
        assert!(close_with_contents(&path).is_err());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_new_folder_is_made_closed_and_later_starts_only_check_it() {
        let root = scratch("new");
        let dir = root.join("AnixOps").join("NetworkCore");
        let made = close_with_contents(&dir).unwrap();
        assert!(!made.walked);
        assert!(closed(&describe(&dir).unwrap()));
        assert!(dir.join(CLOSED_MARKER).is_file());
        let again = close_with_contents(&dir).unwrap();
        assert!(!again.walked);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_an_earlier_version_left_open_is_taken_back_with_its_contents() {
        let root = scratch("open");
        let dir = root.join("NetworkCore");
        let nested = dir.join("anx-license").join("o-1");
        fs::create_dir_all(&nested).unwrap();
        let state = dir.join("desktop-state.json");
        let key = nested.join("device.key");
        fs::write(&state, "{}").unwrap();
        fs::write(&key, "secret").unwrap();
        // As Windows leaves what is made under %ProgramData%: ordinary users may read it.
        for path in [&dir, &state, &key] {
            set(
                path,
                DACL_SECURITY_INFORMATION,
                "D:(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;BU)",
            )
            .unwrap();
            assert!(open_to_users(&describe(path).unwrap()));
        }

        let outcome = close_with_contents(&dir).unwrap();
        assert!(outcome.walked);
        assert!(outcome.removed.is_empty(), "{:?}", outcome.removed);
        assert!(closed(&describe(&dir).unwrap()));
        for path in [&state, &key, &nested] {
            let described = describe(path).unwrap();
            assert!(
                !open_to_users(&described),
                "{}: {described}",
                path.display()
            );
            assert_eq!(owner(&described), Some("BA"), "{described}");
        }
        // The data itself is kept.
        assert_eq!(fs::read_to_string(&key).unwrap(), "secret");
        // The next start only checks the folder.
        assert!(!close_with_contents(&dir).unwrap().walked);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_marker_someone_else_put_there_does_not_skip_the_contents() {
        let root = scratch("marker");
        let dir = root.join("NetworkCore");
        fs::create_dir_all(&dir).unwrap();
        let state = dir.join("desktop-state.json");
        fs::write(&state, "{}").unwrap();
        set(
            &state,
            DACL_SECURITY_INFORMATION,
            "D:(A;;FA;;;BA)(A;;FR;;;BU)",
        )
        .unwrap();
        fs::write(dir.join(CLOSED_MARKER), b"").unwrap();
        account::enable_ownership_privileges();
        // Closing only the folder (as the installer does) leaves the files as they were.
        take_over(&dir, ACCESS).unwrap();
        if set(&dir.join(CLOSED_MARKER), OWNER_SECURITY_INFORMATION, "O:BU").is_ok() {
            let outcome = close_with_contents(&dir).unwrap();
            assert!(outcome.walked);
            assert!(!open_to_users(&describe(&state).unwrap()));
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn links_are_removed_and_never_followed() {
        let root = scratch("links");
        let dir = root.join("NetworkCore");
        let outside = root.join("outside");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        let before = describe(&outside).unwrap();
        let link = dir.join("logs");
        // Making a link needs a right that not every account has; without it there is nothing to test.
        if std::os::windows::fs::symlink_dir(&outside, &link).is_ok() {
            let outcome = close_with_contents(&dir).unwrap();
            assert!(outcome.removed.contains(&link), "{:?}", outcome.removed);
            assert!(fs::symlink_metadata(&link).is_err());
            assert_eq!(describe(&outside).unwrap(), before);
            assert_eq!(
                fs::read_to_string(outside.join("keep.txt")).unwrap(),
                "keep"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_an_ordinary_user_put_there_is_removed() {
        let root = scratch("foreign");
        let dir = root.join("NetworkCore");
        fs::create_dir_all(&dir).unwrap();
        let planted = dir.join("redirects.json");
        let own = dir.join("desktop-state.json");
        fs::write(&planted, "{}").unwrap();
        fs::write(&own, "{}").unwrap();
        account::enable_ownership_privileges();
        // Giving a file to the users group needs the restore right, which an elevated administrator has.
        if set(&planted, OWNER_SECURITY_INFORMATION, "O:BU").is_ok() {
            let outcome = close_with_contents(&dir).unwrap();
            assert!(outcome.removed.contains(&planted), "{:?}", outcome.removed);
            assert!(!planted.exists());
            assert!(own.exists());
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
