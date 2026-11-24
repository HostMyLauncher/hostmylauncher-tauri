//! Tauri 2 bindings. Hosts enable `plugin` and supply their own runtime.
use crate::{contract::*, registry, Bridge};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Emitter, Manager, Runtime, State,
};

#[tauri::command]
async fn list_instances(
    service: State<'_, Bridge>,
    request: ListInstancesRequest,
) -> Result<Vec<InstanceDto>, BridgeError> {
    service.list_instances(request).await
}
#[tauri::command]
async fn get_instance(
    service: State<'_, Bridge>,
    request: GetInstanceRequest,
) -> Result<InstanceDto, BridgeError> {
    service.get_instance(request).await
}
#[tauri::command]
async fn resolve_by_code(
    service: State<'_, Bridge>,
    request: ResolveByCodeRequest,
) -> Result<InstanceDto, BridgeError> {
    service.resolve_by_code(request).await
}
#[tauri::command]
async fn get_instance_files(
    service: State<'_, Bridge>,
    request: GetInstanceFilesRequest,
) -> Result<Vec<FileDescriptorDto>, BridgeError> {
    service.get_instance_files(request).await
}
#[tauri::command]
async fn start_operation(
    service: State<'_, Bridge>,
    request: StartOperationRequest,
) -> Result<OperationSnapshot, BridgeError> {
    service.start_operation(request).await
}
#[tauri::command]
async fn watch_catalog(
    service: State<'_, Bridge>,
    request: WatchCatalogRequest,
) -> Result<Vec<InstanceDto>, BridgeError> {
    service.watch_catalog(request).await
}
#[tauri::command]
async fn get_operation(
    service: State<'_, Bridge>,
    request: GetOperationRequest,
) -> Result<OperationSnapshot, BridgeError> {
    service.get_operation(request).await
}

/// Owns the service for the app session and connects complete snapshots to emit.
/// Event delivery is best-effort; get_operation remains authoritative.
pub fn init<R: Runtime>(service: Bridge) -> TauriPlugin<R> {
    macro_rules! handlers {
        ($($key:ident => $command:ident,)*) => {
            tauri::generate_handler![#![plugin(hostmylauncher)] $($command),*]
        };
    }
    Builder::new(registry::PLUGIN_NAME)
        .invoke_handler(crate::hostmylauncher_commands!(handlers))
        .setup(move |app, _| {
            if !app.manage(service.clone()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "hostmylauncher service already managed",
                )
                .into());
            }
            let emitter = app.clone();
            service.add_event_sink(std::sync::Arc::new(move |snapshot| {
                let _ = emitter.emit(registry::OPERATION_EVENT, snapshot);
            }));
            let emitter = app.clone();
            service.add_catalog_sink(std::sync::Arc::new(move |snapshot| {
                let _ = emitter.emit(registry::CATALOG_EVENT, snapshot);
            }));
            Ok(())
        })
        .build()
}
