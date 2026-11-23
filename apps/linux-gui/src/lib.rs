//! AnixOps Net for Linux. The backend and the command dispatch live in `crates/client-core` (re-exported here as
//! `backend` and `client`), which the workspace tests without the window toolkit; the window (`app`) sits behind
//! the `gui` feature.

#[cfg(feature = "gui")]
pub mod app;
pub use client_core::{backend, client, instance};
