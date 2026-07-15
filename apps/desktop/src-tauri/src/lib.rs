pub mod commands;
pub mod daemon_client;
pub mod project_registry;
pub mod skill_catalog;
pub mod storage_layout;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let managed_runtime = commands::managed_runtime_state();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(
            project_registry::ProjectRegistry::load().expect("project registry should initialize"),
        )
        .manage(managed_runtime.clone())
        .manage(commands::DaemonBridgeState::default())
        .invoke_handler(tauri::generate_handler![
            commands::daemon_connect,
            commands::daemon_request,
            commands::daemon_subscribe,
            commands::daemon_unsubscribe,
            commands::daemon_read_blob,
            commands::daemon_stage_blob_from_path,
            commands::daemon_list_reference_candidates,
            commands::add_project,
            commands::clear_mcp_diagnostics,
            commands::clear_skill_secret,
            commands::delete_agent_profile,
            commands::delete_mcp_server,
            commands::delete_project,
            commands::delete_skill,
            commands::get_execution_settings,
            commands::get_default_workspace,
            commands::get_app_info,
            commands::get_mcp_server_config,
            commands::get_model_settings_page,
            commands::get_model_usage_summary,
            commands::list_official_quota_snapshots,
            commands::refresh_model_provider_catalog,
            commands::refresh_official_quota,
            commands::get_plugin_detail,
            commands::get_provider_config_api_key,
            commands::get_runtime_execution_status,
            commands::get_skill_detail,
            commands::get_skill_config,
            commands::get_skill_file,
            commands::get_skill_catalog_entry,
            commands::get_skill_catalog_file,
            commands::import_skill,
            commands::install_plugin_from_path,
            commands::install_skill_from_catalog,
            commands::list_skill_catalog_install_tasks,
            commands::list_agent_profiles,
            commands::list_browser_mcp_presets,
            commands::list_skill_catalog_entries,
            commands::list_skill_catalog_sources,
            commands::list_model_provider_catalog,
            commands::list_mcp_servers,
            commands::list_mcp_diagnostics,
            commands::list_plugins,
            commands::list_provider_settings,
            commands::list_provider_capability_routes,
            commands::list_provider_capability_route_options,
            commands::list_projects,
            commands::list_runtime_tools,
            commands::reset_runtime_tools,
            commands::move_project,
            commands::rename_project,
            commands::switch_project,
            commands::list_skills,
            commands::request_provider_config_api_key_reveal,
            commands::save_agent_profile,
            commands::save_browser_mcp_preset,
            commands::save_mcp_server,
            commands::save_provider_settings,
            commands::save_provider_capability_route,
            commands::delete_provider_capability_route,
            commands::set_mcp_server_enabled,
            commands::set_execution_settings,
            commands::set_skill_config_value,
            commands::set_skill_secret,
            commands::set_skill_enabled,
            commands::subscribe_mcp_diagnostics,
            commands::unsubscribe_mcp_diagnostics,
            commands::uninstall_plugin,
            commands::restart_mcp_server,
            commands::reload_plugin,
            commands::set_plugin_enabled,
            commands::set_project_plugins_enabled,
            commands::set_runtime_tool_enabled,
            commands::update_plugin_config,
            commands::validate_plugin_from_path,
            commands::validate_provider_settings,
            commands::list_provider_probe_snapshots,
            commands::probe_provider_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
