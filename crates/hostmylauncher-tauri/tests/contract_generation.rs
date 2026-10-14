use hostmylauncher_tauri::{contract::*, registry};
use ts_rs::TS;

#[test]
fn exact_identifiers_are_generated_from_registration_list() {
    assert_eq!(registry::PLUGIN_NAME, "hostmylauncher");
    assert_eq!(
        registry::COMMANDS,
        &[
            ("listInstances", "plugin:hostmylauncher|list_instances"),
            ("getInstance", "plugin:hostmylauncher|get_instance"),
            ("resolveByCode", "plugin:hostmylauncher|resolve_by_code"),
            (
                "getInstanceFiles",
                "plugin:hostmylauncher|get_instance_files"
            ),
            ("startOperation", "plugin:hostmylauncher|start_operation"),
            ("getOperation", "plugin:hostmylauncher|get_operation"),
        ]
    );
    assert_eq!(registry::OPERATION_EVENT, "hostmylauncher://operation");
    let generated = registry::typescript_contract();
    for (key, command) in registry::COMMANDS {
        assert!(generated.contains(&format!("{key}: '{command}'")));
    }
    assert!(generated.contains("export const OPERATION_EVENT = 'hostmylauncher://operation'"));
    assert!(
        !generated.contains("bigint"),
        "JSON wire counters are numbers"
    );
    assert!(generated.contains(&OperationSnapshot::decl()));
    assert!(generated.contains(&StartOperationRequest::decl()));
    assert!(generated.contains(&OperationResult::decl()));
    assert_eq!(generated, registry::typescript_contract());
}

#[test]
fn generated_file_matches_rust_source() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/tauri/src/generated/contract.ts");
    let actual = std::fs::read_to_string(path)
        .expect("parent must run generate-contract before this consistency test");
    assert_eq!(actual, registry::typescript_contract());
}
