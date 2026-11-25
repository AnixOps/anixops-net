//! The window's end of the channel to a background core, for a window written in Rust.
//!
//! The native desktop clients run the core in a system service (Windows) or a daemon (Linux) and the window
//! in the user's session; `client-ipc` carries what they say (`docs/architecture/desktop-native.md`). This
//! crate is what such a window needs and nothing of the engine:
//!
//! - [`link`]: [`CoreLink`], the standing link to the core. It connects, greets, and connects again
//!   whenever the core went away (stopped, upgraded, restarted), and tells the window each change.
//! - [`window`]: [`WindowEnd`] answers what the core asks of the user's session (the system proxy settings,
//!   the login entry) and passes the core's events on.
//! - [`ask`] and [`event`]: the names of those questions and events. `client-service`, the core's side,
//!   uses the same names; the samples that fix their shapes are under its `fixtures/`.

#[cfg(target_os = "linux")]
pub mod link;
pub mod window;

#[cfg(target_os = "linux")]
pub use crate::link::{CoreLink, LinkOptions, LinkState};
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
    /// `{ "action": { "action": "select_node", "node": … } }`, an action an AI tool asked for
    /// (`client_mcp::Action`, docs/mcp-desktop.md §6). The window asks the user to allow or deny it, with the
    /// choice to allow actions of the same kind for 15 minutes, and closes the question itself after 60 seconds.
    /// Answer: `{ "answer": "allowed" | "denied" | "no_answer", "for_kind": false }`. An error (a window that
    /// cannot ask) refuses the action.
    pub const CONFIRM_ACTION: &str = "session.confirm_action";
}

/// What the core tells a window without being asked.
pub mod event {
    /// `{ "title": …, "body": …, "openUrl": … }`: a notification for the window to show.
    pub const NOTICE: &str = "notice";
}
