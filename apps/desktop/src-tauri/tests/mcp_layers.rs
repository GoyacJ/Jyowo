use harness_contracts::{
    now, CorrelationId, Event, EventId, EventSource, EventSourceKind, McpActivationFailedEvent,
    McpActivationFailureReason, McpConnectionLostEvent, McpConnectionLostReason, McpServerId,
    McpServerSource, RunId, RunSegmentId, SessionId, TaskEventEnvelope, TaskId, TenantId,
};
use jyowo_desktop_shell::commands::{
    delete_mcp_server_for_layer_with_runtime_state, ensure_mcp_config_layer_identity,
    get_mcp_server_config_for_layer_with_runtime_state,
    list_mcp_servers_for_layer_with_runtime_state, mcp_task_diagnostic_record_from_envelope,
    save_mcp_server_for_layer_with_runtime_state,
    set_mcp_server_enabled_for_layer_with_runtime_state, DeleteMcpServerRequest,
    DesktopMcpDiagnosticStore, DesktopRuntimeState, GetMcpServerConfigRequest, McpConfigLayer,
    McpDiagnosticPlane, McpDiagnosticRecord, McpDiagnosticSeverity, McpDiagnosticStore,
    McpNameValueSaveRecord, McpServerConfigRecord, McpServerStore, McpServerTransportConfig,
    SaveMcpServerRequest, SaveMcpServerTransportConfig, SetMcpServerEnabledRequest,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MemoryMcpServerStore {
    records: Mutex<Vec<McpServerConfigRecord>>,
}

impl McpServerStore for MemoryMcpServerStore {
    fn load_records(
        &self,
    ) -> Result<Vec<McpServerConfigRecord>, jyowo_desktop_shell::commands::CommandErrorPayload>
    {
        Ok(self.records.lock().unwrap().clone())
    }

    fn save_record(
        &self,
        record: &McpServerConfigRecord,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        let mut records = self.records.lock().unwrap();
        if let Some(existing) = records.iter_mut().find(|existing| existing.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        Ok(())
    }

    fn delete_record(
        &self,
        id: &str,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        self.records
            .lock()
            .unwrap()
            .retain(|record| record.id != id);
        Ok(())
    }
}

fn layered_mcp_request(id: &str, display_name: &str) -> SaveMcpServerRequest {
    SaveMcpServerRequest {
        enabled: false,
        required: false,
        display_name: display_name.to_owned(),
        id: id.to_owned(),
        scope: "global".to_owned(),
        transport: SaveMcpServerTransportConfig::Stdio {
            command: "node".to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            inherit_env: Vec::new(),
            working_dir: None,
        },
    }
}

fn stdio_request(
    id: &str,
    display_name: &str,
    value: Option<&str>,
    preserve_existing: bool,
) -> SaveMcpServerRequest {
    SaveMcpServerRequest {
        enabled: false,
        required: false,
        display_name: display_name.to_owned(),
        id: id.to_owned(),
        scope: "global".to_owned(),
        transport: SaveMcpServerTransportConfig::Stdio {
            command: "node".to_owned(),
            args: Vec::new(),
            env: vec![McpNameValueSaveRecord {
                key: "LOG_LEVEL".to_owned(),
                value: value.map(str::to_owned),
                preserve_existing,
            }],
            inherit_env: Vec::new(),
            working_dir: None,
        },
    }
}

fn http_request(
    id: &str,
    display_name: &str,
    value: Option<&str>,
    preserve_existing: bool,
) -> SaveMcpServerRequest {
    SaveMcpServerRequest {
        enabled: false,
        required: false,
        display_name: display_name.to_owned(),
        id: id.to_owned(),
        scope: "global".to_owned(),
        transport: SaveMcpServerTransportConfig::Http {
            url: "https://mcp.example.com/mcp".to_owned(),
            bearer_token_env_var: None,
            headers: vec![McpNameValueSaveRecord {
                key: "X-Client-Name".to_owned(),
                value: value.map(str::to_owned),
                preserve_existing,
            }],
            headers_from_env: Vec::new(),
        },
    }
}

#[tokio::test]
async fn project_mcp_override_is_isolated_and_delete_reveals_global() {
    let workspace = tempfile::tempdir().expect("workspace");
    let workspace_root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace_root)
        .expect("desktop runtime state");

    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Global,
        layered_mcp_request("shared", "Global server"),
        &state,
    )
    .await
    .expect("save global server");
    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Project,
        layered_mcp_request("shared", "Project server"),
        &state,
    )
    .await
    .expect("save project override");

    let global = list_mcp_servers_for_layer_with_runtime_state(McpConfigLayer::Global, &state)
        .await
        .expect("list global servers");
    assert_eq!(global.servers.len(), 1);
    assert_eq!(global.servers[0].config_layer, McpConfigLayer::Global);
    assert!(!global.servers[0].effective);
    assert!(global.servers[0].manageable);

    let project = list_mcp_servers_for_layer_with_runtime_state(McpConfigLayer::Project, &state)
        .await
        .expect("list project servers");
    assert_eq!(project.servers.len(), 1);
    assert_eq!(project.servers[0].display_name, "Project server");
    assert_eq!(project.servers[0].config_layer, McpConfigLayer::Project);
    assert!(project.servers[0].effective);
    assert!(project.servers[0].overrides_global);
    assert_eq!(project.servers[0].origin, "project");
    assert_eq!(project.servers[0].status_source, "settings");

    delete_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Project,
        DeleteMcpServerRequest {
            id: "shared".to_owned(),
        },
        &state,
    )
    .await
    .expect("delete project override");

    let revealed = list_mcp_servers_for_layer_with_runtime_state(McpConfigLayer::Project, &state)
        .await
        .expect("list revealed global server");
    assert_eq!(revealed.servers.len(), 1);
    assert_eq!(revealed.servers[0].display_name, "Global server");
    assert_eq!(revealed.servers[0].config_layer, McpConfigLayer::Global);
    assert!(revealed.servers[0].effective);
    assert!(!revealed.servers[0].manageable);
    assert!(!revealed.servers[0].overrides_global);
}

#[tokio::test]
async fn project_mcp_layer_requires_an_active_project() {
    let workspace = tempfile::tempdir().expect("workspace");
    let workspace_root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace_root)
        .expect("desktop runtime state");
    state.set_project_mcp_server_store_for_test(None);

    let error = list_mcp_servers_for_layer_with_runtime_state(McpConfigLayer::Project, &state)
        .await
        .expect_err("project layer requires an active project");
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn inherited_global_mcp_record_is_read_only_in_project_layer() {
    let workspace = tempfile::tempdir().expect("workspace");
    let workspace_root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace_root)
        .expect("desktop runtime state");
    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Global,
        layered_mcp_request("inherited", "Inherited server"),
        &state,
    )
    .await
    .expect("save global server");

    let config = get_mcp_server_config_for_layer_with_runtime_state(
        McpConfigLayer::Project,
        GetMcpServerConfigRequest {
            id: "inherited".to_owned(),
        },
        &state,
    )
    .await
    .expect("read inherited config");
    assert_eq!(config.server.config_layer, McpConfigLayer::Global);
    assert!(!config.server.manageable);

    let error = set_mcp_server_enabled_for_layer_with_runtime_state(
        McpConfigLayer::Project,
        SetMcpServerEnabledRequest {
            id: "inherited".to_owned(),
            enabled: true,
        },
        &state,
    )
    .await
    .expect_err("inherited config must be read-only");
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn inherited_global_hidden_values_are_preserved_when_creating_project_overrides() {
    let workspace = tempfile::tempdir().expect("workspace");
    let workspace_root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace_root)
        .expect("desktop runtime state");
    let global_store = Arc::new(MemoryMcpServerStore::default());
    let project_store = Arc::new(MemoryMcpServerStore::default());
    state.set_mcp_server_store_for_test(global_store.clone());
    state.set_project_mcp_server_store_for_test(Some(project_store.clone()));

    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Global,
        stdio_request("inherited-stdio", "Global stdio", Some("verbose"), false),
        &state,
    )
    .await
    .expect("save global stdio server");
    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Global,
        http_request(
            "inherited-http",
            "Global HTTP",
            Some("jyowo-desktop"),
            false,
        ),
        &state,
    )
    .await
    .expect("save global HTTP server");

    for (id, hidden_value) in [
        ("inherited-stdio", "verbose"),
        ("inherited-http", "jyowo-desktop"),
    ] {
        let config = get_mcp_server_config_for_layer_with_runtime_state(
            McpConfigLayer::Project,
            GetMcpServerConfigRequest { id: id.to_owned() },
            &state,
        )
        .await
        .expect("read inherited config");
        let serialized = serde_json::to_string(&config).expect("serialize inherited config");
        assert_eq!(config.server.config_layer, McpConfigLayer::Global);
        assert!(!serialized.contains(hidden_value));
        match config.server.transport {
            jyowo_desktop_shell::commands::McpServerConfigTransportPayload::Stdio {
                env, ..
            } => {
                assert!(env[0].has_value);
                assert!(env[0].value.is_none());
            }
            jyowo_desktop_shell::commands::McpServerConfigTransportPayload::Http {
                headers,
                ..
            } => {
                assert!(headers[0].has_value);
                assert!(headers[0].value.is_none());
            }
            _ => panic!("unexpected inherited transport"),
        }
    }
    let global_before_override = global_store.records.lock().unwrap().clone();

    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Project,
        stdio_request("inherited-stdio", "Project stdio", None, true),
        &state,
    )
    .await
    .expect("preserve inherited stdio env in project override");
    save_mcp_server_for_layer_with_runtime_state(
        McpConfigLayer::Project,
        http_request("inherited-http", "Project HTTP", None, true),
        &state,
    )
    .await
    .expect("preserve inherited HTTP header in project override");

    let project_records = project_store.records.lock().unwrap().clone();
    let project_stdio = project_records
        .iter()
        .find(|record| record.id == "inherited-stdio")
        .expect("project stdio record");
    let McpServerTransportConfig::Stdio { env, .. } = &project_stdio.transport else {
        panic!("expected project stdio transport");
    };
    assert_eq!(env[0].value, "verbose");
    let project_http = project_records
        .iter()
        .find(|record| record.id == "inherited-http")
        .expect("project HTTP record");
    let McpServerTransportConfig::Http { headers, .. } = &project_http.transport else {
        panic!("expected project HTTP transport");
    };
    assert_eq!(headers[0].value, "jyowo-desktop");

    assert_eq!(
        *global_store.records.lock().unwrap(),
        global_before_override
    );

    for (id, hidden_value) in [
        ("inherited-stdio", "verbose"),
        ("inherited-http", "jyowo-desktop"),
    ] {
        delete_mcp_server_for_layer_with_runtime_state(
            McpConfigLayer::Project,
            DeleteMcpServerRequest { id: id.to_owned() },
            &state,
        )
        .await
        .expect("delete project override");
        let revealed = get_mcp_server_config_for_layer_with_runtime_state(
            McpConfigLayer::Project,
            GetMcpServerConfigRequest { id: id.to_owned() },
            &state,
        )
        .await
        .expect("read revealed global config");
        assert_eq!(revealed.server.config_layer, McpConfigLayer::Global);
        assert!(!serde_json::to_string(&revealed)
            .expect("serialize revealed global config")
            .contains(hidden_value));
    }
}

#[test]
fn mcp_diagnostic_plane_defaults_to_settings_and_preserves_task_context() {
    let legacy = serde_json::from_value::<McpDiagnosticRecord>(json!({
        "eventType": "connection_lost",
        "id": "legacy",
        "serverId": "github",
        "severity": "warning",
        "summary": "legacy diagnostic",
        "timestamp": "2026-07-13T00:00:00Z"
    }))
    .expect("legacy diagnostic");
    assert_eq!(legacy.plane, McpDiagnosticPlane::Settings);
    assert!(legacy.task_id.is_none());

    let task = serde_json::from_value::<McpDiagnosticRecord>(json!({
        "eventType": "activation_failed",
        "id": "task-event",
        "serverId": "github",
        "severity": "error",
        "summary": "activation failed",
        "timestamp": "2026-07-13T00:00:01Z",
        "plane": "task",
        "taskId": "task-1",
        "sessionId": "session-1",
        "runId": "run-1",
        "runSegmentId": "segment-1"
    }))
    .expect("task diagnostic");
    assert_eq!(task.plane, McpDiagnosticPlane::Task);
    assert_eq!(task.task_id.as_deref(), Some("task-1"));
    assert_eq!(task.session_id.as_deref(), Some("session-1"));
    assert_eq!(task.run_id.as_deref(), Some("run-1"));
    assert_eq!(task.run_segment_id.as_deref(), Some("segment-1"));
}

#[test]
fn task_journal_mcp_event_converts_with_envelope_identity_and_runtime_context() {
    let task_id = TaskId::new();
    let event_id = EventId::new();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let run_segment_id = RunSegmentId::new();
    let at = now();
    let envelope = TaskEventEnvelope {
        global_offset: 42,
        task_id,
        stream_sequence: 7,
        event_id,
        event_type: "engine.mcp_connection_lost".to_owned(),
        schema_version: 1,
        recorded_at: at,
        source: EventSource {
            kind: EventSourceKind::Engine,
            actor_id: None,
            client_id: None,
        },
        payload: json!({
            "tenantId": TenantId::SINGLE,
            "sessionId": session_id,
            "journalOffset": 41,
            "runId": run_id,
            "runSegmentId": run_segment_id,
            "correlationId": CorrelationId::new(),
            "causationId": null,
            "event": Event::McpConnectionLost(McpConnectionLostEvent {
                session_id: Some(session_id),
                server_id: McpServerId("github".to_owned()),
                server_source: McpServerSource::Project,
                reason: McpConnectionLostReason::Other("closed".to_owned()),
                attempts_so_far: 1,
                terminal: false,
                at,
            }),
        }),
    };

    let diagnostic =
        mcp_task_diagnostic_record_from_envelope(envelope).expect("task MCP diagnostic");
    assert_eq!(diagnostic.id, event_id.to_string());
    assert_eq!(diagnostic.server_id, "github");
    assert_eq!(diagnostic.plane, McpDiagnosticPlane::Task);
    assert_eq!(diagnostic.task_id, Some(task_id.to_string()));
    assert_eq!(diagnostic.session_id, Some(session_id.to_string()));
    assert_eq!(diagnostic.run_id, Some(run_id.to_string()));
    assert_eq!(diagnostic.run_segment_id, Some(run_segment_id.to_string()));
}

#[test]
fn task_journal_activation_failure_converts_to_desktop_diagnostic() {
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let run_segment_id = RunSegmentId::new();
    let at = now();
    let envelope = TaskEventEnvelope {
        global_offset: 43,
        task_id,
        stream_sequence: 8,
        event_id: EventId::new(),
        event_type: "engine.mcp_activation_failed".to_owned(),
        schema_version: 1,
        recorded_at: at,
        source: EventSource {
            kind: EventSourceKind::Engine,
            actor_id: None,
            client_id: None,
        },
        payload: json!({
            "tenantId": TenantId::SINGLE,
            "sessionId": session_id,
            "journalOffset": 42,
            "runId": run_id,
            "runSegmentId": run_segment_id,
            "correlationId": CorrelationId::new(),
            "causationId": null,
            "event": Event::McpActivationFailed(McpActivationFailedEvent {
                session_id: Some(session_id),
                run_id: Some(run_id),
                server_id: McpServerId("github".to_owned()),
                server_source: McpServerSource::Project,
                required: false,
                reason: McpActivationFailureReason::CredentialUnavailable,
                at,
            }),
        }),
    };

    let diagnostic =
        mcp_task_diagnostic_record_from_envelope(envelope).expect("activation diagnostic");
    assert_eq!(diagnostic.event_type, "activation_failed");
    assert_eq!(diagnostic.server_id, "github");
    assert_eq!(diagnostic.summary, "MCP server credential is unavailable.");
    assert_eq!(diagnostic.plane, McpDiagnosticPlane::Task);
    assert_eq!(diagnostic.task_id, Some(task_id.to_string()));
    assert_eq!(diagnostic.run_id, Some(run_id.to_string()));
    assert_eq!(diagnostic.run_segment_id, Some(run_segment_id.to_string()));
}

#[test]
fn diagnostic_clear_without_daemon_persists_task_watermark() {
    let temp = tempfile::tempdir().expect("tempdir");
    let runtime_root = temp
        .path()
        .canonicalize()
        .expect("canonical tempdir")
        .join("runtime");
    let store = DesktopMcpDiagnosticStore::new_runtime_root(runtime_root.clone());
    store
        .append_record(&McpDiagnosticRecord {
            event_type: "connected".to_owned(),
            id: "diagnostic-1".to_owned(),
            server_id: "server-a".to_owned(),
            severity: McpDiagnosticSeverity::Info,
            summary: "connected".to_owned(),
            timestamp: "2026-07-13T00:00:00Z".to_owned(),
            plane: McpDiagnosticPlane::Settings,
            task_id: None,
            session_id: None,
            run_id: None,
            run_segment_id: None,
        })
        .expect("append settings diagnostic");
    let cleared_at = now();

    store
        .clear_records(None, cleared_at)
        .expect("clear without daemon");

    assert!(store
        .load_records()
        .expect("load settings records")
        .is_empty());
    drop(store);
    let reopened = DesktopMcpDiagnosticStore::new_runtime_root(runtime_root);
    let watermarks = reopened
        .load_task_clear_watermarks()
        .expect("load persisted task watermark");
    assert_eq!(watermarks.all, Some(cleared_at));
    assert!(watermarks.servers.is_empty());
}

#[test]
fn task_journal_non_mcp_engine_event_is_not_a_diagnostic() {
    let envelope = TaskEventEnvelope {
        global_offset: 1,
        task_id: TaskId::new(),
        stream_sequence: 1,
        event_id: EventId::new(),
        event_type: "engine.run_started".to_owned(),
        schema_version: 1,
        recorded_at: now(),
        source: EventSource {
            kind: EventSourceKind::Engine,
            actor_id: None,
            client_id: None,
        },
        payload: json!({}),
    };

    assert!(mcp_task_diagnostic_record_from_envelope(envelope).is_none());
}

#[test]
fn project_mcp_mutations_reject_a_stale_project_identity() {
    let active = tempfile::tempdir().expect("active project");
    let stale = tempfile::tempdir().expect("stale project");
    let state = DesktopRuntimeState::with_workspace_for_test(active.path().to_path_buf())
        .expect("runtime state");

    ensure_mcp_config_layer_identity(
        McpConfigLayer::Project,
        Some(active.path().to_str().expect("active path")),
        &state,
    )
    .expect("active identity");
    let error = ensure_mcp_config_layer_identity(
        McpConfigLayer::Project,
        Some(stale.path().to_str().expect("stale path")),
        &state,
    )
    .expect_err("stale identity must fail");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("project identity"));

    let missing = ensure_mcp_config_layer_identity(McpConfigLayer::Project, None, &state)
        .expect_err("project mutation without identity must fail");
    assert_eq!(missing.code, "INVALID_PAYLOAD");

    let unexpected = ensure_mcp_config_layer_identity(
        McpConfigLayer::Global,
        Some(active.path().to_str().expect("active path")),
        &state,
    )
    .expect_err("global mutation with project identity must fail");
    assert_eq!(unexpected.code, "INVALID_PAYLOAD");
}
