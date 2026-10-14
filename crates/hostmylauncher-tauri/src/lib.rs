//! Session-scoped provider bridge, independent of Tauri runtime.
mod bridge;
mod commands;
pub mod contract;
pub mod registry;
pub use bridge::{Bridge, EventSink, InstanceRegistration};
