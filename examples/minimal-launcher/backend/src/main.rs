//! Headless demonstration transport, not a Tauri WebView.
use hostmylauncher_tauri::{contract::*, Bridge, InstanceLocation, LocateError};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn output(writer: &Mutex<io::Stdout>, value: Value) {
    let mut writer = writer.lock().expect("stdout lock");
    writeln!(writer, "{value}").expect("stdout write");
    writer.flush().expect("stdout flush");
}

#[tokio::main]
async fn main() {
    let provider = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/launchers/demo/instances/demo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"id": "demo", "launcher_id": "demo", "title": "Local demo"}
        })))
        .mount(&provider)
        .await;
    Mock::given(method("GET"))
        .and(path("/launchers/demo/instances/demo/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"path": "mods/demo.jar", "size": 8,
                "hashes": {"sha1": "0000000000000000000000000000000000000000"},
                "downloads": []}]
        })))
        .mount(&provider)
        .await;
    let temporary = tempfile::tempdir().expect("project-confined TMPDIR");
    let writer = Arc::new(Mutex::new(io::stdout()));
    let events = writer.clone();
    let bridge = Bridge::new(
        hostmylauncher::Client::with_base_url(&provider.uri()).unwrap(),
        vec!["demo".into()],
        {
            let destination = temporary.path().join("missing-install");
            let manifest_path = temporary.path().join("manifest.json");
            // The host answers from its own catalog; this example has one installation.
            move |launcher: &str, instance: &str| {
                if (launcher, instance) == ("demo", "demo") {
                    Ok(InstanceLocation {
                        destination: destination.clone(),
                        manifest_path: manifest_path.clone(),
                    })
                } else {
                    Err(LocateError::NotInstalled)
                }
            }
        },
        Arc::new(move |snapshot| {
            output(
                &events,
                json!({
                    "event": hostmylauncher_tauri::registry::OPERATION_EVENT, "payload": snapshot
                }),
            )
        }),
    )
    .expect("valid host configuration");
    for line in io::stdin().lock().lines() {
        let message: Value =
            serde_json::from_str(&line.expect("stdin line")).expect("request JSON");
        let id = message["id"].clone();
        let request = message["args"]["request"].clone();
        // This demonstration only exercises the public service methods needed
        // by the smoke test. Real IPC registration lives in the Tauri plugin.
        let result = match message["command"].as_str().unwrap_or("") {
            "plugin:hostmylauncher|get_instance" => {
                match serde_json::from_value::<GetInstanceRequest>(request) {
                    Ok(request) => bridge.get_instance(request).await.map(|v| json!(v)),
                    Err(_) => Err(invalid_request()),
                }
            }
            "plugin:hostmylauncher|start_operation" => {
                match serde_json::from_value::<StartOperationRequest>(request) {
                    Ok(request) => bridge.start_operation(request).await.map(|v| json!(v)),
                    Err(_) => Err(invalid_request()),
                }
            }
            "plugin:hostmylauncher|get_operation" => {
                match serde_json::from_value::<GetOperationRequest>(request) {
                    Ok(request) => bridge.get_operation(request).await.map(|v| json!(v)),
                    Err(_) => Err(invalid_request()),
                }
            }
            _ => Err(invalid_request()),
        };
        match result {
            Ok(result) => output(&writer, json!({"id": id, "result": result})),
            Err(error) => output(&writer, json!({"id": id, "error": error})),
        }
    }
    // stdin EOF means the demonstration client finished. Its operation must be
    // terminal before closing stdin; scoped fixtures clean up on normal exit.
}

fn invalid_request() -> BridgeError {
    BridgeError {
        code: ErrorCode::InvalidRequest,
        operation_id: None,
        stage: None,
        context: None,
    }
}
