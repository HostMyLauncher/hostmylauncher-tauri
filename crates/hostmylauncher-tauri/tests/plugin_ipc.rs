#![cfg(feature = "plugin-tests")]
//! Real Tauri dispatch on MockRuntime, not proof of a live WebView.
use hostmylauncher::Client;
mod common;
use common::{located, InstanceRegistration};
use hostmylauncher_tauri::{contract::*, plugin, registry, Bridge};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    test::{mock_builder, mock_context, noop_assets, MockRuntime},
    Listener,
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn invoke(
    window: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    request: Value,
) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        window,
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(json!({"request":request})),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        },
    )
    .map(|body| body.deserialize().unwrap())
}
fn context() -> tauri::Context<MockRuntime> {
    let mut context = mock_context(noop_assets());
    // Explicit local test authority only; production hosts configure capabilities.
    for (_, command) in registry::COMMANDS {
        context.runtime_authority_mut().__allow_command(
            (*command).into(),
            tauri::utils::acl::ExecutionContext::Local,
        );
    }
    context
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_generated_commands_reach_real_tauri_bindings() {
    let server = MockServer::start().await;
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        located(vec![]),
        Arc::new(|_| {}),
    )
    .unwrap();
    let app = mock_builder()
        .plugin(plugin::init(service))
        .build(context())
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let cases = [
        (
            "listInstances",
            json!({"launcherId":"outside","limit":null,"offset":null}),
            ErrorCode::InvalidRequest,
        ),
        (
            "getInstance",
            json!({"launcherId":"outside","instanceId":"one"}),
            ErrorCode::InvalidRequest,
        ),
        (
            "resolveByCode",
            json!({"launcherIds":[],"code":"PRIVATE_CODE"}),
            ErrorCode::InvalidRequest,
        ),
        (
            "getInstanceFiles",
            json!({"launcherId":"outside","instanceId":"one","code":null}),
            ErrorCode::InvalidRequest,
        ),
        (
            "startOperation",
            json!({"launcherId":"catalog","instanceId":"unknown","kind":"sync","code":null}),
            ErrorCode::NotFound,
        ),
        (
            "getOperation",
            json!({"operationId":"unknown"}),
            ErrorCode::NotFound,
        ),
        (
            "watchCatalog",
            json!({"launcherId":"outside"}),
            ErrorCode::InvalidRequest,
        ),
    ];
    for (key, request, expected) in cases {
        let command = registry::COMMANDS
            .iter()
            .find(|(name, _)| *name == key)
            .unwrap()
            .1;
        let response = invoke(&window, command, request).unwrap_err();
        let error: BridgeError = serde_json::from_value(response)
            .expect("registered handler returns structured BridgeError");
        assert_eq!(error.code, expected);
        assert!(error.operation_id.is_none() && error.stage.is_none() && error.context.is_none());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(invoke(&window, "plugin:hostmylauncher|not_registered", json!({})).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_acceptance_status_and_real_emit_use_the_generated_contract() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances/one/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[]})))
        .mount(&server)
        .await;
    let temp = tempfile::TempDir::new().unwrap();
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        located(vec![InstanceRegistration {
            instance_id: "one".into(),
            launcher_id: "catalog".into(),
            destination: temp.path().join("root"),
            manifest_path: temp.path().join("manifest.json"),
        }]),
        Arc::new(|_| {}),
    )
    .unwrap();
    let app = mock_builder()
        .plugin(plugin::init(service))
        .build(context())
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::<OperationSnapshot>::new()));
    let captured = events.clone();
    let listener = app.listen(registry::OPERATION_EVENT, move |event| {
        captured
            .lock()
            .unwrap()
            .push(serde_json::from_str(event.payload()).unwrap());
    });
    let start = registry::COMMANDS
        .iter()
        .find(|(key, _)| *key == "startOperation")
        .unwrap()
        .1;
    let get = registry::COMMANDS
        .iter()
        .find(|(key, _)| *key == "getOperation")
        .unwrap()
        .1;
    let accepted: OperationSnapshot = serde_json::from_value(
        invoke(
            &window,
            start,
            json!({"launcherId":"catalog","instanceId":"one","kind":"sync","code":null}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(accepted.status, OperationStatus::Queued);
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state: OperationSnapshot = serde_json::from_value(
                invoke(&window, get, json!({"operationId":accepted.operation_id})).unwrap(),
            )
            .unwrap();
            if matches!(
                state.status,
                OperationStatus::Succeeded | OperationStatus::Failed
            ) {
                break state;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(terminal.status, OperationStatus::Succeeded);
    assert_eq!(
        terminal.result,
        Some(OperationResult::Sync {
            downloaded: 0,
            removed: 0
        })
    );
    assert_eq!(terminal.instance_id, accepted.instance_id);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(events.lock().unwrap().last(), Some(&terminal));
    app.unlisten(listener);
}

#[test]
fn build_permissions_parse_and_cover_registered_commands() {
    let permissions: tauri::utils::acl::manifest::PermissionFile = serde_json::from_str(
        include_str!(concat!(env!("OUT_DIR"), "/hostmylauncher-permissions.json")),
    )
    .unwrap();
    assert_eq!(permissions.permission.len(), registry::COMMANDS.len());
    assert_eq!(
        permissions.default.as_ref().unwrap().permissions.len(),
        registry::COMMANDS.len()
    );
    for (_, full_name) in registry::COMMANDS {
        let command = full_name.split('|').nth(1).unwrap();
        let permission = permissions
            .permission
            .iter()
            .find(|p| p.commands.allow == [command])
            .unwrap();
        assert_eq!(
            permission.identifier,
            format!("allow-{}", command.replace('_', "-"))
        );
        assert!(permissions
            .default
            .as_ref()
            .unwrap()
            .permissions
            .contains(&permission.identifier));
    }
}
