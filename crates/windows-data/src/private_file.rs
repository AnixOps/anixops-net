//! Key files only their owner, the system and administrators may open: the Windows counterpart of mode
//! `0600`, for a key written to a folder whose access list may still be open to other users.

use std::io;
use std::path::Path;

use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
};

use crate::descriptor::set;

/// The file's owner (`OW`, whoever made it), the system and administrators in full; nothing inherited.
const PRIVATE: &str = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)";

/// Closes `path` to everyone but its owner, the system and administrators. Called on a file just made, before
/// anything secret is written to it.
pub fn restrict(path: &Path) -> io::Result<()> {
    set(
        path,
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        PRIVATE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::describe;

    #[test]
    fn a_private_file_lets_in_only_its_owner_the_system_and_administrators() {
        let path = std::env::temp_dir().join(format!(
            "anixops-windows-data-private-{}",
            std::process::id()
        ));
        std::fs::write(&path, "").unwrap();
        restrict(&path).unwrap();
        std::fs::write(&path, "secret").unwrap();
        let described = describe(&path).unwrap();
        assert!(described.contains("D:P"), "{described}");
        for open in [";;;WD)", ";;;BU)", ";;;AU)", ";;;IU)"] {
            assert!(!described.contains(open), "{described}");
        }
        // Whoever made it can still read it.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        std::fs::remove_file(&path).unwrap();
    }
}
