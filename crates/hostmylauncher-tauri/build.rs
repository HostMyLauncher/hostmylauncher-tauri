//! Generate Tauri permission metadata in Cargo OUT_DIR, not the source tree.
//! Equivalent to the permission metadata convention consumed by tauri-build.
#[path = "src/commands.rs"]
mod commands;

fn main() {
    println!("cargo:rerun-if-changed=src/commands.rs");
    println!("cargo:rerun-if-changed=build.rs");
    macro_rules! names {
        ($($key:ident => $command:ident,)*) => { &[$(stringify!($command)),*] };
    }
    let commands: &[&str] = hostmylauncher_commands!(names);
    let permissions: Vec<_> = commands
        .iter()
        .map(|command| {
            serde_json::json!({
                "identifier": format!("allow-{}", command.replace('_', "-")),
                "description": format!("Allows the {command} command."),
                "commands": { "allow": [command] }
            })
        })
        .collect();
    let default: Vec<_> = commands
        .iter()
        .map(|command| format!("allow-{}", command.replace('_', "-")))
        .collect();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let permission_file = out.join("hostmylauncher-permissions.json");
    std::fs::write(&permission_file, serde_json::to_vec(&serde_json::json!({
        "default": { "description": "Allows the six host-configured bridge commands.", "permissions": default },
        "permission": permissions
    })).expect("serialize permissions")).expect("write permissions");
    let paths_file = out.join("hostmylauncher-permission-files");
    std::fs::write(
        &paths_file,
        serde_json::to_vec(&vec![permission_file]).expect("serialize permission paths"),
    )
    .expect("write permission paths");
    println!("cargo:PERMISSION_FILES_PATH={}", paths_file.display());
}
