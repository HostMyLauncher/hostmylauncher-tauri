//! Test host: a fixed list of installations behind the public locator.
#![allow(dead_code)]
use hostmylauncher_tauri::{InstanceLocation, InstanceLocator, LocateError};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct InstanceRegistration {
    pub instance_id: String,
    pub launcher_id: String,
    pub destination: PathBuf,
    pub manifest_path: PathBuf,
}

pub fn located(registrations: Vec<InstanceRegistration>) -> impl InstanceLocator {
    move |launcher: &str, instance: &str| {
        registrations
            .iter()
            .find(|r| r.launcher_id == launcher && r.instance_id == instance)
            .map(|r| InstanceLocation {
                destination: r.destination.clone(),
                manifest_path: r.manifest_path.clone(),
            })
            .ok_or(LocateError::NotInstalled)
    }
}
