//! Optional IPC transport for hostmylauncher-rs.
//! DTOs and the command registry are available without a WebView runtime.
//! The host owns provider configuration and destination registrations.
mod bridge;
mod commands;
pub mod contract;
pub mod registry;

pub use bridge::{Bridge, EventSink, InstanceRegistration};

#[cfg(feature = "plugin")]
pub mod plugin;
