//! Shared by the handler, generator, and build-time Tauri permissions.
#[macro_export]
macro_rules! hostmylauncher_commands {
    ($consumer:ident) => {
        $consumer! {
            listInstances => list_instances,
            getInstance => get_instance,
            resolveByCode => resolve_by_code,
            getInstanceFiles => get_instance_files,
            startOperation => start_operation,
            getOperation => get_operation,
        }
    };
}
