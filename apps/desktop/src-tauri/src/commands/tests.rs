use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::error::runtime_operation_failed;
    use crate::commands::mcp::mcp_stdio_env;
    use crate::commands::plugins::{plugin_package_dir_name, redact_plugin_detail_config_values};
    use crate::commands::runtime::{
        desktop_cargo_extension_search_paths, desktop_settings_permission_rules,
    };
    use crate::commands::skills::{
        emit_skill_catalog_install_progress, get_or_create_skill_catalog_install_task,
        record_skill_catalog_install_task_progress, skill_catalog_install_stage,
    };
    use crate::commands::stores::ensure_plugin_package_dir_name;
    use crate::commands::validation::mcp_safe_connection_error_message;
    use harness_contracts::{
        ActionPlanHash, Decision, DecisionScope, PermissionSubject, PluginProductState,
        PluginSourceKind, RequestId, Severity, ToolUseId,
    };
    use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
    use std::path::Path;

    #[test]
    fn skill_catalog_progress_payload_serializes_camel_case() {
        let payload = SkillCatalogInstallProgressPayload {
            operation_id: "catalog-install-001".to_owned(),
            source_id: "anthropic".to_owned(),
            entry_id: "anthropic:frontend-design".to_owned(),
            version: Some("main".to_owned()),
            stage: "downloading",
            percent: 45,
            message: None,
        };

        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({
                "operationId": "catalog-install-001",
                "sourceId": "anthropic",
                "entryId": "anthropic:frontend-design",
                "version": "main",
                "stage": "downloading",
                "percent": 45
            })
        );
        assert_eq!(skill_catalog_install_stage("unknown"), "preparing");
    }

    #[test]
    fn mcp_stdio_empty_inherit_env_uses_empty_environment_for_plain_commands() {
        let env = mcp_stdio_env("node", &[], &[]);

        assert!(matches!(env, StdioEnv::Empty { extra } if extra.is_empty()));
    }

    #[test]
    fn mcp_stdio_empty_inherit_env_adds_execution_env_for_package_runners() {
        let env = mcp_stdio_env("npx", &[], &[]);

        assert!(matches!(env, StdioEnv::Allowlist { inherit, extra }
            if inherit == BTreeSet::from([
                "HOME".to_owned(),
                "PATH".to_owned(),
                "TMPDIR".to_owned(),
                "USER".to_owned(),
            ]) && extra.is_empty()));
    }

    #[test]
    fn mcp_stdio_inherit_env_rejects_secret_bearing_names() {
        let error =
            harness_contracts::validate_persisted_mcp_transport(&McpServerTransportConfig::Stdio {
                command: "npx".to_owned(),
                args: vec!["@playwright/mcp@latest".to_owned()],
                env: Vec::new(),
                inherit_env: vec!["LINEAR_API_KEY".to_owned()],
                working_dir: None,
            })
            .expect_err("secret-bearing inherited env names should be rejected");

        assert_eq!(error, harness_contracts::PersistedMcpValidationError::Stdio);
    }

    #[tokio::test]
    async fn desktop_settings_permission_rules_allow_mcp_transport_without_interactivity() {
        let broker = harness_permission::RuleEngineBroker::builder()
            .with_tenant(TenantId::SINGLE)
            .with_rules(desktop_settings_permission_rules())
            .build()
            .await
            .unwrap();
        let session_id = SessionId::new();
        let tool_use_id = ToolUseId::new();
        let request = PermissionRequest {
            request_id: RequestId::new(),
            tenant_id: TenantId::SINGLE,
            session_id,
            tool_use_id,
            tool_name: "mcp_transport".to_owned(),
            subject: PermissionSubject::Custom {
                kind: "mcp_transport".to_owned(),
                payload: serde_json::json!({}),
            },
            severity: Severity::Medium,
            scope_hint: DecisionScope::ToolName("mcp_transport".to_owned()),
            action_plan_hash: ActionPlanHash::default(),
            decision_options: Vec::new(),
            confirmation_expected: None,
            created_at: Utc::now(),
        };
        let context = PermissionContext {
            permission_mode: PermissionMode::Default,
            previous_mode: None,
            session_id,
            tenant_id: TenantId::SINGLE,
            run_id: Some(RunId::new()),
            interactivity: InteractivityLevel::NoInteractive,
            timeout_policy: None,
            fallback_policy: FallbackPolicy::AskUser,
            hook_overrides: Vec::new(),
        };

        assert_eq!(broker.decide(request, context).await, Decision::AllowOnce);
    }

    #[test]
    fn mcp_connection_error_distinguishes_authorization_from_process_permissions() {
        assert_eq!(
            mcp_safe_connection_error_message("permission denied: authorization failed: DenyOnce"),
            "MCP server permission was denied."
        );
        assert_eq!(
            mcp_safe_connection_error_message("transport: Permission denied (os error 13)"),
            "MCP server command could not be executed."
        );
    }

    #[test]
    fn skill_catalog_progress_emit_requires_operation_id_and_clamps_percent() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let emitter: Option<SkillCatalogInstallProgressEmitter> = Some(Arc::new(move |payload| {
            captured_events.lock().unwrap().push(payload);
            Ok(())
        }));
        let request = InstallSkillFromCatalogRequest {
            source_id: "anthropic".to_owned(),
            entry_id: "anthropic:frontend-design".to_owned(),
            version: Some("main".to_owned()),
            operation_id: Some("catalog-install-001".to_owned()),
        };

        emit_skill_catalog_install_progress(&emitter, &request, "downloading", 250, None).unwrap();

        let recorded_events = events.lock().unwrap();
        assert_eq!(recorded_events.len(), 1);
        assert_eq!(recorded_events[0].stage, "downloading");
        assert_eq!(recorded_events[0].percent, 100);
        drop(recorded_events);

        let request_without_operation = InstallSkillFromCatalogRequest {
            operation_id: None,
            ..request
        };
        emit_skill_catalog_install_progress(
            &emitter,
            &request_without_operation,
            "downloading",
            25,
            None,
        )
        .unwrap();

        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn skill_catalog_install_tasks_are_keyed_by_operation_id() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let request = InstallSkillFromCatalogRequest {
            source_id: "anthropic".to_owned(),
            entry_id: "anthropic:frontend-design".to_owned(),
            version: Some("main".to_owned()),
            operation_id: Some("catalog-install-001".to_owned()),
        };
        let duplicate_request = InstallSkillFromCatalogRequest {
            operation_id: Some("catalog-install-002".to_owned()),
            ..request.clone()
        };

        let first = get_or_create_skill_catalog_install_task(&state, &request).unwrap();
        let duplicate =
            get_or_create_skill_catalog_install_task(&state, &duplicate_request).unwrap();
        record_skill_catalog_install_task_progress(&state, &request, "downloading", 45, None)
            .await
            .unwrap();
        let tasks = list_skill_catalog_install_tasks_with_runtime_state(&state)
            .await
            .unwrap();

        assert_ne!(duplicate.operation_id, first.operation_id);
        assert_eq!(tasks.tasks.len(), 2);
        assert_eq!(tasks.tasks[0].operation_id, "catalog-install-001");
        assert_eq!(tasks.tasks[0].stage, "downloading");
        assert_eq!(tasks.tasks[0].percent, 45);
        assert_eq!(tasks.tasks[0].status, "running");
    }

    #[tokio::test]
    async fn plugin_install_failure_does_not_write_store_record() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let old_field = ["manifest", "schema", "version"].join("_");
        std::fs::write(
            source.path().join("plugin.json"),
            format!(r#"{{"{old_field}":99,"name":"bad-plugin"}}"#),
        )
        .unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();

        let result = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let plugins = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(result.status, PluginOperationStatus::Rejected);
        let report = result.report.as_ref().expect("rejection includes report");
        assert_eq!(report.source_path, "<local-plugin>");
        assert_eq!(
            report.reason.as_deref(),
            Some("plugin manifest is invalid.")
        );
        assert!(!report
            .source_path
            .contains(source_path.to_string_lossy().as_ref()));
        assert!(!report
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains(source_path.to_string_lossy().as_ref()));
        assert!(plugins.plugins.is_empty());
    }

    #[tokio::test]
    async fn installed_plugin_can_be_listed_and_disabled_without_activation() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "local-tools");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();

        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();
        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(install.status, PluginOperationStatus::Installed);
        assert_eq!(listed.plugins.len(), 1);
        assert_eq!(listed.plugins[0].id, installed_id);
        assert!(!listed.plugins[0].enabled);
        assert!(matches!(
            listed.plugins[0].state,
            PluginProductState::Disabled { .. }
        ));

        let disabled = set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: installed_id.clone(),
                enabled: false,
            },
            &state,
        )
        .await
        .unwrap();
        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(disabled.status, PluginOperationStatus::Disabled);
        assert_eq!(listed.plugins[0].id, installed_id);
        assert!(!listed.plugins[0].enabled);
        assert!(matches!(
            listed.plugins[0].state,
            PluginProductState::Disabled { .. }
        ));
    }

    #[tokio::test]
    async fn unregistered_user_plugin_package_is_rejected_by_desktop_registry() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "registered-tools");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        let rogue_package = state.plugin_store.package_root().join("rogue-tools_0.1.0");
        std::fs::create_dir_all(&rogue_package).unwrap();
        write_desktop_plugin_package(&rogue_package, "rogue-tools");

        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(listed.plugins.len(), 1);
        assert_eq!(listed.plugins[0].name, "registered-tools");
        assert!(listed
            .plugins
            .iter()
            .all(|plugin| plugin.name != "rogue-tools"));
    }

    #[tokio::test]
    async fn installing_file_plugin_without_sidecar_is_rejected() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest(source.path(), "local-preflight");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();
        let result = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(result.status, PluginOperationStatus::Rejected);
        assert_eq!(
            result
                .report
                .as_ref()
                .and_then(|report| report.reason.as_deref()),
            Some("local plugin package must include a jyowo-plugin-* sidecar executable")
        );
        assert!(listed.plugins.is_empty());
    }

    #[tokio::test]
    async fn plugin_config_update_preserves_existing_secret_config_fields() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_config_schema(source.path(), "secret-tools");
        write_desktop_plugin_sidecar(source.path(), "secret-tools");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();

        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();
        let mut settings = state.plugin_store.load_record().unwrap();
        settings.records[0].config =
            serde_json::json!({ "apiToken": "managed-secret-ref", "lineWidth": 80 });
        state.plugin_store.save_record(&settings).unwrap();

        update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id.clone(),
                values: serde_json::json!({ "lineWidth": 120 }),
            },
            &state,
        )
        .await
        .unwrap();

        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({ "apiToken": "managed-secret-ref", "lineWidth": 120 })
        );
        let detail = get_plugin_detail_with_runtime_state(
            GetPluginDetailRequest {
                plugin_id: installed_id,
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(
            detail.plugin.config,
            serde_json::json!({ "lineWidth": 120 })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_validates_merged_config_values() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_required_config_schema(source.path(), "merged-config");
        write_desktop_plugin_sidecar(source.path(), "merged-config");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let installed_id = PluginId("merged-config@0.1.0".to_owned());
        let package_dir = plugin_package_dir_name(&installed_id);
        state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .unwrap();
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: installed_id.clone(),
                    name: "merged-config".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: serde_json::json!({ "mode": "default", "limit": 10 }),
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .unwrap();

        update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "limit": 20 }),
            },
            &state,
        )
        .await
        .expect("merged config satisfies required schema fields");

        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({ "mode": "default", "limit": 20 })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_rejects_unknown_schema_fields_without_persisting() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_required_config_schema(source.path(), "strict-config");
        write_desktop_plugin_sidecar(source.path(), "strict-config");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let installed_id = PluginId("strict-config@0.1.0".to_owned());
        let package_dir = plugin_package_dir_name(&installed_id);
        state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .unwrap();
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: installed_id.clone(),
                    name: "strict-config".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: serde_json::json!({ "mode": "default", "limit": 10 }),
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .unwrap();

        let result = update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "unknown": "ignored" }),
            },
            &state,
        )
        .await;

        assert!(result.is_err());
        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({ "mode": "default", "limit": 10 })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_rejects_existing_unknown_schema_fields_without_persisting_patch()
    {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_required_config_schema(source.path(), "strict-existing");
        write_desktop_plugin_sidecar(source.path(), "strict-existing");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let installed_id = PluginId("strict-existing@0.1.0".to_owned());
        let package_dir = plugin_package_dir_name(&installed_id);
        state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .unwrap();
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: installed_id.clone(),
                    name: "strict-existing".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: serde_json::json!({
                        "mode": "default",
                        "unknown": "already-present"
                    }),
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .unwrap();

        let result = update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "limit": 20 }),
            },
            &state,
        )
        .await;

        assert!(result.is_err());
        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({
                "mode": "default",
                "unknown": "already-present"
            })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_rejects_secret_like_fields_without_secret_schema() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest(source.path(), "plain-config");
        write_desktop_plugin_sidecar(source.path(), "plain-config");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();
        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();

        let result = update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "apiToken": "not-even-a-real-token" }),
            },
            &state,
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn plugin_detail_config_without_schema_is_not_returned() {
        let redacted = redact_plugin_detail_config_values(
            None,
            serde_json::json!({
                "apiToken": "not-even-a-real-token",
                "mode": "default"
            }),
        );

        assert_eq!(redacted, serde_json::Value::Null);
    }

    #[test]
    fn plugin_detail_config_drops_secret_and_unknown_fields() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "apiToken": { "type": "string", "secret": true },
                "mode": { "type": "string" },
                "nested": {
                    "type": "object",
                    "properties": {
                        "visible": { "type": "string" },
                        "secretValue": { "type": "string", "secret": true }
                    }
                }
            },
            "additionalProperties": false
        });
        let redacted = redact_plugin_detail_config_values(
            Some(&schema),
            serde_json::json!({
                "apiToken": "not-even-a-real-token",
                "mode": "default",
                "unknown": "stored-before-schema",
                "nested": {
                    "visible": "ok",
                    "secretValue": "hidden",
                    "extra": "hidden"
                }
            }),
        );

        assert_eq!(
            redacted,
            serde_json::json!({
                "mode": "default",
                "nested": {
                    "visible": "ok"
                }
            })
        );
    }

    #[tokio::test]
    async fn project_plugin_allow_gate_is_persisted_by_command() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();

        let response = set_project_plugins_enabled_with_runtime_state(
            SetProjectPluginsEnabledRequest { enabled: true },
            &state,
        )
        .await
        .unwrap();

        assert!(response.allow_project_plugins);
        assert!(
            state
                .plugin_store
                .load_record()
                .unwrap()
                .allow_project_plugins
        );
        assert!(state
            .project_config_store
            .as_ref()
            .unwrap()
            .load_project_plugin_selection_if_present()
            .unwrap()
            .is_none());
        assert!(
            list_plugins_with_runtime_state(&state)
                .await
                .unwrap()
                .allow_project_plugins
        );
    }

    #[tokio::test]
    async fn enabling_cargo_extension_plugin_does_not_run_activate_preflight() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        write_desktop_plugin_manifest(source.path(), "counting-sidecar");
        let binary = source.path().join("jyowo-plugin-counting-sidecar");
        let counter = workspace.path().join("activate-count");
        write_desktop_executable(
            &binary,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
cat "$0.metadata"
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf activate >> '{}'
    printf '{{"jsonrpc":"2.0","id":1,"result":{{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{{"jsonrpc":"2.0","id":1,"result":null}}'
    exit 0
    ;;
esac
fi
	exit 2
	"#,
                counter.display()
            ),
        );
        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: install.plugin_id.unwrap(),
                enabled: true,
            },
            &state,
        )
        .await
        .unwrap();

        assert!(
            !counter.exists(),
            "enable preflight must not execute sidecar activate"
        );
    }

    #[tokio::test]
    async fn enabling_plugin_rejects_installed_package_hash_mismatch() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "tampered-sidecar");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();
        let settings = state.plugin_store.load_record().unwrap();
        let package_dir = settings.records[0].package_dir.clone();
        write_desktop_executable(
            &state
                .plugin_store
                .package_root()
                .join(&package_dir)
                .join("jyowo-plugin-tampered-sidecar"),
            r#"#!/bin/sh
printf tampered
exit 0
"#,
        );

        let result = set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: installed_id,
                enabled: true,
            },
            &state,
        )
        .await;

        assert!(result.is_err());
        let settings = state.plugin_store.load_record().unwrap();
        assert!(!settings.records[0].enabled);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn plugin_validation_rejects_world_writable_source_directory() {
        use std::os::unix::fs::PermissionsExt;

        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest(source.path(), "world-writable-plugin");
        let mut permissions = std::fs::metadata(source.path()).unwrap().permissions();
        permissions.set_mode(0o777);
        std::fs::set_permissions(source.path(), permissions).unwrap();
        let state = DesktopRuntimeState::with_workspace_for_test(
            tempfile::tempdir().unwrap().path().to_path_buf(),
        )
        .unwrap();

        let error = validate_plugin_from_path_with_runtime_state(
            ValidatePluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .expect_err("world-writable plugin source must be rejected");

        assert!(error.message.contains("world-writable"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn plugin_validation_rejects_world_writable_source_ancestor() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("writable-parent");
        let source = parent.join("plugin");
        std::fs::create_dir_all(&source).unwrap();
        write_desktop_plugin_manifest(&source, "world-writable-parent-plugin");
        let mut permissions = std::fs::metadata(&parent).unwrap().permissions();
        permissions.set_mode(0o777);
        std::fs::set_permissions(&parent, permissions).unwrap();
        let state = DesktopRuntimeState::with_workspace_for_test(
            tempfile::tempdir().unwrap().path().to_path_buf(),
        )
        .unwrap();

        let error = validate_plugin_from_path_with_runtime_state(
            ValidatePluginFromPathRequest {
                source_path: source.canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .expect_err("world-writable plugin source ancestor must be rejected");

        assert!(error.message.contains("world-writable"));
    }

    #[test]
    fn plugin_package_dir_validation_rejects_path_like_values() {
        for value in [".", "..", ".hidden", "nested/path", "nested\\path"] {
            assert!(
                ensure_plugin_package_dir_name(value).is_err(),
                "{value} must be rejected"
            );
        }

        ensure_plugin_package_dir_name("formatter_0.1.0").unwrap();
    }

    #[test]
    fn plugin_store_rejects_tampered_package_dir_in_index() {
        let workspace = tempfile::tempdir().unwrap();
        let workspace = workspace.path().canonicalize().unwrap();
        let layout = crate::storage_layout::StorageLayout::new(
            crate::storage_layout::JyowoHome::new(workspace.join(".jyowo-home")),
        );
        let store = DesktopPluginStore::project(layout, workspace);
        let index_path = store.index_path();
        std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
        let record = serde_json::json!({
            "records": [{
                "pluginId": "formatter@0.1.0",
                "name": "formatter",
                "version": "0.1.0",
                "enabled": true,
                "packageDir": "..",
                "sourcePath": "<local-plugin>",
                "contentHash": "hash",
                "importedAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:00:00Z",
                "config": null
            }]
        });
        std::fs::write(index_path, serde_json::to_vec(&record).unwrap()).unwrap();

        let error = store
            .load_record()
            .expect_err("tampered index must fail closed");

        assert!(error.message.contains("plugin package directory"));
    }

    #[tokio::test]
    async fn desktop_cargo_extension_search_path_discovers_workspace_owned_sidecar() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        write_desktop_cargo_extension(
            &state.plugin_store.cargo_extension_root(),
            "standalone-tools",
        );

        let response = list_plugins_with_runtime_state(&state).await.unwrap();

        assert!(response.plugins.iter().any(|plugin| {
            plugin.id == PluginId("standalone-tools@0.1.0".to_owned())
                && plugin.source == PluginSourceKind::CargoExtension
        }));
    }

    #[tokio::test]
    async fn missing_plugin_selection_uses_current_index_enabled_state() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let plugin_id = PluginId("current-tools@0.1.0".to_owned());
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "current-tools");
        let package_dir = plugin_package_dir_name(&plugin_id);
        let content_hash = state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .expect("write project plugin package");
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: plugin_id.clone(),
                    name: "current-tools".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: true,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash,
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: Value::Null,
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .expect("save project plugin index");

        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert!(!workspace.path().join(".jyowo/config/plugins.json").exists());
        assert!(listed.plugins.iter().any(|plugin| {
            plugin.id == plugin_id && plugin.name == "current-tools" && plugin.enabled
        }));
    }

    #[tokio::test]
    async fn project_plugin_selection_blocks_unselected_project_source_plugins() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let project_plugin_dir = workspace.path().join("plugin");
        std::fs::create_dir_all(&project_plugin_dir).unwrap();
        write_desktop_plugin_package(&project_plugin_dir, "project-rogue");
        state
            .project_config_store
            .as_ref()
            .expect("project config")
            .save_project_plugin_selection(&harness_contracts::PluginSelectionRecord {
                allow_project_plugins: true,
                enabled: Vec::new(),
            })
            .expect("save project plugin selection");

        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert!(listed
            .plugins
            .iter()
            .all(|plugin| plugin.name != "project-rogue"));
    }

    #[tokio::test]
    async fn global_plugin_selection_id_does_not_enable_project_record_collision() {
        let workspace = tempfile::tempdir().unwrap();
        let workspace_root = workspace.path().canonicalize().unwrap();
        let state = DesktopRuntimeState::with_workspace_for_test(workspace_root.clone()).unwrap();
        let plugin_id = PluginId("shared-collision@0.1.0".to_owned());
        let layout = state
            .global_config_store
            .as_ref()
            .expect("global config")
            .layout()
            .clone();
        let global_store = DesktopPluginStore::global(layout.clone());
        let project_store = DesktopPluginStore::project(layout, workspace_root);
        let global_source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(global_source.path(), "shared-collision");
        let package_dir = plugin_package_dir_name(&plugin_id);
        let global_content_hash = global_store
            .write_plugin_package(&package_dir, &global_source.path().canonicalize().unwrap())
            .expect("write global plugin package");
        global_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: plugin_id.clone(),
                    name: "shared-collision".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: true,
                    package_dir: package_dir.clone(),
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: global_content_hash,
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: Value::Null,
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .expect("save global plugin index");
        let project_source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(project_source.path(), "shared-collision");
        let project_content_hash = project_store
            .write_plugin_package(&package_dir, &project_source.path().canonicalize().unwrap())
            .expect("write project plugin package");
        project_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: plugin_id.clone(),
                    name: "shared-collision".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: project_content_hash,
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: Value::Null,
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .expect("save project plugin index");
        state
            .project_config_store
            .as_ref()
            .expect("project config")
            .save_project_plugin_selection(&harness_contracts::PluginSelectionRecord {
                allow_project_plugins: false,
                enabled: vec![plugin_id.0.clone()],
            })
            .expect("save project plugin selection");

        let listed = list_plugins_with_runtime_state(&state).await.unwrap();
        let plugin = listed
            .plugins
            .iter()
            .find(|plugin| plugin.name == "shared-collision")
            .expect("shared plugin should be discovered");

        assert_eq!(plugin.id, plugin_id);
        assert!(plugin.enabled);
    }

    #[tokio::test]
    async fn project_record_cannot_override_global_disabled_plugin_name() {
        let workspace = tempfile::tempdir().unwrap();
        let workspace_root = workspace.path().canonicalize().unwrap();
        let state = DesktopRuntimeState::with_workspace_for_test(workspace_root.clone()).unwrap();
        let global_plugin_id = PluginId("shared-tools@0.1.0".to_owned());
        let project_plugin_id = PluginId("shared-tools@0.1.1".to_owned());
        let layout = state
            .global_config_store
            .as_ref()
            .expect("global config")
            .layout()
            .clone();
        let global_store = DesktopPluginStore::global(layout.clone());
        let project_store = DesktopPluginStore::project(layout, workspace_root);
        let global_source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(global_source.path(), "shared-tools");
        let global_package_dir = "shared-tools_0.1.0".to_owned();
        let global_content_hash = global_store
            .write_plugin_package(
                &global_package_dir,
                &global_source.path().canonicalize().unwrap(),
            )
            .expect("write global plugin package");
        global_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: global_plugin_id.clone(),
                    name: "shared-tools".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir: global_package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: global_content_hash,
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: Value::Null,
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .expect("save global plugin index");
        let project_source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(project_source.path(), "shared-tools");
        let project_package_dir = "shared-tools_0.1.1".to_owned();
        let project_content_hash = project_store
            .write_plugin_package(
                &project_package_dir,
                &project_source.path().canonicalize().unwrap(),
            )
            .expect("write project plugin package");
        project_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: project_plugin_id.clone(),
                    name: "shared-tools".to_owned(),
                    version: "0.1.1".to_owned(),
                    enabled: true,
                    package_dir: project_package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: project_content_hash,
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: Value::Null,
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .expect("save project plugin index");
        state
            .project_config_store
            .as_ref()
            .expect("project config")
            .save_project_plugin_selection(&harness_contracts::PluginSelectionRecord {
                allow_project_plugins: false,
                enabled: vec![project_plugin_id.0.clone()],
            })
            .expect("save project plugin selection");

        let listed = list_plugins_with_runtime_state(&state).await.unwrap();
        let plugin = listed
            .plugins
            .iter()
            .find(|plugin| plugin.name == "shared-tools")
            .expect("shared plugin should be discovered");

        assert_eq!(plugin.id, global_plugin_id);
        assert_eq!(plugin.version, "0.1.0");
        assert!(!plugin.enabled);
        assert!(matches!(plugin.state, PluginProductState::Disabled { .. }));
        assert!(listed
            .plugins
            .iter()
            .all(|plugin| plugin.id != project_plugin_id));
    }

    #[tokio::test]
    async fn plugin_uninstall_does_not_delete_package_when_index_save_fails() {
        let workspace = tempfile::tempdir().unwrap();
        let plugin_id = PluginId("formatter@0.1.0".to_owned());
        let store = Arc::new(FailingSavePluginStore::new(PluginSettingsRecord {
            records: vec![PluginStoreRecord {
                plugin_id: plugin_id.clone(),
                name: "formatter".to_owned(),
                version: "0.1.0".to_owned(),
                enabled: true,
                package_dir: "formatter_0.1.0".to_owned(),
                source_path: "/tmp/formatter".to_owned(),
                content_hash: "hash".to_owned(),
                imported_at: "2026-01-01T00:00:00Z".to_owned(),
                updated_at: "2026-01-01T00:00:00Z".to_owned(),
                config: Value::Null,
                last_validation_error: None,
            }],
            ..PluginSettingsRecord::default()
        }));
        let mut state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        state.plugin_store = store.clone();

        let result =
            uninstall_plugin_with_runtime_state(UninstallPluginRequest { plugin_id }, &state).await;

        assert!(result.is_err());
        assert!(store.deleted_packages().is_empty());
    }

    #[test]
    fn desktop_cargo_extension_search_paths_use_workspace_owned_extension_dir() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();

        let paths = desktop_cargo_extension_search_paths(state.plugin_store.as_ref());

        assert_eq!(paths, vec![state.plugin_store.cargo_extension_root()]);
    }

    fn write_desktop_plugin_manifest(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "tools": [{ "name": "local-tool", "destructive": false }]
            }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_desktop_plugin_manifest_with_config_schema(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "configuration_schema": {
                    "type": "object",
                    "required": ["apiToken"],
                    "properties": {
                        "apiToken": { "type": "string", "secret": true },
                        "lineWidth": { "type": "number" }
                    },
                    "additionalProperties": false
                },
                "tools": [{ "name": "local-tool", "destructive": false }]
            }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_desktop_plugin_manifest_with_required_config_schema(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "configuration_schema": {
                    "type": "object",
                    "required": ["mode"],
                    "properties": {
                        "mode": { "type": "string" },
                        "limit": { "type": "number" }
                    },
                    "additionalProperties": false
                }
            }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_desktop_plugin_package(root: &Path, name: &str) {
        write_desktop_plugin_manifest(root, name);
        write_desktop_plugin_sidecar(root, name);
    }

    fn write_desktop_cargo_extension(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "tools": [{ "name": "local-tool", "destructive": false }]
            }
        });
        let metadata = serde_json::json!({
            "manifest": manifest,
            "package_metadata": { "package": name }
        });
        write_desktop_executable(
            &root.join(format!("jyowo-plugin-{name}")),
            format!(
                r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
printf '%s' '{}'
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
  printf '{{"jsonrpc":"2.0","id":1,"result":{{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}}}'
  exit 0
fi
exit 2
"#,
                metadata
            ),
        );
    }

    fn write_desktop_plugin_sidecar(root: &Path, name: &str) {
        write_desktop_executable(
            &root.join(format!("jyowo-plugin-{name}")),
            r#"#!/bin/sh
if [ "$1" = "--harness-runtime" ]; then
  printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
  exit 0
fi
exit 2
"#,
        );
    }

    fn write_desktop_executable(path: &Path, content: impl AsRef<str>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content.as_ref()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).unwrap();
        }
    }

    #[test]
    fn normalize_probe_timeout_ms_uses_default_and_clamps_bounds() {
        use crate::commands::model_settings::{
            normalize_probe_timeout_ms, DEFAULT_PROBE_TIMEOUT_MS, MAX_PROBE_TIMEOUT_MS,
            MIN_PROBE_TIMEOUT_MS,
        };

        assert_eq!(normalize_probe_timeout_ms(None), DEFAULT_PROBE_TIMEOUT_MS);
        assert_eq!(normalize_probe_timeout_ms(Some(500)), MIN_PROBE_TIMEOUT_MS);
        assert_eq!(
            normalize_probe_timeout_ms(Some(120_000)),
            MAX_PROBE_TIMEOUT_MS
        );
    }

    #[derive(Clone)]
    struct FailingSavePluginStore {
        deleted_packages: Arc<std::sync::Mutex<Vec<String>>>,
        record: PluginSettingsRecord,
        root: PathBuf,
    }

    impl FailingSavePluginStore {
        fn new(record: PluginSettingsRecord) -> Self {
            Self {
                deleted_packages: Arc::new(std::sync::Mutex::new(Vec::new())),
                record,
                root: std::env::temp_dir().join(format!("jyowo-plugin-store-{}", RunId::new())),
            }
        }

        fn deleted_packages(&self) -> Vec<String> {
            self.deleted_packages.lock().unwrap().clone()
        }
    }

    impl PluginStore for FailingSavePluginStore {
        fn package_root(&self) -> PathBuf {
            self.root.join("user")
        }

        fn cargo_extension_root(&self) -> PathBuf {
            self.root.join("extensions")
        }

        fn workspace_plugin_root(&self) -> PathBuf {
            self.root.join("workspace")
        }

        fn load_record(&self) -> Result<PluginSettingsRecord, CommandErrorPayload> {
            Ok(self.record.clone())
        }

        fn save_record(&self, _record: &PluginSettingsRecord) -> Result<(), CommandErrorPayload> {
            Err(runtime_operation_failed(
                "plugin index save failed".to_owned(),
            ))
        }

        fn write_plugin_package(
            &self,
            _package_dir: &str,
            _source_path: &Path,
        ) -> Result<String, CommandErrorPayload> {
            Ok("test-hash".to_owned())
        }

        fn delete_plugin_package(&self, package_dir: &str) -> Result<(), CommandErrorPayload> {
            self.deleted_packages
                .lock()
                .unwrap()
                .push(package_dir.to_owned());
            Ok(())
        }
    }
}
