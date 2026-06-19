pub mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(commands::runtime_state())
        .invoke_handler(tauri::generate_handler![
            commands::cancel_run,
            commands::delete_mcp_server,
            commands::delete_memory_item,
            commands::export_memory_items,
            commands::export_support_bundle,
            commands::get_context_snapshot,
            commands::get_app_info,
            commands::get_conversation,
            commands::get_memory_item,
            commands::get_replay_timeline,
            commands::harness_healthcheck,
            commands::list_activity,
            commands::list_artifacts,
            commands::list_conversations,
            commands::list_eval_cases,
            commands::list_mcp_servers,
            commands::list_memory_items,
            commands::resolve_permission,
            commands::run_eval_case,
            commands::save_mcp_server,
            commands::save_provider_settings,
            commands::start_run,
            commands::update_memory_item,
            commands::validate_provider_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
