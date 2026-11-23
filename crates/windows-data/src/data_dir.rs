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

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

use crate::account;
pub use crate::descriptor::{describe, owner};
use crate::descriptor::{last_error, wide, Descriptor};
use crate::entry::{final_child, Entry};

/// Owned by administrators; the system and administrators in full, for the folder and everything made in
/// it later; nothing inherited from the folder above.
const ACCESS: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// The same access list without the owner, for a folder that already has a trusted one.
const ACCESS_ONLY: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// What a file inside the folder is given when the folder is taken back.
const FILE_ACCESS: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)";

/// What the folder above the data folder (`%ProgramData%\AnixOps`) is given when someone other than the
/// system or an administrator owns it, or it lets others remove, rename or change what is in it: owned by
/// administrators, the system and administrators in full, users only reading and going through it.
const PARENT_ACCESS: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";

/// TrustedInstaller, which owns folders Windows itself installs. SDDL has no short name for it.
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

/// Rights over a folder that let whoever has them remove it or what is in it, rename it, or change its
/// owner or access list: `DELETE`, `WRITE_DAC`, `WRITE_OWNER`, `FILE_DELETE_CHILD`, `GENERIC_ALL` and
/// `GENERIC_WRITE`. Making files and folders in it (as `%ProgramData%` lets users do) is not among them.
const TAKEOVER_RIGHTS: u32 =
    0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x40 | 0x1000_0000 | 0x4000_0000;

/// Written in the folder once everything in it has been taken back, so that later starts only check the
/// folder itself. It counts only when an administrator or the system made it.
pub const CLOSED_MARKER: &str = ".access-closed";

/// How deep [`close_with_contents`] goes into folders in the data folder. The client's own go a few
/// levels down; deeper ones were put there by someone else, and the folder is then not used.
const MAX_DEPTH: usize = 64;

/// What [`close_with_contents`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Closed {
    /// Whether the folder's contents were gone through (the first start after an upgrade, or a folder whose
    /// access had been changed).
    pub walked: bool,
    /// Files and folders given the closed access list.
    pub reset: usize,
    /// What was removed: links out of the folder (symbolic links, junctions, and files with more than one
    /// name), and what someone other than the system or an administrator put in it.
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

fn refused(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

/// Whether `sid` (an alias such as `BA`, or a SID) is the system, the administrators group or
/// TrustedInstaller.
fn administrative(sid: &str) -> bool {
    ["SY", "BA", "S-1-5-18", "S-1-5-32-544", TRUSTED_INSTALLER]
        .iter()
        .any(|trusted| trusted.eq_ignore_ascii_case(sid))
}

/// The access mask an ACE's rights field in SDDL stands for: a number, or two-letter names run together.
/// `None` for a name not known here.
fn rights_mask(rights: &str) -> Option<u32> {
    if let Some(hex) = rights
        .strip_prefix("0x")
        .or_else(|| rights.strip_prefix("0X"))
    {
        return u32::from_str_radix(hex, 16).ok();
    }
    if !rights.is_empty() && rights.bytes().all(|byte| byte.is_ascii_digit()) {
        return rights.parse().ok();
    }
    if !rights.len().is_multiple_of(2) || !rights.is_ascii() {
        return None;
    }
    let mut mask = 0_u32;
    for at in (0..rights.len()).step_by(2) {
        mask |= match &rights[at..at + 2] {
            "GA" => 0x1000_0000,
            "GR" => 0x8000_0000,
            "GW" => 0x4000_0000,
            "GX" => 0x2000_0000,
            "RC" => 0x0002_0000,
            "SD" => 0x0001_0000,
            "WD" => 0x0004_0000,
            "WO" => 0x0008_0000,
            "CC" => 0x1,
            "DC" => 0x2,
            "LC" => 0x4,
            "SW" => 0x8,
            "RP" => 0x10,
            "WP" => 0x20,
            "DT" => 0x40,
            "LO" => 0x80,
            "CR" => 0x100,
            "FA" => 0x001F_01FF,
            "FR" => 0x0012_0089,
            "FW" => 0x0012_0116,
            "FX" => 0x0012_00A0,
            _ => return None,
        };
    }
    Some(mask)
}

/// The ACEs of the access list in `described` (from [`describe`]), each split into its fields. `None`
/// when there is no access list (everyone is let in) or it cannot be read.
fn aces(described: &str) -> Option<Vec<Vec<&str>>> {
    let dacl = &described[described.find("D:")? + 2..];
    if dacl.starts_with("NO_ACCESS_CONTROL") {
        return None;
    }
    let mut found = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (at, character) in dacl.char_indices() {
        match character {
            '(' => {
                if depth == 0 {
                    start = at + 1;
                }
                depth += 1;
            }
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    found.push(dacl[start..at].split(';').collect());
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(found)
}

/// Whether the folder `described` (from [`describe`]) is owned by someone other than the system, an
/// administrator or TrustedInstaller, or lets anyone else remove, rename or change it or what is in it.
/// Such a folder above the data folder could be used to swap the data folder or what is on the way to
/// it for a link while the elevated client works in it. What cannot be read counts as open.
fn open_to_others(described: &str) -> bool {
    if !owner(described).is_some_and(administrative) {
        return true;
    }
    let Some(aces) = aces(described) else {
        return true;
    };
    aces.iter().any(|fields| {
        let [kind, flags, rights, _, _, sid, ..] = fields.as_slice() else {
            return true;
        };
        // Deny entries only take away; inherit-only ones do not apply to the folder itself.
        let inherit_only = flags.as_bytes().chunks(2).any(|flag| flag == b"IO");
        if *kind == "D" || inherit_only || administrative(sid) {
            return false;
        }
        // Any other kind of entry (conditional, object, callback) is not read here and counts as open.
        *kind != "A" || rights_mask(rights).is_none_or(|mask| mask & TAKEOVER_RIGHTS != 0)
    })
}

/// Makes the folders above `path` when missing and gives where `path` must turn out to be once opened:
/// the folder above, opened as itself and with every link on the way to it resolved, and `path`'s name.
/// A folder above that is itself a link or other reparse point is refused. One that someone other than
/// the system, an administrator or TrustedInstaller owns, or that lets others remove, rename or change
/// what is in it (an ordinary user may have made it first, as `%ProgramData%` lets anyone do), is taken
/// over through the same handle ([`PARENT_ACCESS`]); when that fails, `path` is not used.
fn final_path_inside_parent(path: &Path) -> io::Result<String> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a folder inside another", path.display()),
        ));
    };
    // The folders above are shared with other programs and keep the access the system gives them.
    fs::create_dir_all(parent)?;
    // Never removed, so opened without asking to: a console whose current folder it is must not stop this.
    let above = Entry::open_kept(parent)?;
    if above.is_link() || above.is_reparse_point() || !above.is_folder() {
        return Err(refused(format!(
            "{} is a link or not a folder and is not used",
            parent.display()
        )));
    }
    let taken = |error: io::Error| {
        refused(format!(
            "{} is owned by or open to ordinary users and could not be taken over, so {} is not \
             used: {error}",
            parent.display(),
            path.display()
        ))
    };
    if open_to_others(&above.describe().map_err(taken)?) {
        above.take_over(PARENT_ACCESS).map_err(taken)?;
        if open_to_others(&above.describe().map_err(taken)?) {
            return Err(taken(io::Error::other("its access list did not change")));
        }
    }
    Ok(final_child(&above.final_path()?, &name.to_string_lossy()))
}

/// Opens `path` as itself, with `DELETE` when it can be had, and only when it turns out to be `expected`
/// (a path from [`final_child`]). A folder on the way that was swapped for a link after it was checked
/// leads elsewhere, and what is found there is then neither changed nor removed.
fn open_inside(path: &Path, expected: &str) -> io::Result<Entry> {
    let entry = Entry::open(path)?;
    let found = entry.final_path()?;
    if !found.eq_ignore_ascii_case(expected) {
        return Err(refused(format!(
            "{} leads to {found} and is not used",
            path.display()
        )));
    }
    Ok(entry)
}

/// Opens the folder `path` as itself, and only when it is where `expected` says: not a link, and not
/// reached through a folder above that was swapped for a link since `expected` was read.
/// Opened without asking to remove it, since it never is, so that another program holding it open (a
/// console whose current folder it is, a backup) does not stop the client from starting.
fn open_folder(path: &Path, expected: &str) -> io::Result<Entry> {
    let folder = Entry::open_kept(path)?;
    if folder.is_link() || !folder.is_folder() {
        return Err(refused(format!(
            "{} is not a folder and is not used",
            path.display()
        )));
    }
    let found = folder.final_path()?;
    if !found.eq_ignore_ascii_case(expected) {
        return Err(refused(format!(
            "{} leads to {found} and is not used",
            path.display()
        )));
    }
    Ok(folder)
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
/// owns it, which is an error, as is a link in its place. The folder is opened as itself and checked and
/// changed through that one handle, so the change never lands on what a link leads to.
pub fn secure(path: &Path) -> io::Result<()> {
    // Taking over the folder above may need them (see `close_with_contents`).
    let _privileges = account::enable_ownership_privileges();
    let expected = final_path_inside_parent(path)?;
    let made = create_closed(path)?;
    let folder = open_folder(path, &expected)?;
    if made {
        return Ok(());
    }
    if !trusted_owner(&folder.describe()?) {
        return Err(refused(format!(
            "{} was made by someone other than the system or an administrator and is not used",
            path.display()
        )));
    }
    folder.set(
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        ACCESS_ONLY,
    )
}

/// Owners whose files are the client's own: the system, the administrators group (the owner Windows gives
/// what an administrator's elevated program makes), the built-in administrator and this process's account.
fn trusted_owners() -> Vec<String> {
    let mut owners = vec!["BA".to_string(), "SY".to_string(), "LA".to_string()];
    owners.extend(account::current_user_sid());
    owners
}

fn not_found(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

/// Whether the marker in the folder was left there by the client: a file of its own with no other name.
/// A link, or a second name of a file elsewhere that the system owns, does not count.
fn marker_is_ours(marker: &Path, owners: &[String]) -> bool {
    Entry::open_kept(marker).is_ok_and(|entry| {
        !entry.is_link() && !entry.is_folder() && entry.links() == 1 && entry.owned_by(owners)
    })
}

/// Writes [`CLOSED_MARKER`] in `folder` (where [`Entry::final_path`] says `folder_final`) when it is not
/// there. The file is made by its own handle, never through a link, and checked to be in the folder
/// before it counts; one made elsewhere is removed again. A marker already there is kept only when it is
/// a file of its own (not a link or other reparse point, no second name) in the folder, owned by one of
/// `owners`; otherwise this fails, and nothing is written through it.
fn write_marker(folder: &Path, folder_final: &str, owners: &[String]) -> io::Result<()> {
    let marker = folder.join(CLOSED_MARKER);
    let expected = final_child(folder_final, CLOSED_MARKER);
    if let Some(made) = Entry::create_new(&marker)? {
        let found = made.final_path()?;
        if !found.eq_ignore_ascii_case(&expected) {
            // Made by this call, so nothing that was there before is removed.
            let removal = made.remove();
            return Err(refused(format!(
                "{} was made at {found} and is not used (removed again: {removal:?})",
                marker.display()
            )));
        }
        return Ok(());
    }
    let existing = Entry::open_kept(&marker)?;
    if existing.is_reparse_point() || existing.is_folder() || existing.links() != 1 {
        return Err(refused(format!(
            "{} is a link, a folder or a file with another name, and is not used",
            marker.display()
        )));
    }
    let found = existing.final_path()?;
    if !found.eq_ignore_ascii_case(&expected) || !existing.owned_by(owners) {
        return Err(refused(format!(
            "{} leads to {found} or is someone else's, and is not used",
            marker.display()
        )));
    }
    Ok(())
}

/// Makes `path` (and the folders above it) when missing, closed to ordinary users, as [`secure`] does. A
/// folder that is already there is taken back with everything in it, once: the folder and each file and
/// folder in it are owned by administrators and let in only the system and administrators; links are
/// removed, never followed, and so are files with more than one name, whose other names may be outside the
/// folder; and what was owned by someone other than the system, an administrator or this account is
/// removed, since an ordinary user could have put it there for the client to read. Later starts find the
/// folder closed with [`CLOSED_MARKER`] in it and only check that.
///
/// Each entry is opened once, as itself, and checked, changed or removed through that handle, so that
/// nothing an ordinary user renames or links meanwhile can turn a change meant for the folder's contents
/// onto a file elsewhere. What is removed is removed without its security being changed first.
///
/// Fails when any of it cannot be done: the caller then must not read or write the folder.
pub fn close_with_contents(path: &Path) -> io::Result<Closed> {
    // On before anything is opened: an open for backup and restore with these rights may change the owner
    // and access list of, or remove, what the access list alone would refuse. Off again on return.
    let _privileges = account::enable_ownership_privileges();
    let expected = final_path_inside_parent(path)?;
    let marker = path.join(CLOSED_MARKER);
    let owners = trusted_owners();
    if create_closed(path)? {
        let _folder = open_folder(path, &expected)?;
        write_marker(path, &expected, &owners)?;
        return Ok(Closed::default());
    }
    let folder = open_folder(path, &expected)?;
    if closed(&folder.describe()?) && marker_is_ours(&marker, &owners) {
        return Ok(Closed::default());
    }

    // The folder first: from here on no one else can add to it while its contents are gone through.
    folder.take_over(ACCESS)?;
    let mut walk = Walk {
        owners,
        outcome: Closed {
            walked: true,
            ..Closed::default()
        },
    };
    walk.contents(path, &expected, true, 0)?;
    write_marker(path, &expected, &walk.owners)?;
    Ok(walk.outcome)
}

/// Going through the data folder's contents for [`close_with_contents`].
struct Walk {
    owners: Vec<String>,
    outcome: Closed,
}

impl Walk {
    /// Goes through what is in `folder` (already taken back; `folder_final` is its [`Entry::final_path`]).
    /// With `keep`, what is the client's own is taken back and kept; without, everything is removed.
    fn contents(
        &mut self,
        folder: &Path,
        folder_final: &str,
        keep: bool,
        depth: usize,
    ) -> io::Result<()> {
        if depth >= MAX_DEPTH {
            return Err(refused(format!(
                "{} is nested too deeply and is not used",
                folder.display()
            )));
        }
        // Listed in full first, so that nothing of the folder is held open while its entries are removed.
        let names = match fs::read_dir(folder) {
            Ok(entries) => entries
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<io::Result<Vec<OsString>>>()?,
            Err(error) if not_found(&error) => return Ok(()),
            Err(error) => return Err(error),
        };
        for name in names {
            let child = folder.join(&name);
            let child_final = final_child(folder_final, &name.to_string_lossy());
            // Checked to be in the folder before anything is changed or removed through it: `child` is
            // looked up again by its path, and only its last part is opened as itself.
            let entry = match open_inside(&child, &child_final) {
                Ok(entry) => entry,
                Err(error) if not_found(&error) => continue,
                Err(error) => return Err(error),
            };
            let is_folder = entry.is_folder();
            if entry.is_link() || (!is_folder && entry.links() > 1) {
                // A link out of the folder: only this name goes. What it leads to, or the file's other
                // names, are never opened for a change, so their owner and access list stay as they are.
                entry.remove()?;
                self.outcome.removed.push(child);
                continue;
            }
            if !(keep && entry.owned_by(&self.owners)) {
                if is_folder {
                    // Taken back only so that it can be emptied: what is in it may refuse administrators.
                    entry.take_over(ACCESS)?;
                    self.contents(&child, &child_final, false, depth + 1)?;
                }
                // Its folder belongs to administrators by now, which lets them remove what is in it.
                entry.remove()?;
                self.outcome.removed.push(child);
                continue;
            }
            let access = if is_folder { ACCESS } else { FILE_ACCESS };
            if let Err(error) = entry.take_over(access) {
                if is_folder {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("{} could not be closed: {error}", child.display()),
                    ));
                }
                // A file that cannot be taken back is removed.
                entry.remove().map_err(|removal| {
                    io::Error::new(
                        error.kind(),
                        format!(
                            "{} could not be closed ({error}) or removed ({removal})",
                            child.display()
                        ),
                    )
                })?;
                self.outcome.removed.push(child);
                continue;
            }
            self.outcome.reset += 1;
            if is_folder {
                self.contents(&child, &child_final, true, depth + 1)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::set;
    use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;

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
        // In a folder of its own: the folder above may be taken over, which the temporary folder must not.
        let root = scratch("file");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("NetworkCore");
        fs::write(&path, "").unwrap();
        assert!(secure(&path).is_err());
        assert!(close_with_contents(&path).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_above_owned_by_or_open_to_ordinary_users_is_noticed() {
        // As Windows leaves a folder an administrator's elevated program makes in %ProgramData%.
        let made_by_administrator =
            "O:BAD:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIIOID;GA;;;CO)\
             (A;OICIID;0x1200a9;;;BU)(A;CIID;DCLCRPCR;;;BU)";
        assert!(!open_to_others(made_by_administrator));
        assert!(!open_to_others(PARENT_ACCESS));
        assert!(!open_to_others(&format!(
            "O:{TRUSTED_INSTALLER}D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(D;OICI;FA;;;WD)"
        )));
        // Made first by an ordinary user, who then owns it.
        assert!(open_to_others(
            "O:S-1-5-21-1-2-3-1001D:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)\
             (A;OICIID;FA;;;S-1-5-21-1-2-3-1001)"
        ));
        assert!(open_to_others("O:BUD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        // Owned by administrators but letting users remove what is in it, or change its access list.
        assert!(open_to_others(
            "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;BU)"
        ));
        assert!(open_to_others("O:BAD:P(A;OICI;FA;;;SY)(A;;DT;;;AU)"));
        assert!(open_to_others("O:BAD:P(A;OICI;FA;;;SY)(A;;WD;;;WD)"));
        assert!(open_to_others("O:BAD:P(A;OICI;FA;;;SY)(A;;0x10000;;;BU)"));
        assert!(open_to_others("O:BAD:P(A;OICI;FA;;;SY)(A;;GW;;;BU)"));
        // What is not read here counts as open.
        assert!(open_to_others("O:BAD:NO_ACCESS_CONTROL"));
        assert!(open_to_others("O:BA"));
        assert!(open_to_others("O:BAD:P(A;;ZZ;;;BU)"));
        assert!(open_to_others("O:BAD:P(XA;;FR;;;BU;(Member_of {SID(BA)}))"));
    }

    #[test]
    fn a_folder_above_owned_by_an_ordinary_user_is_taken_over() {
        let root = scratch("parent-owner");
        let parent = root.join("AnixOps");
        fs::create_dir_all(&parent).unwrap();
        let _privileges = account::enable_ownership_privileges();
        set(
            &parent,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;BU)",
        )
        .unwrap();
        // Giving the folder to the users group needs the restore right, which an elevated administrator
        // has; without it only the open access list is tested.
        let _ = set(&parent, OWNER_SECURITY_INFORMATION, "O:BU");
        assert!(open_to_others(&describe(&parent).unwrap()));
        close_with_contents(&parent.join("NetworkCore")).unwrap();
        let described = describe(&parent).unwrap();
        assert!(!open_to_others(&described), "{described}");
        assert_eq!(owner(&described), Some("BA"), "{described}");
        assert!(closed(&describe(&parent.join("NetworkCore")).unwrap()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_is_found_outside_the_folder_is_neither_changed_nor_removed() {
        let root = scratch("swapped");
        let dir = root.join("NetworkCore");
        let outside = root.join("outside");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("keep.txt");
        let alias = outside.join("alias.txt");
        fs::write(&file, "keep").unwrap();
        // A second name, which the walk removes when it is in the folder.
        fs::hard_link(&file, &alias).unwrap();
        let before = describe(&file).unwrap();
        let dir_final = Entry::open_kept(&dir).unwrap().final_path().unwrap();

        // Opened by a path that turns out to lead outside the folder: refused before anything is done.
        assert!(open_inside(&file, &final_child(&dir_final, "keep.txt")).is_err());

        // The folder swapped for a link to another folder after it was checked: the walk reads through
        // the link, finds each entry elsewhere and stops, removing and changing nothing.
        let link = root.join("swapped");
        // Making a link needs a right that not every account has; without it that part is not tested.
        if std::os::windows::fs::symlink_dir(&outside, &link).is_ok() {
            let _privileges = account::enable_ownership_privileges();
            let mut walk = Walk {
                owners: trusted_owners(),
                outcome: Closed::default(),
            };
            assert!(walk.contents(&link, &dir_final, false, 0).is_err());
            assert!(
                walk.outcome.removed.is_empty(),
                "{:?}",
                walk.outcome.removed
            );
            let mut walk = Walk {
                owners: trusted_owners(),
                outcome: Closed::default(),
            };
            assert!(walk.contents(&link, &dir_final, true, 0).is_err());
            assert!(
                walk.outcome.removed.is_empty(),
                "{:?}",
                walk.outcome.removed
            );
        }
        assert_eq!(fs::read_to_string(&file).unwrap(), "keep");
        assert!(alias.exists());
        assert_eq!(describe(&file).unwrap(), before);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_marker_that_is_a_link_or_has_a_second_name_is_refused() {
        let root = scratch("marker-refused");
        let dir = root.join("NetworkCore");
        fs::create_dir_all(&dir).unwrap();
        let dir_final = Entry::open_kept(&dir).unwrap().final_path().unwrap();
        let owners = trusted_owners();
        let elsewhere = root.join("elsewhere.txt");
        fs::write(&elsewhere, "keep").unwrap();
        let marker = dir.join(CLOSED_MARKER);

        fs::hard_link(&elsewhere, &marker).unwrap();
        assert!(write_marker(&dir, &dir_final, &owners).is_err());
        fs::remove_file(&marker).unwrap();

        // Making a link needs a right that not every account has; without it that part is not tested.
        if std::os::windows::fs::symlink_file(&elsewhere, &marker).is_ok() {
            assert!(write_marker(&dir, &dir_final, &owners).is_err());
            assert!(fs::symlink_metadata(&marker)
                .unwrap()
                .file_type()
                .is_symlink());
            fs::remove_file(&marker).unwrap();
        }
        // A link to where nothing is yet is not followed to make a file there.
        let missing = root.join("missing.txt");
        if std::os::windows::fs::symlink_file(&missing, &marker).is_ok() {
            assert!(write_marker(&dir, &dir_final, &owners).is_err());
            assert!(!missing.exists());
            fs::remove_file(&marker).unwrap();
        }
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "keep");

        // With nothing in the way it is made, and a second call keeps it.
        write_marker(&dir, &dir_final, &owners).unwrap();
        assert!(marker_is_ours(&marker, &owners));
        write_marker(&dir, &dir_final, &owners).unwrap();
        fs::remove_dir_all(&root).unwrap();
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
        let _privileges = account::enable_ownership_privileges();
        // Closing only the folder (as the installer does) leaves the files as they were.
        Entry::open_kept(&dir).unwrap().take_over(ACCESS).unwrap();
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
        let _privileges = account::enable_ownership_privileges();
        // Giving a file to the users group needs the restore right, which an elevated administrator has.
        if set(&planted, OWNER_SECURITY_INFORMATION, "O:BU").is_ok() {
            let outcome = close_with_contents(&dir).unwrap();
            assert!(outcome.removed.contains(&planted), "{:?}", outcome.removed);
            assert!(!planted.exists());
            assert!(own.exists());
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_second_name_of_a_file_elsewhere_is_removed_and_the_file_left_as_it_was() {
        let root = scratch("hardlink");
        let dir = root.join("NetworkCore");
        let embedded = dir.join("embedded");
        fs::create_dir_all(&embedded).unwrap();
        // Files outside the data folder: one as an administrator's program leaves it (owned by
        // administrators, which the walk counts as the client's own) and, where the restore right allows,
        // one given to another owner (which the walk counts as someone else's).
        let own = root.join("outside-own.txt");
        let foreign = root.join("outside-foreign.txt");
        fs::write(&own, "own").unwrap();
        fs::write(&foreign, "foreign").unwrap();
        let open = "D:(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;BU)";
        set(&own, DACL_SECURITY_INFORMATION, open).unwrap();
        set(&foreign, DACL_SECURITY_INFORMATION, open).unwrap();
        {
            let _privileges = account::enable_ownership_privileges();
            let _ = set(&foreign, OWNER_SECURITY_INFORMATION, "O:BU");
        }
        let own_before = describe(&own).unwrap();
        let foreign_before = describe(&foreign).unwrap();
        // As an ordinary user could have done while earlier versions left the folder open.
        let own_link = embedded.join("x.json");
        let foreign_link = dir.join("y.json");
        fs::hard_link(&own, &own_link).unwrap();
        fs::hard_link(&foreign, &foreign_link).unwrap();
        let state = dir.join("desktop-state.json");
        fs::write(&state, "{}").unwrap();

        let outcome = close_with_contents(&dir).unwrap();
        assert!(outcome.walked);
        for link in [&own_link, &foreign_link] {
            assert!(outcome.removed.contains(link), "{:?}", outcome.removed);
            assert!(fs::symlink_metadata(link).is_err(), "{}", link.display());
        }
        // The files outside keep their owner, access list and contents.
        assert_eq!(describe(&own).unwrap(), own_before);
        assert_eq!(describe(&foreign).unwrap(), foreign_before);
        assert_eq!(fs::read_to_string(&own).unwrap(), "own");
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "foreign");
        // What is the client's own is kept and closed.
        assert!(state.exists());
        assert!(!open_to_users(&describe(&state).unwrap()));
        assert!(closed(&describe(&embedded).unwrap()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_marker_with_a_second_name_does_not_skip_the_contents() {
        let root = scratch("marker-link");
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
        // A file the system or an administrator owns elsewhere, linked in as the marker.
        let elsewhere = root.join("elsewhere");
        fs::write(&elsewhere, b"").unwrap();
        fs::hard_link(&elsewhere, dir.join(CLOSED_MARKER)).unwrap();
        {
            let _privileges = account::enable_ownership_privileges();
            Entry::open_kept(&dir).unwrap().take_over(ACCESS).unwrap();
        }
        let outcome = close_with_contents(&dir).unwrap();
        assert!(outcome.walked);
        assert!(!open_to_users(&describe(&state).unwrap()));
        // The marker written afterwards is a file of its own.
        assert!(marker_is_ours(&dir.join(CLOSED_MARKER), &trusted_owners()));
        fs::remove_dir_all(&root).unwrap();
    }

    /// Opens `path` as a console does its current folder, or a backup or virus scan what it reads: others
    /// may read and write it, but not remove it.
    fn hold_open(path: &Path) -> fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap()
    }

    #[test]
    fn a_folder_another_program_holds_open_is_still_checked_and_closed() {
        let root = scratch("held");
        let parent = root.join("AnixOps");
        let dir = parent.join("NetworkCore");
        close_with_contents(&dir).unwrap();
        {
            let _parent = hold_open(&parent);
            let _folder = hold_open(&dir);
            // Already closed: the next start only checks it, and the service's check passes too.
            assert!(!close_with_contents(&dir).unwrap().walked);
            secure(&dir).unwrap();
            assert!(closed(&describe(&dir).unwrap()));
        }

        // Left open by an earlier version, with a file of the client's that is also held open: both are
        // taken back and kept rather than the start failing.
        let state = dir.join("desktop-state.json");
        fs::write(&state, "{}").unwrap();
        let open = "D:(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;BU)";
        set(&dir, DACL_SECURITY_INFORMATION, open).unwrap();
        set(&state, DACL_SECURITY_INFORMATION, open).unwrap();
        {
            let _folder = hold_open(&dir);
            let _state = hold_open(&state);
            let outcome = close_with_contents(&dir).unwrap();
            assert!(outcome.walked);
            assert!(outcome.removed.is_empty(), "{:?}", outcome.removed);
            assert!(closed(&describe(&dir).unwrap()));
            assert!(!open_to_users(&describe(&state).unwrap()));
        }
        assert_eq!(fs::read_to_string(&state).unwrap(), "{}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_above_that_is_a_link_is_refused() {
        let root = scratch("parent-link");
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let link = root.join("AnixOps");
        // Making a link needs a right that not every account has; without it there is nothing to test.
        if std::os::windows::fs::symlink_dir(&real, &link).is_ok() {
            assert!(close_with_contents(&link.join("NetworkCore")).is_err());
            assert!(secure(&link.join("NetworkCore")).is_err());
            assert!(!real.join("NetworkCore").exists());
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
