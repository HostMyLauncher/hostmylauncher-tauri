//! Build-only consumer: no window or production API requests are started.
pub fn context() -> tauri::Context<tauri::test::MockRuntime> {
    tauri::generate_context!()
}
