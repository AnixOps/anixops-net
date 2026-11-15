//! AnixOps Net for Linux, and the Tauri window of the Android app (`apps/android`). The backend and the command
//! dispatch live in `crates/client-core` (re-exported here as `backend` and `client`), which the workspace tests
//! without the window toolkit; the window (`app`) sits behind the `gui` feature.

#[cfg(feature = "gui")]
pub mod app;
pub use client_core::{backend, client};
