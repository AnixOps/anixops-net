//! What the client changes in the signed-in user's own session: the system proxy settings, the entry that
//! starts the client after login and, where the core runs in a service, the user's trust in the HTTPS
//! decryption root.
//!
//! These belong to one user, not to the machine, so whichever program runs in that user's session does
//! them: the core itself where it runs in the window's process, the window where the core is a background
//! service (`docs/architecture/desktop-native.md`). This crate has no engine in it, so such a window links
//! it without linking the core. `client-core` shows the same modules under `client_core::backend`.
//!
//! - [`session`]: [`UserSession`](session::UserSession), what the core asks of the session, and
//!   [`LocalSession`](session::LocalSession), the session of the process itself.
//! - [`system_proxy`]: the desktop's proxy settings (GNOME, KDE Plasma; on Windows the user's own settings).
//! - [`autostart`]: the XDG entry that starts the client after login.

pub mod autostart;
pub mod session;
#[cfg(not(windows))]
pub mod system_proxy;
#[cfg(windows)]
#[path = "system_proxy_windows.rs"]
pub mod system_proxy;
