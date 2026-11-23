//! The service's data folder, closed to everyone but the system and administrators.
//!
//! `%ProgramData%` lets any user make folders in it, and a folder made there is open to all users unless
//! told otherwise. The service's folder holds subscriptions, keys and captured traffic, and the service
//! acts on what it reads there with the system's rights. So the folder is made with an access list of its
//! own, and one that is already there is only used when the system or an administrator owns it: a folder
//! an ordinary user put in the way first is refused. The window client shares this code
//! (`crates/windows-data`).

pub use windows_data::data_dir::{closed, describe, secure, trusted_owner};
