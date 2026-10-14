//! Usage: cargo run -p hostmylauncher-tauri --bin generate-contract -- [--check]
use std::{env, fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if !args.is_empty() && args != ["--check"] {
        eprintln!("usage: generate-contract [--check]");
        return ExitCode::from(2);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/tauri/src/generated/contract.ts");
    let expected = hostmylauncher_tauri::registry::typescript_contract();
    if args == ["--check"] {
        match fs::read_to_string(&path) {
            Ok(actual) if actual == expected => {
                println!("generated contract is current");
                ExitCode::SUCCESS
            }
            _ => {
                eprintln!("generated contract is missing or stale; run generate-contract");
                ExitCode::FAILURE
            }
        }
    } else {
        let result = fs::create_dir_all(path.parent().expect("output parent"))
            .and_then(|()| fs::write(&path, expected));
        match result {
            Ok(()) => {
                println!("generated packages/tauri/src/generated/contract.ts");
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("cannot write generated contract");
                ExitCode::FAILURE
            }
        }
    }
}
