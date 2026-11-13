//! AnixOps Net for Linux, and the shared backend of the Android app (`apps/android`). The backend is a library
//! so the workspace tests it without the window toolkit; the window (`app`) sits behind the `gui` feature.

#[cfg(feature = "gui")]
pub mod app;
pub mod backend;
