use super::*;

#[tokio::test]
async fn memory_commands_list_inspect_update_delete_and_export_visible_items() {
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let state = runtime_state_with_memory_provider(Arc::new(RawExportMemoryProvider::new(
        provider.clone(),
    )))
    .await;
    let session_id = state.default_conversation_id();
    let visible = test_memory_record(session_id, "Prefers concise Chinese responses");
    provider.upsert(visible.clone()).await.unwrap();
    provider
        .upsert(test_memory_record(
            SessionId::new(),
            "Hidden session memory",
        ))
        .await
        .unwrap();

    let list = list_memory_items_with_runtime_state(&state).await.unwrap();

    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].id, visible.id.to_string());
    assert_eq!(list.items[0].visibility, "private");
    assert_eq!(list.items[0].kind, "user_preference");

    let detail = get_memory_item_with_runtime_state(
        GetMemoryItemRequest {
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(detail.item.content, "Prefers concise Chinese responses");

    let updated = update_memory_item_with_runtime_state(
        UpdateMemoryItemRequest {
            action_plan_id: Some(harness_contracts::ActionPlanId::new().to_string()),
            content: "Prefers terse Chinese responses".to_owned(),
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(updated.item.content, "Prefers terse Chinese responses");

    let exported = export_memory_items_with_runtime_state(
        ExportMemoryItemsRequest {
            session_id: None,
            scope: ExportMemoryItemsScope::Visible,
            format: ExportMemoryItemsFormat::Json,
            include_raw_content: false,
            include_metadata: true,
            include_hashes: true,
            explicit_user_action: true,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(exported.format, "json");
    assert_eq!(exported.item_count, 1);
    assert!(exported.path.starts_with(".jyowo/runtime/exports/memory-"));
    let export_content = std::fs::read_to_string(state.workspace_root().join(&exported.path))
        .expect("memory export file should be readable");
    assert!(export_content.contains("contentPreview"));
    assert!(export_content.contains("[redacted memory content]"));
    assert!(export_content.contains("contentHash"));
    assert!(export_content.contains("source"));
    assert!(export_content.contains("tags"));
    assert!(!export_content.contains("\"content\""));
    assert!(!export_content.contains("Prefers terse Chinese responses"));
    let exported_items: serde_json::Value =
        serde_json::from_str(&export_content).expect("memory export should be valid json");
    let expected_content_hash = blake3::hash("Prefers terse Chinese responses".as_bytes())
        .to_hex()
        .to_string();
    assert_eq!(
        exported_items
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("contentHash"))
            .and_then(serde_json::Value::as_str),
        Some(expected_content_hash.as_str())
    );
    let raw_export = export_memory_items_with_runtime_state(
        ExportMemoryItemsRequest {
            session_id: None,
            scope: ExportMemoryItemsScope::Visible,
            format: ExportMemoryItemsFormat::Json,
            include_raw_content: true,
            include_metadata: true,
            include_hashes: true,
            explicit_user_action: true,
        },
        &state,
    )
    .await
    .unwrap();
    assert!(raw_export.include_raw_content);
    let raw_export_content = std::fs::read_to_string(state.workspace_root().join(&raw_export.path))
        .expect("raw memory export file should be readable");
    assert!(raw_export_content.contains("\"content\""));
    assert!(raw_export_content.contains("Prefers terse Chinese responses"));
    let raw_exported_items: serde_json::Value =
        serde_json::from_str(&raw_export_content).expect("raw memory export should be valid json");
    assert_eq!(
        raw_exported_items
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("contentHash"))
            .and_then(serde_json::Value::as_str),
        Some(expected_content_hash.as_str())
    );

    let metadata_free_export = export_memory_items_with_runtime_state(
        ExportMemoryItemsRequest {
            session_id: None,
            scope: ExportMemoryItemsScope::Visible,
            format: ExportMemoryItemsFormat::Json,
            include_raw_content: false,
            include_metadata: false,
            include_hashes: false,
            explicit_user_action: true,
        },
        &state,
    )
    .await
    .unwrap();
    let metadata_free_content =
        std::fs::read_to_string(state.workspace_root().join(&metadata_free_export.path))
            .expect("metadata-free memory export file should be readable");
    assert!(metadata_free_content.contains("contentPreview"));
    assert!(!metadata_free_content.contains("contentHash"));
    assert!(!metadata_free_content.contains("source"));
    assert!(!metadata_free_content.contains("tags"));

    let denied_export = export_memory_items_with_runtime_state(
        ExportMemoryItemsRequest {
            session_id: None,
            scope: ExportMemoryItemsScope::Visible,
            format: ExportMemoryItemsFormat::Json,
            include_raw_content: false,
            include_metadata: true,
            include_hashes: true,
            explicit_user_action: false,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(denied_export.code, "INVALID_PAYLOAD");

    let denied_raw_export = export_memory_items_with_runtime_state(
        ExportMemoryItemsRequest {
            session_id: None,
            scope: ExportMemoryItemsScope::Visible,
            format: ExportMemoryItemsFormat::Json,
            include_raw_content: true,
            include_metadata: true,
            include_hashes: true,
            explicit_user_action: false,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(denied_raw_export.code, "INVALID_PAYLOAD");

    let deleted = delete_memory_item_with_runtime_state(
        DeleteMemoryItemRequest {
            action_plan_id: Some(harness_contracts::ActionPlanId::new().to_string()),
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(deleted.status, "deleted");

    let list_after_delete = list_memory_items_with_runtime_state(&state).await.unwrap();
    assert!(list_after_delete.items.is_empty());
}

#[tokio::test]
async fn no_workspace_memory_export_uses_global_runtime_exports() {
    let _lock = HOME_ENV_LOCK.lock().unwrap();
    let home = unique_workspace("no-workspace-memory-home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let _home_guard = EnvVarGuard::set(HOME_ENV, home.as_os_str());
    let session_id = SessionId::new();
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let runtime_root = home
        .join(".jyowo")
        .join("runtime")
        .join("global-conversations");
    let conversation_cwd = runtime_root.join("workdir").join(session_id.to_string());
    std::fs::create_dir_all(&conversation_cwd).unwrap();
    let settings_runtime: Arc<DesktopSettingsRuntime> = Arc::new(
        DesktopSettingsRuntime::builder()
            .with_options(test_settings_options(&conversation_cwd))
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_memory_provider_arc(Arc::new(RawExportMemoryProvider::new(provider.clone())))
            .build()
            .await
            .expect("settings runtime should build with memory provider")
            .into(),
    );
    let state = DesktopRuntimeState::with_settings_runtime_for_global_conversation(
        runtime_root,
        session_id,
        settings_runtime,
    )
    .expect("state should use the settings runtime");
    provider
        .upsert(test_memory_record(session_id, "Global conversation memory"))
        .await
        .unwrap();

    let exported = export_memory_items_with_runtime_state(
        ExportMemoryItemsRequest {
            session_id: Some(session_id),
            scope: ExportMemoryItemsScope::Visible,
            format: ExportMemoryItemsFormat::Json,
            include_raw_content: false,
            include_metadata: true,
            include_hashes: true,
            explicit_user_action: true,
        },
        &state,
    )
    .await
    .expect("memory export should succeed");

    assert!(exported.path.starts_with(&format!("exports/{session_id}/")));
    assert!(state.runtime_root().join(&exported.path).is_file());
    assert!(!state
        .conversation_cwd()
        .join(".jyowo")
        .join("runtime")
        .join("exports")
        .exists());
}
