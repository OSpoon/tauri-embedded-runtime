mod runtime;

use tauri::{Manager, RunEvent, WindowEvent};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(runtime::ServiceState::default())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                runtime::commands::shutdown_runtime(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            runtime::commands::runtime_status,
            runtime::commands::runtime_setup_plan,
            runtime::commands::runtime_projects_catalog,
            runtime::commands::runtime_bootstrap,
            runtime::commands::runtime_install,
            runtime::commands::runtime_repair,
            runtime::commands::runtime_repair_component,
            runtime::commands::runtime_start_services,
            runtime::commands::runtime_projects_status,
            runtime::commands::runtime_projects_install,
            runtime::commands::runtime_projects_repair,
            runtime::commands::runtime_project_repair,
            runtime::commands::runtime_cancel,
            runtime::commands::runtime_stop_services,
            runtime::commands::runtime_service_status
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if matches!(event, RunEvent::Exit) {
                runtime::commands::shutdown_runtime(app_handle);
            }
        });
}
