//! Who may control the daemon.
//!
//! The socket itself is open to every local user; the kernel tells the daemon which user is at the other
//! end, and the daemon decides here. Root may, and so may a user who is signed in at the machine: the
//! login manager keeps a record per signed-in user (`/run/systemd/users/<uid>`) and a runtime folder
//! (`/run/user/<uid>`) from the user's first session until the last one ends. An account nobody is signed
//! in to (a service account, or a program left running by one) may not.
//!
//! This is the same line Windows draws for the named pipe (users signed in at the machine). It does not
//! tell a person at the screen from one signed in over the network; actions that need that distinction are
//! to go through polkit when they are added.

use std::path::PathBuf;

/// What a refused user is told.
pub const REFUSED: &str =
    "this user is not signed in at this computer and may not control AnixOps Net";

pub trait Access: Send + Sync + 'static {
    fn allows(&self, uid: u32) -> bool;
}

/// Root, and users with a session on this machine.
pub struct SignedIn {
    /// Folders in which the system keeps one entry per signed-in user, named by user id. One is enough.
    places: Vec<PathBuf>,
}

impl SignedIn {
    pub fn new(places: Vec<PathBuf>) -> Self {
        Self { places }
    }
}

impl Default for SignedIn {
    /// The login manager's own record first: the unit hides `/run/user` from the daemon
    /// (`ProtectHome`), so the runtime folders only count where the daemon runs without that.
    fn default() -> Self {
        Self::new(vec![
            PathBuf::from("/run/systemd/users"),
            PathBuf::from("/run/user"),
        ])
    }
}

impl Access for SignedIn {
    fn allows(&self, uid: u32) -> bool {
        uid == 0
            || self
                .places
                .iter()
                .any(|place| place.join(uid.to_string()).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn root_and_signed_in_users_are_let_in() {
        let root = std::env::temp_dir().join(format!("anixops-netd-access-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let records = root.join("records");
        let folders = root.join("folders");
        fs::create_dir_all(&records).unwrap();
        fs::create_dir_all(folders.join("1001")).unwrap();
        fs::write(records.join("1000"), "").unwrap();
        // A place that does not exist on this system lets nobody in and breaks nothing.
        let access = SignedIn::new(vec![root.join("missing"), records, folders]);
        assert!(access.allows(0));
        assert!(access.allows(1000));
        assert!(access.allows(1001));
        assert!(!access.allows(1002));
        fs::remove_dir_all(&root).unwrap();
    }
}
