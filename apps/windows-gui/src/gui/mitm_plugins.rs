//! The plugin library of the desktop clients (`desktop_mitm::plugins`), kept under the managed data directory.

pub use desktop_mitm::plugins::*;
use std::path::PathBuf;

/// The folder under the managed data directory that holds the library.
pub fn directory() -> PathBuf {
    platform_windows::managed::windows_managed_data_directory()
        .join("mitm")
        .join("plugins")
}
