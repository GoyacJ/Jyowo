pub mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(commands::runtime_state())
        .invoke_handler(tauri::generate_handler![
            commands::cancel_run,
            commands::create_attachment_from_path,
            commands::create_conversation,
            commands::delete_conversation,
            commands::delete_mcp_server,
            commands::delete_memory_item,
            commands::delete_skill,
            commands::export_memory_items,
            commands::export_support_bundle,
            commands::get_context_snapshot,
            commands::get_execution_settings,
            commands::get_app_info,
            commands::get_conversation,
            commands::get_memory_item,
            commands::get_provider_config_api_key,
            commands::get_skill,
            commands::get_replay_timeline,
            commands::harness_healthcheck,
            commands::import_skill,
            commands::list_activity,
            commands::list_artifacts,
            commands::list_conversations,
            commands::list_eval_cases,
            commands::list_reference_candidates,
            commands::list_model_provider_catalog,
            commands::list_mcp_servers,
            commands::list_memory_items,
            commands::list_provider_settings,
            commands::list_skills,
            commands::resolve_permission,
            commands::request_provider_config_api_key_reveal,
            commands::run_eval_case,
            commands::save_mcp_server,
            commands::save_provider_settings,
            commands::set_execution_settings,
            commands::set_conversation_model_config,
            commands::set_skill_enabled,
            commands::start_run,
            commands::subscribe_conversation_events,
            commands::update_memory_item,
            commands::unsubscribe_conversation_events,
            commands::validate_provider_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
