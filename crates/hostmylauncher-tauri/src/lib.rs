//! Optional IPC transport for hostmylauncher-rs.
//! DTOs and the command registry are available without a WebView runtime.
//! The host owns provider configuration and locates its installations.
mod bridge;
mod commands;
pub mod contract;
pub mod registry;

pub use bridge::{Bridge, CatalogSink, EventSink, InstanceLocation, InstanceLocator, LocateError};

#[cfg(feature = "plugin")]
pub mod plugin;
