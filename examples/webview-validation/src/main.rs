use hostmylauncher_tauri::{Bridge, InstanceLocation, LocateError};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::Manager;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

struct ProofConfig {
    result_file: PathBuf,
    destination: PathBuf,
    finished: Arc<AtomicBool>,
}

#[tauri::command]
fn finish_proof(app: tauri::AppHandle, report: String) -> Result<(), String> {
    let config = app.state::<ProofConfig>();
    let mut report: serde_json::Value =
        serde_json::from_str(&report).map_err(|error| error.to_string())?;
    let operation_succeeded = report["ok"] == true;
    let destination_verified = std::fs::read(config.destination.join("proof.bin"))
        .is_ok_and(|bytes| bytes == b"webview-proof-local-data");
    report["destinationVerified"] = json!(destination_verified);
    report["ok"] = json!(operation_succeeded && destination_verified);
    std::fs::write(&config.result_file, report.to_string()).map_err(|error| error.to_string())?;
    config.finished.store(true, Ordering::SeqCst);
    app.exit(if report["ok"] == true { 0 } else { 1 });
    Ok(())
}

#[tokio::main]
async fn main() {
    let provider = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/launchers/proof/instances/proof-instance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"id":"proof-instance", "launcher_id":"proof", "title":"Local WebView proof"}
        })))
        .mount(&provider)
        .await;
    let fixture_bytes = b"webview-proof-local-data";
    Mock::given(method("GET"))
        .and(path("/launchers/proof/instances/proof-instance/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[{
            "path":"proof.bin", "size":fixture_bytes.len(),
            "downloads":[format!("{}/artifact", provider.uri())]
        }]})))
        .mount(&provider)
        .await;
    Mock::given(method("GET"))
        .and(path("/artifact"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(500))
                .set_body_bytes(fixture_bytes.to_vec()),
        )
        .mount(&provider)
        .await;

    let temporary = tempfile::tempdir().expect("isolated proof fixture");
    let bridge = Bridge::new(
        hostmylauncher::Client::with_base_url(&provider.uri()).expect("local mock provider"),
        vec!["proof".into()],
        {
            let destination = temporary.path().join("installation");
            let manifest_path = temporary.path().join("manifest.json");
            // The host answers from its own catalog; this example has one installation.
            move |launcher: &str, instance: &str| {
                if (launcher, instance) == ("proof", "proof-instance") {
                    Ok(InstanceLocation {
                        destination: destination.clone(),
                        manifest_path: manifest_path.clone(),
                    })
                } else {
                    Err(LocateError::NotInstalled)
                }
            }
        },
        Arc::new(|_| {}),
    )
    .expect("bridge configuration");
    let result_file = std::env::var_os("HOSTMYLAUNCHER_WEBVIEW_RESULT")
        .map(PathBuf::from)
        .expect("set HOSTMYLAUNCHER_WEBVIEW_RESULT");
    let destination = temporary.path().join("installation");

    tauri::Builder::default()
        .manage(ProofConfig {
            result_file,
            destination,
            finished: Arc::new(AtomicBool::new(false)),
        })
        .plugin(hostmylauncher_tauri::plugin::init(bridge))
        .invoke_handler(tauri::generate_handler![finish_proof])
        .setup(|app| {
            let proof = app.state::<ProofConfig>();
            let result_file = proof.result_file.clone();
            let finished = proof.finished.clone();
            let app = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(20));
                if !finished.swap(true, Ordering::SeqCst) {
                    let _ = std::fs::write(
                        result_file,
                        r#"{"ok":false,"webview":true,"error":"WebView proof timed out"}"#,
                    );
                    app.exit(1);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run Wry WebView proof");
    drop(temporary);
}
