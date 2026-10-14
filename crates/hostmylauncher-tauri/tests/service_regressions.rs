//! Additional public-interface regressions for callback isolation and admission.
use futures_util::FutureExt;
use hostmylauncher::Client;
use hostmylauncher_tauri::{contract::*, Bridge, InstanceRegistration};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

async fn done(service: &Bridge, id: &str) -> OperationSnapshot {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = service
                .get_operation(GetOperationRequest {
                    operation_id: id.into(),
                })
                .await
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
    .unwrap()
}
fn start(kind: OperationKind) -> StartOperationRequest {
    StartOperationRequest {
        instance_id: "one".into(),
        kind,
        code: None,
    }
}
fn registration(temp: &tempfile::TempDir) -> InstanceRegistration {
    InstanceRegistration {
        instance_id: "one".into(),
        launcher_id: "catalog".into(),
        destination: temp.path().join("root"),
        manifest_path: temp.path().join("manifest.json"),
    }
}
async fn two_files(server: &MockServer) {
    for (name, bytes) in [("a", "12345678"), ("b", "1234")] {
        Mock::given(method("GET"))
            .and(path(format!("/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(bytes))
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances/one/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[
            {"path":"a", "size":8, "downloads":[format!("{}/a",server.uri())]},
            {"path":"b", "size":4, "downloads":[format!("{}/b",server.uri())]}
        ]})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn callback_can_query_authoritative_state_without_global_lock_deadlock() {
    let server = MockServer::start().await;
    two_files(&server).await;
    let temp = tempfile::TempDir::new().unwrap();
    let slot = Arc::new(Mutex::new(None::<Bridge>));
    let callback_slot = slot.clone();
    let terminal_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_terminal = terminal_seen.clone();
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        vec![registration(&temp)],
        Arc::new(move |snapshot: OperationSnapshot| {
            let service = callback_slot.lock().unwrap().as_ref().unwrap().clone();
            let (send, receive) = std::sync::mpsc::channel();
            let id = snapshot.operation_id.clone();
            std::thread::spawn(move || {
                let state = service
                    .get_operation(GetOperationRequest { operation_id: id })
                    .now_or_never()
                    .unwrap()
                    .unwrap();
                let _ = send.send(state);
            });
            let queried = receive
                .recv_timeout(Duration::from_secs(1))
                .expect("callback must not hold service lock");
            assert_eq!(queried.operation_id, snapshot.operation_id);
            assert!(queried.sequence >= snapshot.sequence);
            if matches!(
                snapshot.status,
                OperationStatus::Succeeded | OperationStatus::Failed
            ) {
                callback_terminal.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }),
    )
    .unwrap();
    *slot.lock().unwrap() = Some(service.clone());
    let accepted = service
        .start_operation(start(OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(
        done(&service, &accepted.operation_id).await.status,
        OperationStatus::Succeeded
    );
    // Terminal state can be observed before its sink finishes.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !terminal_seen.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    *slot.lock().unwrap() = None;
}

#[tokio::test]
async fn actual_byte_counters_reset_between_files_and_install_counts_are_preparation() {
    let server = MockServer::start().await;
    two_files(&server).await;
    let temp = tempfile::TempDir::new().unwrap();
    let events = Arc::new(Mutex::new(Vec::<OperationSnapshot>::new()));
    let captured = events.clone();
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        vec![registration(&temp)],
        Arc::new(move |snapshot| captured.lock().unwrap().push(snapshot)),
    )
    .unwrap();
    let accepted = service
        .start_operation(start(OperationKind::Sync))
        .await
        .unwrap();
    let final_state = done(&service, &accepted.operation_id).await;
    assert_eq!(
        final_state.result,
        Some(OperationResult::Sync {
            downloaded: 2,
            removed: 0
        })
    );
    let events = events.lock().unwrap();
    let download: Vec<_> = events
        .iter()
        .filter(|s| s.status == OperationStatus::Running)
        .filter_map(|s| s.progress.as_ref())
        .filter(|p| p.stage == ProgressStage::Download)
        .collect();
    assert!(download.iter().all(|p| p.unit == Some(ProgressUnit::Bytes)));
    assert!(download
        .windows(2)
        .any(|p| p[0].current.unwrap() > p[1].current.unwrap()));
    assert!(download
        .iter()
        .any(|p| p.current == Some(8) && p.total == Some(8)));
    assert!(download
        .iter()
        .any(|p| p.current == Some(4) && p.total == Some(4)));
    let install: Vec<_> = events
        .iter()
        .filter(|s| s.status == OperationStatus::Running)
        .filter_map(|s| s.progress.as_ref())
        .filter(|p| p.stage == ProgressStage::Install)
        .collect();
    assert!(install.iter().any(|p| p.current == Some(1)
        && p.total == Some(2)
        && p.unit == Some(ProgressUnit::Entries)));
    assert!(install
        .iter()
        .any(|p| p.current == Some(2) && p.total == Some(2)));
}

#[tokio::test]
async fn panic_from_base_progress_is_caught_and_releases_guard() {
    let server = MockServer::start().await;
    two_files(&server).await;
    let temp = tempfile::TempDir::new().unwrap();
    let once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let captured = once.clone();
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        vec![registration(&temp)],
        Arc::new(move |snapshot| {
            if snapshot.status == OperationStatus::Running
                && snapshot
                    .progress
                    .as_ref()
                    .is_some_and(|p| p.stage == ProgressStage::Download)
                && !captured.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                panic!("test sink panic");
            }
        }),
    )
    .unwrap();
    let accepted = service
        .start_operation(start(OperationKind::Sync))
        .await
        .unwrap();
    let failed = done(&service, &accepted.operation_id).await;
    assert_eq!(failed.status, OperationStatus::Failed);
    assert_eq!(failed.error.as_ref().unwrap().code, ErrorCode::Internal);
    assert_eq!(
        failed.error.as_ref().unwrap().stage,
        Some(ProgressStage::Download)
    );
    assert!(failed.result.is_none());
    let next = service
        .start_operation(start(OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(
        done(&service, &next.operation_id).await.status,
        OperationStatus::Succeeded
    );
}

#[tokio::test]
async fn invalid_request_fields_do_not_consume_operation_ids_or_make_requests() {
    let server = MockServer::start().await;
    two_files(&server).await;
    let temp = tempfile::TempDir::new().unwrap();
    let events = Arc::new(Mutex::new(Vec::<OperationSnapshot>::new()));
    let captured = events.clone();
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        vec![registration(&temp)],
        Arc::new(move |snapshot| captured.lock().unwrap().push(snapshot)),
    )
    .unwrap();
    for request in [
        StartOperationRequest {
            instance_id: "../one".into(),
            kind: OperationKind::Sync,
            code: None,
        },
        StartOperationRequest {
            instance_id: "one".into(),
            kind: OperationKind::Verify,
            code: Some(" ".into()),
        },
    ] {
        let error = service.start_operation(request).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.operation_id.is_none() && error.stage.is_none() && error.context.is_none());
    }
    assert!(events.lock().unwrap().is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(!temp.path().join("root").exists() && !temp.path().join("manifest.json").exists());
    let accepted = service
        .start_operation(start(OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(accepted.operation_id, "operation-1");
    assert_eq!(
        done(&service, &accepted.operation_id).await.status,
        OperationStatus::Succeeded
    );
}

#[cfg(unix)]
#[tokio::test]
async fn changed_manifest_symlink_is_rejected_before_state_http_or_files() {
    let server = MockServer::start().await;
    let temp = tempfile::TempDir::new().unwrap();
    let registration = registration(&temp);
    let service = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        vec![registration.clone()],
        Arc::new(|_| {}),
    )
    .unwrap();
    let unrelated = temp.path().join("unrelated.json");
    std::fs::write(&unrelated, b"keep").unwrap();
    std::os::unix::fs::symlink(&unrelated, &registration.manifest_path).unwrap();
    let error = service
        .start_operation(start(OperationKind::Repair))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.operation_id.is_none());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
    assert!(!registration.destination.exists());
    assert_eq!(
        service
            .get_operation(GetOperationRequest {
                operation_id: "operation-1".into()
            })
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
