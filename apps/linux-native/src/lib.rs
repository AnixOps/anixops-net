//! The native Linux window of AnixOps Net (preview).
//!
//! What the window shows and how it is started lives here without GTK, so it is tested on every system:
//! the pages ([`sections`]) and the launch options ([`launch`]). The wording comes from the catalogue all
//! native windows share (`client-strings`). The window itself ([`window`]) needs GTK 4 and libadwaita and is
//! only built with the `gui` feature, on Linux.

pub mod launch;
pub mod sections;

#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod window;

pub use client_strings::Language;

/// The application id: the name on the session bus, the desktop file and the window class.
pub const APP_ID: &str = "io.anixops.net.preview";
