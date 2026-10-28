//! Stage-one RED: Bridge and InstanceRegistration deliberately do not exist yet.
//! All assertions use the intended public service, never its internal storage.
use hostmylauncher::Client;
use hostmylauncher_tauri::{contract::*, Bridge, InstanceRegistration};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

type Events = Arc<Mutex<Vec<OperationSnapshot>>>;
fn registration(id: &str, root: PathBuf, manifest: PathBuf) -> InstanceRegistration {
    registration_for("catalog", id, root, manifest)
}
fn registration_for(
    launcher: &str,
    id: &str,
    root: PathBuf,
    manifest: PathBuf,
) -> InstanceRegistration {
    InstanceRegistration {
        instance_id: id.into(),
        launcher_id: launcher.into(),
        destination: root,
        manifest_path: manifest,
    }
}
fn bridge(server: &MockServer, registrations: Vec<InstanceRegistration>, events: Events) -> Bridge {
    bridge_with_catalog(server, vec!["catalog".into()], registrations, events)
}
fn bridge_with_catalog(
    server: &MockServer,
    catalog: Vec<String>,
    registrations: Vec<InstanceRegistration>,
    events: Events,
) -> Bridge {
    Bridge::new(
        Client::with_base_url(&server.uri())
            .unwrap()
            .with_token("PRIVATE_BEARER"),
        catalog,
        registrations,
        Arc::new(move |snapshot| events.lock().unwrap().push(snapshot)),
    )
    .unwrap()
}
fn request(id: &str, kind: OperationKind) -> StartOperationRequest {
    request_for("catalog", id, kind)
}
fn request_for(launcher: &str, id: &str, kind: OperationKind) -> StartOperationRequest {
    StartOperationRequest {
        launcher_id: launcher.into(),
        instance_id: id.into(),
        kind,
        code: Some("PRIVATE_CODE".into()),
    }
}
async fn terminal(bridge: &Bridge, accepted: &OperationSnapshot) -> OperationSnapshot {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = bridge
                .get_operation(GetOperationRequest {
                    operation_id: accepted.operation_id.clone(),
                })
                .await
                .unwrap();
            if matches!(
                state.status,
                OperationStatus::Succeeded | OperationStatus::Failed
            ) {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("operation must terminate")
}
async fn provider(server: &MockServer, delay: bool) {
    Mock::given(method("GET"))
        .and(path("/artifact"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"provider".to_vec()))
        .mount(server)
        .await;
    let response = ResponseTemplate::new(200).set_body_json(json!({"results": [{
        "path": "mods/one.jar", "name": "one.jar", "size": 8,
        "downloads": [format!("{}/artifact?PRIVATE_URL", server.uri())]
    }]}));
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances/one/files"))
        .respond_with(if delay {
            response.set_delay(Duration::from_millis(250))
        } else {
            response
        })
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances/two/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .mount(server)
        .await;
}
fn safe(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).unwrap();
    for forbidden in [
        "PRIVATE_CODE",
        "PRIVATE_BEARER",
        "PRIVATE_URL",
        "PRIVATE_EXTRA",
        "downloads\":[",
        "http://",
    ] {
        assert!(
            !text.contains(forbidden),
            "unsafe transport field: {forbidden}"
        );
    }
}

#[tokio::test]
async fn same_instance_id_is_scoped_by_provider_and_snapshots_retain_both() {
    let server = MockServer::start().await;
    for provider_id in ["alpha", "beta"] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/launchers/{provider_id}/instances/shared/files"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
            .mount(&server)
            .await;
    }
    let temp = TempDir::new().unwrap();
    let events = Events::default();
    let bridge = bridge_with_catalog(
        &server,
        vec!["alpha".into(), "beta".into()],
        vec![
            registration_for(
                "alpha",
                "shared",
                temp.path().join("alpha"),
                temp.path().join("alpha.json"),
            ),
            registration_for(
                "beta",
                "shared",
                temp.path().join("beta"),
                temp.path().join("beta.json"),
            ),
        ],
        events.clone(),
    );

    let first = bridge
        .start_operation(request_for("alpha", "shared", OperationKind::Verify))
        .await
        .unwrap();
    let second = bridge
        .start_operation(request_for("beta", "shared", OperationKind::Verify))
        .await
        .unwrap();
    assert_ne!(first.operation_id, second.operation_id);
    assert_eq!(
        (first.launcher_id.as_str(), first.instance_id.as_str()),
        ("alpha", "shared")
    );
    assert_eq!(
        (second.launcher_id.as_str(), second.instance_id.as_str()),
        ("beta", "shared")
    );
    assert_eq!(terminal(&bridge, &first).await.launcher_id, "alpha");
    assert_eq!(terminal(&bridge, &second).await.launcher_id, "beta");
    let emitted = events.lock().unwrap();
    assert!(emitted
        .iter()
        .any(|event| event.operation_id == first.operation_id && event.launcher_id == "alpha"));
    assert!(emitted
        .iter()
        .any(|event| event.operation_id == second.operation_id && event.launcher_id == "beta"));
}

#[tokio::test]
async fn canonical_destination_conflict_is_enforced_across_providers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/launchers/alpha/instances/shared/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"results": []}))
                .set_delay(Duration::from_millis(250)),
        )
        .mount(&server)
        .await;
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("shared-install");
    let events = Events::default();
    let bridge = bridge_with_catalog(
        &server,
        vec!["alpha".into(), "beta".into()],
        vec![
            registration_for(
                "alpha",
                "shared",
                root.clone(),
                temp.path().join("alpha.json"),
            ),
            registration_for(
                "beta",
                "shared",
                root.clone(),
                temp.path().join("beta.json"),
            ),
        ],
        events.clone(),
    );
    let accepted = bridge
        .start_operation(request_for("alpha", "shared", OperationKind::Sync))
        .await
        .unwrap();
    let before = server.received_requests().await.unwrap().len();
    let rejected = bridge
        .start_operation(request_for("beta", "shared", OperationKind::Sync))
        .await
        .unwrap_err();
    assert_eq!(rejected.code, ErrorCode::OperationConflict);
    assert!(rejected.operation_id.is_none());
    assert_eq!(server.received_requests().await.unwrap().len(), before);
    assert_eq!(
        terminal(&bridge, &accepted).await.status,
        OperationStatus::Succeeded
    );
}

#[tokio::test]
async fn queued_correlation_terminal_recovery_and_real_base_semantics() {
    let server = MockServer::start().await;
    provider(&server, false).await;
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("install");
    let manifest = temp.path().join("manifest.json");
    let events = Events::default();
    let bridge = bridge(
        &server,
        vec![registration("one", root.clone(), manifest.clone())],
        events.clone(),
    );
    let missing = bridge
        .start_operation(request("one", OperationKind::Verify))
        .await
        .unwrap();
    let missing_done = terminal(&bridge, &missing).await;
    assert_eq!(missing_done.status, OperationStatus::Succeeded);
    assert_eq!(
        missing_done.result,
        Some(OperationResult::Verify {
            checked: 1,
            issues: vec![VerificationIssue {
                path: "mods/one.jar".into(),
                kind: VerificationIssueKind::Missing,
            }],
        })
    );
    assert!(!root.exists() && !manifest.exists());
    let accepted = bridge
        .start_operation(request("one", OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(accepted.status, OperationStatus::Queued);
    assert_eq!(accepted.instance_id, "one");
    assert_eq!(accepted.kind, OperationKind::Sync);
    assert!(accepted.result.is_none() && accepted.error.is_none());
    safe(&accepted);
    let done = terminal(&bridge, &accepted).await;
    assert_eq!(done.operation_id, accepted.operation_id);
    assert_eq!(done.status, OperationStatus::Succeeded);
    assert!(done.sequence > accepted.sequence);
    assert_eq!(
        done.result,
        Some(OperationResult::Sync {
            downloaded: 1,
            removed: 0
        })
    );
    assert!(done.error.is_none());
    assert!(manifest.is_file(), "success includes manifest persistence");
    assert_eq!(
        std::fs::read(root.join("mods/one.jar")).unwrap(),
        b"provider"
    );
    std::fs::write(root.join("mods/one.jar"), b"user edit").unwrap();
    std::fs::write(root.join("unrelated.txt"), b"keep").unwrap();
    let sync = bridge
        .start_operation(request("one", OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(
        terminal(&bridge, &sync).await.status,
        OperationStatus::Succeeded
    );
    assert_eq!(
        std::fs::read(root.join("mods/one.jar")).unwrap(),
        b"user edit"
    );
    let verify = bridge
        .start_operation(request("one", OperationKind::Verify))
        .await
        .unwrap();
    let verified = terminal(&bridge, &verify).await;
    assert_eq!(verified.status, OperationStatus::Succeeded);
    let issues = vec![VerificationIssue {
        path: "mods/one.jar".into(),
        kind: VerificationIssueKind::Different,
    }];
    assert_eq!(
        verified.result,
        Some(OperationResult::Verify {
            checked: 1,
            issues: issues.clone()
        })
    );
    assert!(verified.error.is_none());
    let repair = bridge
        .start_operation(request("one", OperationKind::Repair))
        .await
        .unwrap();
    let repaired = terminal(&bridge, &repair).await;
    assert_eq!(repaired.status, OperationStatus::Succeeded);
    assert_eq!(
        repaired.result,
        Some(OperationResult::Repair {
            verification: VerificationReport { checked: 1, issues },
            repaired: 1,
            downloaded: 1,
        })
    );
    assert_eq!(
        std::fs::read(root.join("mods/one.jar")).unwrap(),
        b"provider"
    );
    assert_eq!(std::fs::read(root.join("unrelated.txt")).unwrap(), b"keep");
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        bridge
            .get_operation(GetOperationRequest {
                operation_id: repair.operation_id.clone()
            })
            .await
            .unwrap(),
        repaired
    );
    let events = events.lock().unwrap();
    for operation in [&accepted, &sync, &verify, &repair] {
        let states: Vec<_> = events
            .iter()
            .filter(|s| s.operation_id == operation.operation_id)
            .collect();
        assert!(!states.is_empty());
        assert!(states.windows(2).all(|s| s[0].sequence < s[1].sequence));
        assert!(states
            .iter()
            .all(|s| s.instance_id == "one" && s.kind == operation.kind));
        assert!(states.iter().all(|s| s
            .progress
            .as_ref()
            .is_none_or(|p| p.stage != ProgressStage::Remove)));
        for state in states {
            safe(state);
            if let Some(progress) = &state.progress {
                if state.kind == OperationKind::Verify {
                    assert!(matches!(
                        progress.stage,
                        ProgressStage::FetchingFiles | ProgressStage::Verify
                    ));
                    assert!(
                        progress.current.is_none()
                            && progress.total.is_none()
                            && progress.unit.is_none()
                    );
                }
                if state.kind == OperationKind::Repair {
                    assert!(matches!(
                        progress.stage,
                        ProgressStage::FetchingFiles
                            | ProgressStage::Download
                            | ProgressStage::Verify
                    ));
                }
                if progress.stage == ProgressStage::Download {
                    assert_eq!(progress.unit, Some(ProgressUnit::Bytes));
                    assert_ne!(progress.total, Some(0));
                }
                if progress.stage == ProgressStage::Install {
                    assert_eq!(progress.unit, Some(ProgressUnit::Entries));
                }
                if progress.stage == ProgressStage::Extract {
                    assert_eq!(progress.unit, Some(ProgressUnit::Archives));
                }
            }
        }
    }
    assert_eq!(
        events
            .iter()
            .rev()
            .find(|s| s.operation_id == repair.operation_id),
        Some(&repaired)
    );
}

#[tokio::test]
async fn unregistered_and_outside_catalog_admission_has_no_side_effects() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("missing");
    let manifest = temp.path().join("missing.json");
    let events = Events::default();
    let bridge = bridge(
        &server,
        vec![registration("one", root.clone(), manifest.clone())],
        events.clone(),
    );
    let rejected = bridge
        .start_operation(request("unknown", OperationKind::Sync))
        .await
        .unwrap_err();
    assert_eq!(rejected.code, ErrorCode::NotFound);
    assert!(rejected.operation_id.is_none());
    safe(&rejected);
    let bad = bridge
        .list_instances(ListInstancesRequest {
            launcher_id: "outside".into(),
            limit: None,
            offset: None,
        })
        .await
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidRequest);
    let bad = bridge
        .get_instance(GetInstanceRequest {
            launcher_id: "outside".into(),
            instance_id: "one".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidRequest);
    let bad = bridge
        .get_instance_files(GetInstanceFilesRequest {
            launcher_id: "outside".into(),
            instance_id: "one".into(),
            code: None,
        })
        .await
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidRequest);
    for ids in [vec![], vec!["outside".into()], vec!["catalog".into(); 21]] {
        let bad = bridge
            .resolve_by_code(ResolveByCodeRequest {
                launcher_ids: ids,
                code: "PRIVATE_CODE".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(bad.code, ErrorCode::InvalidRequest);
        safe(&bad);
    }
    assert!(events.lock().unwrap().is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(!root.exists() && !manifest.exists());
    assert_eq!(
        bridge
            .get_operation(GetOperationRequest {
                operation_id: "unknown".into()
            })
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn conflicts_cover_same_root_nested_roots_and_shared_manifest_for_every_kind() {
    for kind in [
        OperationKind::Sync,
        OperationKind::Verify,
        OperationKind::Repair,
    ] {
        for shape in ["same", "nested", "manifest", "manifest_inside_root"] {
            let server = MockServer::start().await;
            provider(&server, true).await;
            let temp = TempDir::new().unwrap();
            let root = temp.path().join("install");
            let manifest = temp.path().join("manifest.json");
            let second_root = match shape {
                "same" => root.clone(),
                "nested" => root.join("not-created/yet"),
                _ => temp.path().join("other"),
            };
            let second_manifest = match shape {
                "manifest" => manifest.clone(),
                "manifest_inside_root" => root.join("control.json"),
                _ => temp.path().join("other.json"),
            };
            let events = Events::default();
            let bridge = bridge(
                &server,
                vec![
                    registration("one", root.clone(), manifest.clone()),
                    registration("two", second_root.clone(), second_manifest.clone()),
                ],
                events.clone(),
            );
            let first = bridge
                .start_operation(request("one", OperationKind::Sync))
                .await
                .unwrap();
            let same = bridge
                .start_operation(request("one", kind))
                .await
                .unwrap_err();
            assert_eq!(same.code, ErrorCode::OperationConflict);
            assert!(same.operation_id.is_none());
            let before_requests = server.received_requests().await.unwrap().len();
            let rejected = bridge
                .start_operation(request("two", kind))
                .await
                .unwrap_err();
            assert_eq!(rejected.code, ErrorCode::OperationConflict, "{shape}");
            assert!(rejected.operation_id.is_none());
            assert!(!second_root.exists() && !second_manifest.exists());
            assert!(events
                .lock()
                .unwrap()
                .iter()
                .all(|s| s.operation_id == first.operation_id));
            // First operation may issue its request concurrently; second must never do so.
            let requests = server.received_requests().await.unwrap();
            assert!(requests.len() >= before_requests);
            assert!(requests.iter().all(|r| !r.url.path().contains("/two/")));
            assert_eq!(
                terminal(&bridge, &first).await.status,
                OperationStatus::Succeeded
            );
            let next = bridge.start_operation(request("two", kind)).await.unwrap();
            assert_eq!(
                terminal(&bridge, &next).await.status,
                OperationStatus::Succeeded
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn canonical_symlink_alias_with_missing_descendants_cannot_evade_guard() {
    let server = MockServer::start().await;
    provider(&server, true).await;
    let temp = TempDir::new().unwrap();
    let actual = temp.path().join("actual");
    std::fs::create_dir(&actual).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    let bridge = bridge(
        &server,
        vec![
            registration("one", actual.join("missing"), temp.path().join("one.json")),
            registration(
                "two",
                alias.join("missing/child"),
                temp.path().join("two.json"),
            ),
        ],
        Events::default(),
    );
    let first = bridge
        .start_operation(request("one", OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(
        bridge
            .start_operation(request("two", OperationKind::Repair))
            .await
            .unwrap_err()
            .code,
        ErrorCode::OperationConflict
    );
    assert!(!actual.join("missing").exists());
    assert!(!temp.path().join("two.json").exists());
    assert_eq!(
        terminal(&bridge, &first).await.status,
        OperationStatus::Succeeded
    );
}

#[tokio::test]
async fn safe_query_projections_and_safe_variant_errors() {
    let server = MockServer::start().await;
    let instance = json!({"id":"one", "launcher_id":"catalog", "title":"Pack", "downloads":2,
        "code":"PRIVATE_CODE", "secret":"PRIVATE_EXTRA", "users":{"whitelist":["PRIVATE_EXTRA"]},
        "files":[{"downloads":["http://PRIVATE_URL"]}]});
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"results":[instance.clone()]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances/one"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":instance.clone()})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/instances/resolve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":instance})))
        .mount(&server)
        .await;
    provider(&server, false).await;
    let bridge = bridge(&server, vec![], Events::default());
    let listed = bridge
        .list_instances(ListInstancesRequest {
            launcher_id: "catalog".into(),
            limit: Some(10),
            offset: Some(0),
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title.as_deref(), Some("Pack"));
    assert_eq!(listed[0].downloads, 2);
    safe(&listed);
    let fetched = bridge
        .get_instance(GetInstanceRequest {
            launcher_id: "catalog".into(),
            instance_id: "one".into(),
        })
        .await
        .unwrap();
    assert_eq!(fetched, listed[0]);
    let resolved = bridge
        .resolve_by_code(ResolveByCodeRequest {
            launcher_ids: vec!["catalog".into()],
            code: "PRIVATE_CODE".into(),
        })
        .await
        .unwrap();
    assert_eq!(resolved, listed[0]);
    let files = bridge
        .get_instance_files(GetInstanceFilesRequest {
            launcher_id: "catalog".into(),
            instance_id: "one".into(),
            code: Some("PRIVATE_CODE".into()),
        })
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].size, Some(8));
    safe(&files);
    for (status, code) in [
        (401, ErrorCode::Unauthorized),
        (404, ErrorCode::NotFound),
        (429, ErrorCode::Api),
        (503, ErrorCode::Api),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/launchers/catalog/instances/error{status}")))
            .respond_with(ResponseTemplate::new(status).set_body_json(
                json!({"success":false,"error":"PRIVATE_CODE http://PRIVATE_URL PRIVATE_BEARER"}),
            ))
            .mount(&server)
            .await;
        let error = bridge
            .get_instance(GetInstanceRequest {
                launcher_id: "catalog".into(),
                instance_id: format!("error{status}"),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(error.operation_id.is_none() && error.stage.is_none());
        if code == ErrorCode::Api {
            assert_eq!(
                error.context,
                Some(ErrorContext {
                    http_status: status
                })
            );
        }
        safe(&error);
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
    }
    for (id, body, expected) in [
        ("generic", json!({"success":true}), ErrorCode::Internal),
        ("decode", json!({"data":{"id":42}}), ErrorCode::Decode),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/launchers/catalog/instances/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let error = bridge
            .get_instance(GetInstanceRequest {
                launcher_id: "catalog".into(),
                instance_id: id.into(),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, expected);
        assert!(error.context.is_none());
        safe(&error);
    }
}

#[tokio::test]
async fn execution_failure_is_terminal_and_releases_resource() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/launchers/catalog/instances/one/files"))
        .respond_with(
            ResponseTemplate::new(503)
                .set_body_json(json!({"error":"PRIVATE_CODE http://PRIVATE_URL"})),
        )
        .mount(&server)
        .await;
    let temp = TempDir::new().unwrap();
    let bridge = bridge(
        &server,
        vec![registration(
            "one",
            temp.path().join("root"),
            temp.path().join("manifest.json"),
        )],
        Events::default(),
    );
    let first = bridge
        .start_operation(request("one", OperationKind::Sync))
        .await
        .unwrap();
    let failed = terminal(&bridge, &first).await;
    assert_eq!(failed.status, OperationStatus::Failed);
    assert!(failed.result.is_none());
    let error = failed.error.as_ref().unwrap();
    assert_eq!(error.code, ErrorCode::Api);
    assert_eq!(
        error.operation_id.as_deref(),
        Some(first.operation_id.as_str())
    );
    assert_eq!(error.context, Some(ErrorContext { http_status: 503 }));
    safe(&failed);
    let second = bridge
        .start_operation(request("one", OperationKind::Verify))
        .await
        .unwrap();
    assert_ne!(second.operation_id, first.operation_id);
    assert_eq!(
        terminal(&bridge, &second).await.status,
        OperationStatus::Failed
    );
}

#[tokio::test]
async fn task_callback_panic_becomes_failure_and_does_not_strand_guard() {
    let server = MockServer::start().await;
    provider(&server, false).await;
    let temp = TempDir::new().unwrap();
    let panicked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let once = panicked.clone();
    let bridge = Bridge::new(
        Client::with_base_url(&server.uri()).unwrap(),
        vec!["catalog".into()],
        vec![registration(
            "one",
            temp.path().join("root"),
            temp.path().join("manifest.json"),
        )],
        Arc::new(move |snapshot: OperationSnapshot| {
            if snapshot.status == OperationStatus::Running
                && !once.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                panic!("PRIVATE_CODE");
            }
        }),
    )
    .unwrap();
    let first = bridge
        .start_operation(request("one", OperationKind::Sync))
        .await
        .unwrap();
    let failed = terminal(&bridge, &first).await;
    assert_eq!(failed.status, OperationStatus::Failed);
    assert_eq!(failed.error.as_ref().unwrap().code, ErrorCode::Internal);
    assert!(failed.result.is_none());
    safe(&failed);
    assert!(panicked.load(std::sync::atomic::Ordering::SeqCst));
    let next = bridge
        .start_operation(request("one", OperationKind::Sync))
        .await
        .unwrap();
    assert_eq!(
        terminal(&bridge, &next).await.status,
        OperationStatus::Succeeded
    );
}

#[test]
fn options_are_explicit_nulls_and_results_are_discriminated() {
    let value = serde_json::to_value(OperationSnapshot {
        operation_id: "op".into(),
        launcher_id: "catalog".into(),
        instance_id: "one".into(),
        kind: OperationKind::Verify,
        status: OperationStatus::Queued,
        sequence: 0,
        progress: None,
        result: None,
        error: None,
    })
    .unwrap();
    assert_eq!(
        value,
        json!({"operationId":"op","launcherId":"catalog","instanceId":"one","kind":"verify",
        "status":"queued","sequence":0,"progress":null,"result":null,"error":null})
    );
    let result: Value = serde_json::to_value(OperationResult::Verify {
        checked: 0,
        issues: vec![],
    })
    .unwrap();
    assert_eq!(result, json!({"kind":"verify","checked":0,"issues":[]}));
}
