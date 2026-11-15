//! The client core as a background service, and the window's end of the channel to it.
//!
//! The native desktop clients run `client-core` in a system service (Windows) or a daemon (Linux) and the
//! window in the user's session; `client-ipc` carries what they say (`docs/architecture/desktop-native.md`).
//! This crate joins the two:
//!
//! - [`service`]: the service's side. [`Windows`] keeps the windows that are connected, answers their commands
//!   with the core, and is the core's [`UserSession`](client_core::backend::session::UserSession): what
//!   belongs to one user (the system proxy settings, the login entry, trust in the decryption root) is asked of that user's window.
//! - [`window`]: the window's side, for a window written in Rust. [`WindowEnd`] answers those questions with
//!   the session the window runs in and passes the core's events on.
//!
//! The names and shapes of what the core asks a window are fixed by [`ask`] and the samples under
//! `fixtures/`, which a window written in another language checks too.

pub mod service;
pub mod window;

pub use crate::service::{Commands, Windows};
pub use crate::window::WindowEnd;

/// What the core asks a window. Every name starts with `session.`, which no command of the core does, so a
/// window can never be made to ask the core for one of these.
pub mod ask {
    /// The prefix of every name here.
    pub const PREFIX: &str = "session.";
    /// No arguments. Answer: `{ "desktop": … }`, what keeps the proxy settings in the window's session, or
    /// `null` when the client cannot set them there.
    pub const DESCRIBE: &str = "session.describe";
    /// `{ "port": 7890 }`. Points the user's system proxy at the local proxy. Answer: what to put back.
    pub const APPLY_PROXY: &str = "session.apply_proxy";
    /// `{ "backup": … }`, an earlier answer to [`APPLY_PROXY`]. Puts the user's proxy settings back.
    pub const RESTORE_PROXY: &str = "session.restore_proxy";
    /// No arguments. Answer: whether the window starts after login.
    pub const STARTS_AFTER_LOGIN: &str = "session.starts_after_login";
    /// `{ "enabled": true }`. Adds or removes the entry that starts the window after login.
    pub const SET_START_AFTER_LOGIN: &str = "session.set_start_after_login";
    /// `{ "certificate": "-----BEGIN CERTIFICATE-----…" }`. Answer: whether the user's own certificate store
    /// trusts this HTTPS decryption root, or `null` when the window keeps no such store.
    pub const ROOT_TRUSTED: &str = "session.root_trusted";
    /// `{ "certificate": … }`. Adds the root to the user's own certificate store. The system may ask the
    /// user to confirm, so the answer can take as long as the user does; a refusal is an error.
    pub const TRUST_ROOT: &str = "session.trust_root";
    /// `{ "certificate": … }`. Takes the root out of the user's own certificate store.
    pub const UNTRUST_ROOT: &str = "session.untrust_root";
}

/// What the core tells a window without being asked.
pub mod event {
    /// `{ "title": …, "body": …, "openUrl": … }`: a notification for the window to show.
    pub const NOTICE: &str = "notice";
}
