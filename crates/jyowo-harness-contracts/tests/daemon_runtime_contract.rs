use chrono::{TimeZone, Utc};
use harness_contracts::{
    AutomationDeletedResponse, AutomationEnabledResponse, AutomationRunRecord,
    AutomationRunResponse, AutomationRunStatus, AutomationRunsResponse, AutomationSavedResponse,
    AutomationSchedule, AutomationSpec, AutomationWorkspaceScope, AutomationsResponse,
    ChildAttachment, ClientFrame, ClientRequest, DeleteMemoryItemResponse, GetMemoryItemResponse,
    ListMemoryItemsResponse, ListRuntimeToolsResponse, MemoryId, MissedRunPolicy, PermissionMode,
    RuntimeToolServiceBindingSummary, RuntimeToolSummary, SandboxMode, ServerFrame, ServerMessage,
    SubagentParentProjection, TaskId, ToolProfile, WorkspaceAccess, PROTOCOL_VERSION,
};
use serde_json::{json, Value};

fn workspace_root() -> Option<String> {
    Some("/tmp/project".to_owned())
}

fn automation() -> AutomationSpec {
    AutomationSpec {
        id: "automation-001".to_owned(),
        enabled: true,
        prompt: "Run checks".to_owned(),
        schedule: AutomationSchedule {
            interval_minutes: 60,
        },
        tool_profile: ToolProfile::Coding,
        permission_mode: PermissionMode::Default,
        sandbox_mode: SandboxMode::None,
        workspace_scope: AutomationWorkspaceScope::CurrentWorkspace,
        workspace_access: WorkspaceAccess::ReadOnly,
        missed_run_policy: MissedRunPolicy::RunOnce,
        created_at: Utc.with_ymd_and_hms(2026, 7, 12, 1, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 7, 12, 1, 0, 0).unwrap(),
    }
}

fn automation_run() -> AutomationRunRecord {
    AutomationRunRecord {
        automation_id: "automation-001".to_owned(),
        completed_at: None,
        id: "automation-run-001".to_owned(),
        message: Some("Started".to_owned()),
        run_id: Some("01J00000000000000000000000".to_owned()),
        started_at: Utc.with_ymd_and_hms(2026, 7, 12, 1, 5, 0).unwrap(),
        status: AutomationRunStatus::Started,
    }
}

fn request_value(request: ClientRequest) -> Value {
    serde_json::to_value(ClientFrame {
        request_id: "req-runtime".to_owned(),
        protocol_version: PROTOCOL_VERSION,
        request,
    })
    .expect("serialize client frame")
}

#[test]
fn runtime_and_memory_requests_use_workspace_scoped_camel_case_payloads() {
    let memory_id = MemoryId::new();
    let cases = [
        (
            ClientRequest::ListRuntimeTools {
                workspace_root: workspace_root(),
            },
            "list_runtime_tools",
            None,
        ),
        (
            ClientRequest::ListMemoryItems {
                workspace_root: workspace_root(),
            },
            "list_memory_items",
            None,
        ),
        (
            ClientRequest::GetMemoryItem {
                workspace_root: workspace_root(),
                memory_id,
            },
            "get_memory_item",
            Some(memory_id.to_string()),
        ),
        (
            ClientRequest::DeleteMemoryItem {
                workspace_root: workspace_root(),
                memory_id,
            },
            "delete_memory_item",
            Some(memory_id.to_string()),
        ),
    ];

    for (request, request_type, expected_memory_id) in cases {
        let value = request_value(request);
        assert_eq!(value["request"]["type"], request_type);
        assert_eq!(value["request"]["workspaceRoot"], "/tmp/project");
        assert!(value["request"].get("workspace_root").is_none());
        if let Some(expected_memory_id) = expected_memory_id {
            assert_eq!(value["request"]["memoryId"], expected_memory_id);
            assert!(value["request"].get("memory_id").is_none());
        }
    }
}

#[test]
fn automation_requests_reuse_the_shared_automation_contracts() {
    let cases = [
        request_value(ClientRequest::ListAutomations {
            workspace_root: workspace_root(),
        }),
        request_value(ClientRequest::SaveAutomation {
            workspace_root: workspace_root(),
            automation: automation(),
        }),
        request_value(ClientRequest::SetAutomationEnabled {
            workspace_root: workspace_root(),
            automation_id: "automation-001".to_owned(),
            enabled: false,
        }),
        request_value(ClientRequest::DeleteAutomation {
            workspace_root: workspace_root(),
            automation_id: "automation-001".to_owned(),
        }),
        request_value(ClientRequest::RunAutomationNow {
            workspace_root: workspace_root(),
            automation_id: "automation-001".to_owned(),
        }),
        request_value(ClientRequest::ListAutomationRuns {
            workspace_root: workspace_root(),
            automation_id: None,
        }),
    ];

    assert_eq!(cases[0]["request"]["type"], "list_automations");
    assert_eq!(cases[1]["request"]["automation"]["id"], "automation-001");
    assert_eq!(cases[2]["request"]["automationId"], "automation-001");
    assert_eq!(cases[2]["request"]["enabled"], false);
    assert_eq!(cases[3]["request"]["type"], "delete_automation");
    assert_eq!(cases[4]["request"]["type"], "run_automation_now");
    assert_eq!(cases[5]["request"]["type"], "list_automation_runs");
    assert_eq!(cases[5]["request"]["automationId"], Value::Null);
    assert!(cases
        .iter()
        .all(|value| value["request"]["workspaceRoot"] == "/tmp/project"));
}

#[test]
fn runtime_server_responses_are_typed_and_camel_case() {
    let memory_id = MemoryId::new();
    let messages = [
        ServerMessage::RuntimeTools(ListRuntimeToolsResponse {
            generation: 4,
            tools: vec![RuntimeToolSummary {
                name: "Read".to_owned(),
                display_name: "Read".to_owned(),
                description: "Read a file".to_owned(),
                category: "filesystem".to_owned(),
                group: "fileSystem".to_owned(),
                group_label: "File system".to_owned(),
                origin_kind: "builtin".to_owned(),
                origin_id: None,
                access: "readOnly".to_owned(),
                execution_channel: "directAuthorizedRust".to_owned(),
                required_capabilities: Vec::new(),
                defer_policy: "alwaysLoad".to_owned(),
                long_running: false,
                service_binding: Some(RuntimeToolServiceBindingSummary {
                    provider_id: "builtin".to_owned(),
                    operation_id: "read".to_owned(),
                    route_kind: "fileOperation".to_owned(),
                }),
            }],
        }),
        ServerMessage::MemoryItems(ListMemoryItemsResponse { items: Vec::new() }),
        ServerMessage::MemoryItem(GetMemoryItemResponse { item: None }),
        ServerMessage::MemoryDeleted(DeleteMemoryItemResponse { memory_id }),
        ServerMessage::Automations(AutomationsResponse {
            automations: vec![automation()],
        }),
        ServerMessage::AutomationSaved(AutomationSavedResponse {
            automation: automation(),
        }),
        ServerMessage::AutomationEnabled(AutomationEnabledResponse {
            automation: automation(),
        }),
        ServerMessage::AutomationDeleted(AutomationDeletedResponse {
            automation_id: "automation-001".to_owned(),
        }),
        ServerMessage::AutomationRun(AutomationRunResponse {
            run: automation_run(),
        }),
        ServerMessage::AutomationRuns(AutomationRunsResponse {
            runs: vec![automation_run()],
        }),
    ];

    let values = messages.map(|message| {
        serde_json::to_value(ServerFrame {
            request_id: Some("req-runtime".to_owned()),
            protocol_version: PROTOCOL_VERSION,
            message,
        })
        .expect("serialize server frame")
    });

    assert_eq!(values[0]["message"]["type"], "runtime_tools");
    assert_eq!(values[0]["message"]["generation"], 4);
    assert_eq!(values[0]["message"]["tools"][0]["displayName"], "Read");
    assert!(values[0]["message"]["tools"][0]
        .get("display_name")
        .is_none());
    assert_eq!(values[3]["message"]["memoryId"], memory_id.to_string());
    assert!(values[3]["message"].get("memory_id").is_none());
    assert_eq!(values[7]["message"]["automationId"], "automation-001");
    assert!(values[7]["message"].get("automation_id").is_none());
    assert_eq!(
        values[9]["message"]["runs"][0]["automationId"],
        "automation-001"
    );
}

#[test]
fn runtime_requests_reject_unknown_fields() {
    let frame = json!({
        "requestId": "req-runtime",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "list_runtime_tools",
            "workspaceRoot": "/tmp/project",
            "legacyRuntime": true
        }
    });

    assert!(serde_json::from_value::<ClientFrame>(frame).is_err());
}

#[test]
fn child_parent_projection_carries_explicit_attachment() {
    let projection = SubagentParentProjection {
        parent_task_id: TaskId::new(),
        parent_segment_id: harness_contracts::RunSegmentId::new(),
        delegation_id: harness_contracts::SubagentId::new(),
        attachment: ChildAttachment::Detached,
    };

    let value = serde_json::to_value(&projection).expect("serialize child parent projection");
    assert_eq!(projection.attachment, ChildAttachment::Detached);
    assert_eq!(value["attachment"], "detached");

    let missing_attachment = json!({
        "parentTaskId": TaskId::new(),
        "parentSegmentId": harness_contracts::RunSegmentId::new(),
        "delegationId": harness_contracts::SubagentId::new()
    });
    let legacy: SubagentParentProjection =
        serde_json::from_value(missing_attachment).expect("legacy parent projection parses");
    assert_eq!(legacy.attachment, ChildAttachment::Attached);
}
