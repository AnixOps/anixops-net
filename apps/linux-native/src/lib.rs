//! The native Linux window of AnixOps Net (preview).
//!
//! What the window shows and how it is started lives here without GTK, so it is tested on every system:
//! the pages ([`sections`]), the launch options ([`launch`]) and what the overview and proxies pages show
//! ([`overview`], [`connections`], [`proxies`], [`subscriptions`], [`groups`], [`node`], [`checkup`]). The wording comes from the catalogue all native windows share (`client-strings`). The
//! window itself ([`window`], [`overview_page`], [`proxies_page`]) needs GTK 4 and libadwaita and is only built with the `gui`
//! feature, on Linux.

pub mod checkup;
pub mod connections;
pub mod groups;
pub mod launch;
pub mod node;
pub mod overview;
pub mod proxies;
pub mod sections;
pub mod subscriptions;

#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod connection_list;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod group_list;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod list_keys;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod node_dialog;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod overview_page;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod proxies_page;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod subscription_list;
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod window;

pub use client_strings::Language;

/// The application id: the name on the session bus, the desktop file and the window class.
pub const APP_ID: &str = "io.anixops.net";
