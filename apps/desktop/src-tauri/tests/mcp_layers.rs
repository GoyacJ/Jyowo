use jyowo_desktop_shell::commands::{
    delete_mcp_server_for_layer_with_runtime_state,
    get_mcp_server_config_for_layer_with_runtime_state,
    list_mcp_servers_for_layer_with_runtime_state, save_mcp_server_for_layer_with_runtime_state,
    set_mcp_server_enabled_for_layer_with_runtime_state, DeleteMcpServerRequest,
    DesktopRuntimeState, GetMcpServerConfigRequest, McpConfigLayer, McpDiagnosticPlane,
    McpDiagnosticRecord, SaveMcpServerRequest, SaveMcpServerTransportConfig,
    SetMcpServerEnabledRequest,
};
use serde_json::json;

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
