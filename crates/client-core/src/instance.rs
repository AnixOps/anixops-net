//! One window per data folder. A desktop starts the client again for every `anixops://` link and every click on
//! the launcher; that second process must hand its arguments to the running one and leave without opening the
//! backend, because opening it puts back the proxy settings a previous run left changed, starts the automatic
//! connection and serves the metrics endpoint, all of which belong to the running window.
//!
//! The window's own single instance handoff (the session bus on Linux) decides first. This lock is the second
//! line: the process that holds it owns the folder, whatever the handoff could or could not tell. The system
//! releases it when the process ends, crashed or not, so the next start owns the folder again and still puts
//! back what a crashed run left changed.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// The lock file in the data folder.
pub const LOCK_FILE: &str = "instance.lock";

/// The lock file of the data folder `dir`.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK_FILE)
}

/// Ownership of a data folder, held until it is dropped (or the process ends).
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// What [`claim`] found.
#[derive(Debug)]
pub enum Claim {
    /// This process owns the folder now; keep the lock for as long as the client runs.
    Owner(InstanceLock),
    /// Another live process owns the folder: leave it, its proxy settings and its connection alone.
    Running,
}

/// Takes the data folder `dir` for this process without waiting. An error means the lock file could not be made
/// or locked at all (a read-only folder, a file system without locks); the caller decides whether to go on.
pub fn claim(dir: &Path) -> io::Result<Claim> {
    fs::create_dir_all(dir)?;
    let mut options = OpenOptions::new();
    // Never truncated: the file only carries the lock, and another process may hold it.
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(lock_path(dir))?;
    match file.try_lock() {
        Ok(()) => Ok(Claim::Owner(InstanceLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(Claim::Running),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anixops-net-instance-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_second_start_finds_the_folder_taken() {
        let dir = temp_dir("second");
        let first = claim(&dir).unwrap();
        assert!(matches!(first, Claim::Owner(_)));
        // Each claim opens the file anew, so a second claim in this process stands for a second process.
        assert!(matches!(claim(&dir).unwrap(), Claim::Running));
        assert!(matches!(claim(&dir).unwrap(), Claim::Running));
        drop(first);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_folder_is_free_again_once_the_owner_is_gone() {
        // A crashed run leaves the file but not the lock: the next start owns the folder and restores the proxy.
        let dir = temp_dir("released");
        let first = claim(&dir).unwrap();
        drop(first);
        assert!(lock_path(&dir).exists());
        assert!(matches!(claim(&dir).unwrap(), Claim::Owner(_)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_folder_is_made() {
        let dir = temp_dir("missing").join("nested");
        assert!(matches!(claim(&dir).unwrap(), Claim::Owner(_)));
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }
}
