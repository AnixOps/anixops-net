//! A file or folder opened once by handle, then checked and changed through that handle.
//!
//! A path is looked up again on every call that takes one, and a call that changes security follows links.
//! So a check made through one path call and a change made through another can land on different files when
//! someone else can rename or link things in between, and a change can land on whatever a link leads to.
//! An [`Entry`] is opened as itself, never through the link it may be (`FILE_FLAG_OPEN_REPARSE_POINT`), and
//! what is read and changed afterwards is read and changed on that same open file.

use std::io;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_EXISTS, ERROR_SHARING_VIOLATION, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileAttributeTagInfo, FileDispositionInfo, FileDispositionInfoEx,
    GetFileInformationByHandle, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
    SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, CREATE_NEW, DELETE,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL, SYNCHRONIZE, VOLUME_NAME_DOS,
    WRITE_DAC, WRITE_OWNER,
};

use crate::descriptor::{describe_handle, last_error, owner, set_handle, wide};

/// What every open asks for: reading the attributes and the link count.
const BASE: u32 = FILE_READ_ATTRIBUTES | SYNCHRONIZE;

/// Reading and changing its owner and access list. None of these is checked against how others share an
/// open file, so another program holding it open never makes asking for them fail.
const SECURITY: u32 = READ_CONTROL | WRITE_DAC | WRITE_OWNER;

/// The access asked for, most first, for an entry that may have to be removed. The first is what an
/// elevated administrator gets for anything once the backup and restore rights are on (see
/// `account::enable_ownership_privileges`). The second is the same without `DELETE`, for an entry another
/// program holds open without letting it be removed (a console whose current folder it is, a backup or a
/// virus scan): it can still be checked and taken back, and only removing it then fails. The later ones are
/// for a process without those rights, which can then still read or remove what it may.
const REMOVABLE: [u32; 6] = [
    BASE | SECURITY | DELETE,
    BASE | SECURITY,
    BASE | READ_CONTROL | DELETE,
    BASE | DELETE,
    BASE | READ_CONTROL,
    BASE,
];

/// The access asked for, most first, for an entry that is never removed (the data folder and the folder
/// above it): no `DELETE`, so another program holding it open cannot make the open fail.
const KEPT: [u32; 3] = [BASE | SECURITY, BASE | READ_CONTROL, BASE];

/// Reparse tags with this bit set name another file or folder (symbolic links, junctions and the like).
/// Others (compressed, deduplicated or cloud files) only change how the data is stored.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// A file or folder opened as itself, closed when dropped.
pub(crate) struct Entry {
    handle: HANDLE,
    access: u32,
    attributes: u32,
    links: u32,
    /// Whether it is a reparse point that names another file or folder.
    surrogate: bool,
}

impl Drop for Entry {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

impl Entry {
    /// Opens `path` itself, with `DELETE` when it can be had, so that it may be removed: a symbolic link
    /// or junction is opened as the link, never followed. Others may keep reading, writing and removing it
    /// meanwhile, as they could before.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Self::open_with(path, &REMOVABLE)
    }

    /// Opens `path` itself as [`Entry::open`] does, but never asks to remove it, so another program holding
    /// it open, even without letting it be removed, does not make this fail. [`Entry::remove`] then fails.
    pub(crate) fn open_kept(path: &Path) -> io::Result<Self> {
        Self::open_with(path, &KEPT)
    }

    /// Tries each access in `attempts` in turn. A refusal (access denied, or a sharing conflict with how
    /// another program holds it open) moves to the next; any other error is returned at once.
    fn open_with(path: &Path, attempts: &[u32]) -> io::Result<Self> {
        let name = wide(path);
        let mut refusal = None;
        for &access in attempts {
            // SAFETY: `name` is NUL-terminated; no security attributes or template are passed.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                let error = last_error();
                let code = error.raw_os_error();
                if code == Some(ERROR_ACCESS_DENIED as i32)
                    || code == Some(ERROR_SHARING_VIOLATION as i32)
                {
                    // The first refusal says most about why the fullest access was not had.
                    if refusal.is_none() {
                        refusal = Some(error);
                    }
                    continue;
                }
                return Err(error);
            }
            return Self::from_handle(handle, access);
        }
        Err(refusal.unwrap_or_else(|| io::Error::from(io::ErrorKind::PermissionDenied)))
    }

    /// Makes the file `path`, empty, and opens it as itself with `DELETE`, so that it can be removed again
    /// when it turns out to be somewhere else. `Ok(None)` when something (a file, a folder or a link, which
    /// is never followed) already has that name; nothing is then opened or changed.
    pub(crate) fn create_new(path: &Path) -> io::Result<Option<Self>> {
        let name = wide(path);
        let access = BASE | READ_CONTROL | DELETE;
        // SAFETY: `name` is NUL-terminated; no security attributes or template are passed, so the file
        // gets the access list its folder gives what is made in it.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ,
                null(),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let error = last_error();
            if error.raw_os_error() == Some(ERROR_FILE_EXISTS as i32) {
                return Ok(None);
            }
            return Err(error);
        }
        Self::from_handle(handle, access).map(Some)
    }

    /// Takes over `handle`, opened with `access`, and reads what it is.
    fn from_handle(handle: HANDLE, access: u32) -> io::Result<Self> {
        // Closed when dropped, also when reading what it is fails below.
        let mut entry = Self {
            handle,
            access,
            attributes: 0,
            links: 0,
            surrogate: false,
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is open and `information` is valid for the structure written.
        if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
            return Err(last_error());
        }
        entry.attributes = information.dwFileAttributes;
        entry.links = information.nNumberOfLinks;
        if entry.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
            // SAFETY: the handle is open and `tag` is the structure this class takes, of the size passed.
            if unsafe {
                GetFileInformationByHandleEx(
                    handle,
                    FileAttributeTagInfo,
                    (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                    std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                )
            } == 0
            {
                return Err(last_error());
            }
            entry.surrogate = tag.ReparseTag & NAME_SURROGATE != 0;
        }
        Ok(entry)
    }

    /// Whether it is a symbolic link, junction or other reparse point that leads to another file or
    /// folder. A compressed, deduplicated or cloud file is a reparse point too, but holds its own data and
    /// is not a link.
    pub(crate) fn is_link(&self) -> bool {
        self.surrogate
    }

    /// Whether it is a reparse point of any kind, a link or not.
    pub(crate) fn is_reparse_point(&self) -> bool {
        self.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    pub(crate) fn is_folder(&self) -> bool {
        self.attributes & FILE_ATTRIBUTE_DIRECTORY != 0
    }

    /// How many names the file has. More than one means it is also somewhere else, which may be outside
    /// the folder it was found in.
    pub(crate) fn links(&self) -> u32 {
        self.links
    }

    /// Where it is, with every link and short name on the way resolved, such as
    /// `\\?\C:\ProgramData\AnixOps\NetworkCore`.
    pub(crate) fn final_path(&self) -> io::Result<String> {
        let mut buffer = vec![0_u16; 512];
        for _ in 0..4 {
            // SAFETY: the handle is open and `buffer` is valid for writes of the length passed.
            let length = unsafe {
                GetFinalPathNameByHandleW(
                    self.handle,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                )
            } as usize;
            if length == 0 {
                return Err(last_error());
            }
            if length < buffer.len() {
                return Ok(String::from_utf16_lossy(&buffer[..length]));
            }
            // Too small: `length` is the size needed, the terminator included.
            buffer.resize(length + 1, 0);
        }
        Err(io::Error::other("the path kept growing while it was read"))
    }

    /// Its owner and access list in their text form, as `descriptor::describe` gives them for a path.
    pub(crate) fn describe(&self) -> io::Result<String> {
        describe_handle(self.handle)
    }

    /// Whether one of `owners` (aliases such as `BA`, or SIDs) owns it. `false` when the owner cannot be read.
    pub(crate) fn owned_by(&self, owners: &[String]) -> bool {
        self.describe()
            .ok()
            .as_deref()
            .and_then(owner)
            .is_some_and(|found| {
                owners
                    .iter()
                    .any(|trusted| trusted.eq_ignore_ascii_case(found))
            })
    }

    /// Sets the parts of its security that `information` names from the descriptor `text`.
    pub(crate) fn set(
        &self,
        information: OBJECT_SECURITY_INFORMATION,
        text: &str,
    ) -> io::Result<()> {
        set_handle(self.handle, information, text)
    }

    /// Gives it the owner and access list in `access`. Taking ownership comes first when the access list
    /// does not let administrators change it: the owner may always change the access list.
    pub(crate) fn take_over(&self, access: &str) -> io::Result<()> {
        if self.access & (WRITE_DAC | WRITE_OWNER) != WRITE_DAC | WRITE_OWNER {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "its owner and access list may not be changed by this account",
            ));
        }
        let everything = OWNER_SECURITY_INFORMATION
            | DACL_SECURITY_INFORMATION
            | PROTECTED_DACL_SECURITY_INFORMATION;
        let Err(first) = self.set(everything, access) else {
            return Ok(());
        };
        self.set(OWNER_SECURITY_INFORMATION, access)
            .and_then(|()| {
                self.set(
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    access,
                )
            })
            .map_err(|second| {
                io::Error::new(
                    second.kind(),
                    format!("{first}; owner first, then access list: {second}"),
                )
            })
    }

    /// Removes this name: a link goes, never what it leads to; a file with other names keeps them, with
    /// its security unchanged; a folder must be empty. The name is gone once the handle is closed, here.
    pub(crate) fn remove(self) -> io::Result<()> {
        if self.access & DELETE == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "it may not be removed by this account, or another program holds it open \
                 without letting it be removed",
            ));
        }
        let posix = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        };
        // SAFETY: the handle is open and `posix` is the structure this class takes, of the size passed.
        let marked = unsafe {
            SetFileInformationByHandle(
                self.handle,
                FileDispositionInfoEx,
                (&posix as *const FILE_DISPOSITION_INFO_EX).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        };
        if marked != 0 {
            return Ok(());
        }
        // File systems or Windows versions without the extended form: the name goes once every handle
        // to it is closed.
        let plain = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: as above, with the structure the older class takes.
        let marked = unsafe {
            SetFileInformationByHandle(
                self.handle,
                FileDispositionInfo,
                (&plain as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        if marked == 0 {
            return Err(last_error());
        }
        Ok(())
    }
}

/// `folder` (a path from [`Entry::final_path`]) with `name` added.
pub(crate) fn final_child(folder: &str, name: &str) -> String {
    if folder.ends_with('\\') {
        format!("{folder}{name}")
    } else {
        format!("{folder}\\{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_added_with_one_separator() {
        assert_eq!(
            final_child(r"\\?\C:\ProgramData\AnixOps", "NetworkCore"),
            r"\\?\C:\ProgramData\AnixOps\NetworkCore"
        );
        assert_eq!(final_child(r"\\?\C:\", "Data"), r"\\?\C:\Data");
    }

    #[test]
    fn a_file_is_opened_as_itself() {
        let path =
            std::env::temp_dir().join(format!("anixops-windows-data-entry-{}", std::process::id()));
        std::fs::write(&path, "x").unwrap();
        let entry = Entry::open(&path).unwrap();
        assert!(!entry.is_folder());
        assert!(!entry.is_link());
        assert_eq!(entry.links(), 1);
        assert!(entry.final_path().unwrap().starts_with(r"\\?\"));
        entry.remove().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_new_file_is_made_only_where_nothing_is() {
        let path = std::env::temp_dir().join(format!(
            "anixops-windows-data-entry-new-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let made = Entry::create_new(&path).unwrap().unwrap();
        assert!(!made.is_reparse_point());
        assert_eq!(made.links(), 1);
        drop(made);
        assert!(Entry::create_new(&path).unwrap().is_none());
        Entry::open(&path).unwrap().remove().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_file_another_program_holds_open_is_still_opened_and_taken_back() {
        use std::os::windows::fs::OpenOptionsExt;

        let path = std::env::temp_dir().join(format!(
            "anixops-windows-data-entry-held-{}",
            std::process::id()
        ));
        std::fs::write(&path, "x").unwrap();
        let _privileges = crate::account::enable_ownership_privileges();
        // As a console, backup or virus scan holds it: others may read and write it, not remove it.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&path)
            .unwrap();
        let entry = Entry::open(&path).unwrap();
        assert_eq!(entry.access & DELETE, 0);
        assert!(!entry.is_link());
        entry.take_over("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)").unwrap();
        // Removing it needs what the other program refuses, and fails rather than leaving it half done.
        assert!(entry.remove().is_err());
        assert!(path.exists());
        let kept = Entry::open_kept(&path).unwrap();
        assert_eq!(kept.access & DELETE, 0);
        drop(kept);
        drop(held);
        Entry::open(&path).unwrap().remove().unwrap();
        assert!(!path.exists());
    }
}
