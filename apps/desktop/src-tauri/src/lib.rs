pub mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::harness_healthcheck,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
