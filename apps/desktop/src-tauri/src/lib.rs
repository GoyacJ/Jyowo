pub mod agent_supervisor;
pub mod commands;
pub mod project_registry;
pub mod skill_catalog;

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
        .setup(move |app| {
            let _scheduler =
                commands::spawn_automation_scheduler_on_tauri_runtime(managed_runtime.clone());
            let app_handle = app.handle().clone();
            let supervisor_runtime = managed_runtime.clone();
            tauri::async_runtime::spawn(async move {
                let state = supervisor_runtime.read().await.clone();
                let _ =
                    commands::ensure_agent_supervisor_sidecar_for_state(&app_handle, &state).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_project,
            commands::archive_background_agent,
            commands::cancel_run,
            commands::cancel_background_agent,
            commands::clear_mcp_diagnostics,
            commands::create_attachment_from_path,
            commands::create_conversation,
            commands::delete_background_agent,
            commands::delete_conversation,
            commands::delete_agent_profile,
            commands::delete_automation,
            commands::delete_mcp_server,
            commands::delete_memory_item,
            commands::delete_project,
            commands::delete_skill,
            commands::export_conversation_evidence,
            commands::export_memory_items,
            commands::export_support_bundle,
            commands::get_context_snapshot,
            commands::get_execution_settings,
            commands::get_app_info,
            commands::get_artifact_media_preview,
            commands::get_artifact_revision_content,
            commands::get_attachment_media_preview,
            commands::get_background_agent,
            commands::get_conversation,
            commands::get_conversation_command_output,
            commands::get_conversation_diff_patch,
            commands::get_memory_item,
            commands::get_mcp_server_config,
            commands::get_model_usage_summary,
            commands::list_official_quota_snapshots,
            commands::refresh_official_quota,
            commands::get_plugin_detail,
            commands::get_provider_config_api_key,
            commands::get_skill_detail,
            commands::get_skill_file,
            commands::get_skill_catalog_entry,
            commands::get_skill_catalog_file,
            commands::get_replay_timeline,
            commands::harness_healthcheck,
            commands::import_skill,
            commands::install_plugin_from_path,
            commands::install_skill_from_catalog,
            commands::list_skill_catalog_install_tasks,
            commands::list_activity,
            commands::list_artifacts,
            commands::list_automation_runs,
            commands::list_agent_profiles,
            commands::list_automations,
            commands::list_background_agents,
            commands::list_browser_mcp_presets,
            commands::list_conversations,
            commands::list_eval_cases,
            commands::list_reference_candidates,
            commands::list_skill_catalog_entries,
            commands::list_skill_catalog_sources,
            commands::list_model_provider_catalog,
            commands::list_mcp_servers,
            commands::list_mcp_diagnostics,
            commands::list_memory_items,
            commands::list_plugins,
            commands::list_provider_settings,
            commands::list_provider_capability_routes,
            commands::list_provider_capability_route_options,
            commands::list_projects,
            commands::switch_project,
            commands::list_skills,
            commands::page_conversation_timeline,
            commands::page_conversation_worktree,
            commands::pause_background_agent,
            commands::resolve_permission,
            commands::request_provider_config_api_key_reveal,
            commands::resume_background_agent,
            commands::run_automation_now,
            commands::run_eval_case,
            commands::save_agent_profile,
            commands::save_automation,
            commands::save_browser_mcp_preset,
            commands::save_mcp_server,
            commands::save_provider_settings,
            commands::save_provider_capability_route,
            commands::delete_provider_capability_route,
            commands::set_mcp_server_enabled,
            commands::set_automation_enabled,
            commands::set_execution_settings,
            commands::set_skill_enabled,
            commands::send_background_agent_input,
            commands::start_run,
            commands::subscribe_conversation_events,
            commands::subscribe_mcp_diagnostics,
            commands::update_memory_item,
            commands::unsubscribe_conversation_events,
            commands::unsubscribe_mcp_diagnostics,
            commands::uninstall_plugin,
            commands::restart_mcp_server,
            commands::reload_plugin,
            commands::set_plugin_enabled,
            commands::set_project_plugins_enabled,
            commands::update_plugin_config,
            commands::validate_plugin_from_path,
            commands::validate_provider_settings,
            commands::list_provider_probe_snapshots,
            commands::probe_provider_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
