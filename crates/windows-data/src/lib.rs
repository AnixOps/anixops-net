//! What the Windows clients keep for the whole machine under `%ProgramData%`, shared by the window client
//! (`apps/windows-gui`) and the service (`apps/windows-service`):
//!
//! - [`data_dir`]: the data folder, closed to everyone but the system and administrators, including what an
//!   earlier version left in it open to all users;
//! - [`private_file`]: key files that only their owner, the system and administrators may open;
//! - [`trusted_roots`]: the HTTPS decryption roots users trusted, so that removing the client can take them out
//!   of those users' certificate stores again;
//! - [`account`]: who this process runs as.

#![cfg(windows)]

pub mod account;
pub mod data_dir;
pub mod private_file;
pub mod trusted_roots;

mod descriptor;
mod entry;
