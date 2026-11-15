//! The client core as a background service, and the window's end of the channel to it.
//!
//! The native desktop clients run `client-core` in a system service (Windows) or a daemon (Linux) and the
//! window in the user's session; `client-ipc` carries what they say (`docs/architecture/desktop-native.md`).
//! This crate joins the two:
//!
//! - [`service`]: the service's side. [`Windows`] keeps the windows that are connected, answers their commands
//!   with the core, and is the core's [`UserSession`](client_core::backend::session::UserSession): what
//!   belongs to one user (the system proxy settings, the login entry, trust in the decryption root) is asked of that user's window.
//! - The window's side, for a window written in Rust, is `client-window`, which has no engine in it;
//!   [`WindowEnd`] and what goes with it are shown here too.
//!
//! The names and shapes of what the core asks a window are fixed by [`ask`] and the samples under
//! `fixtures/`, which a window written in another language checks too.

pub mod service;

pub use crate::service::{Commands, Windows};
pub use client_window::{ask, event, window, WindowEnd};
