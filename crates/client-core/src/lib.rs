//! The client service shared by every AnixOps Net window, free of any window toolkit so the workspace tests it
//! on its own. `backend` holds saved state, the embedded engine and the desktop integration; `client` answers
//! the interface's commands by name (names and JSON shapes as in the shared interface's `bridge.ts`) for the
//! Windows and Linux services and the Android app. See `docs/architecture/desktop-native.md`.

pub mod backend;
pub mod client;
pub mod download;
pub mod instance;
pub mod refusal;
