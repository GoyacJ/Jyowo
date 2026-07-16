use chrono::{TimeZone, Utc};
use harness_contracts::{
    ChildAttachment, ClientFrame, ClientRequest, DaemonMemoryItem, DeleteMemoryItemResponse,
    GetMemoryItemResponse, ListMemoryItemsResponse, ListRuntimeToolsResponse, MemoryId,
    MissedRunPolicy, PermissionMode, RuntimeToolServiceBindingSummary, RuntimeToolSummary,
    ScheduledTaskDeletedResponse, ScheduledTaskEnabledResponse, ScheduledTaskRunRecord,
    ScheduledTaskRunResponse, ScheduledTaskRunStatus, ScheduledTaskRunsResponse,
    ScheduledTaskSavedResponse, ScheduledTaskSchedule, ScheduledTaskSpec, ScheduledTasksResponse,
    ServerFrame, ServerMessage, SubagentParentProjection, TaskId, PROTOCOL_VERSION,
};
use serde_json::{json, Value};

fn workspace_root() -> Option<String> {
    Some("/tmp/project".to_owned())
}

fn scheduled_task() -> ScheduledTaskSpec {
    ScheduledTaskSpec {
        id: "scheduled_task-001".to_owned(),
        name: "Checks".to_owned(),
        enabled: true,
        prompt: "Run checks".to_owned(),
        schedule: ScheduledTaskSchedule {
            interval_minutes: 60,
        },
        workspace_root: Some("/tmp/project".to_owned()),
        permission_mode: PermissionMode::Default,
        missed_run_policy: MissedRunPolicy::RunOnce,
        created_at: Utc.with_ymd_and_hms(2026, 7, 12, 1, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 7, 12, 1, 0, 0).unwrap(),
    }
}

fn scheduled_task_run() -> ScheduledTaskRunRecord {
    ScheduledTaskRunRecord {
        scheduled_task_id: "scheduled_task-001".to_owned(),
        completed_at: None,
        id: "scheduled_task-run-001".to_owned(),
        message: Some("Started".to_owned()),
        task_id: Some("01J00000000000000000000000".to_owned()),
        started_at: Utc.with_ymd_and_hms(2026, 7, 12, 1, 5, 0).unwrap(),
        status: ScheduledTaskRunStatus::Started,
    }
}

fn memory_item(memory_id: MemoryId) -> DaemonMemoryItem {
    let timestamp = Utc.with_ymd_and_hms(2026, 7, 12, 1, 0, 0).unwrap();
    DaemonMemoryItem {
        id: memory_id,
        provider_id: Some("local".to_owned()),
        kind: "semantic".to_owned(),
        visibility: "workspace".to_owned(),
        content: "Remember this".to_owned(),
        content_hash: "memory-hash".to_owned(),
        source: "user".to_owned(),
        tags: Vec::new(),
        confidence: 1.0,
        access_count: 0,
        last_accessed_at: None,
        expires_at: None,
        deleted: false,
        created_at: timestamp,
        updated_at: timestamp,
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
                action_plan_id: None,
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
fn scheduled_task_requests_reuse_the_shared_scheduled_task_contracts() {
    let cases = [
        request_value(ClientRequest::ListScheduledTasks),
        request_value(ClientRequest::SaveScheduledTask {
            scheduled_task: scheduled_task(),
        }),
        request_value(ClientRequest::SetScheduledTaskEnabled {
            scheduled_task_id: "scheduled_task-001".to_owned(),
            enabled: false,
        }),
        request_value(ClientRequest::DeleteScheduledTask {
            scheduled_task_id: "scheduled_task-001".to_owned(),
        }),
        request_value(ClientRequest::RunScheduledTaskNow {
            scheduled_task_id: "scheduled_task-001".to_owned(),
        }),
        request_value(ClientRequest::ListScheduledTaskRuns {
            scheduled_task_id: None,
        }),
    ];

    assert_eq!(cases[0]["request"]["type"], "list_scheduled_tasks");
    assert_eq!(
        cases[1]["request"]["scheduledTask"]["id"],
        "scheduled_task-001"
    );
    assert_eq!(cases[2]["request"]["scheduledTaskId"], "scheduled_task-001");
    assert_eq!(cases[2]["request"]["enabled"], false);
    assert_eq!(cases[3]["request"]["type"], "delete_scheduled_task");
    assert_eq!(cases[4]["request"]["type"], "run_scheduled_task_now");
    assert_eq!(cases[5]["request"]["type"], "list_scheduled_task_runs");
    assert_eq!(cases[5]["request"]["scheduled_taskId"], Value::Null);
    assert!(cases
        .iter()
        .all(|value| value["request"].get("workspaceRoot").is_none()));
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
        ServerMessage::MemoryItem(GetMemoryItemResponse {
            item: memory_item(memory_id),
        }),
        ServerMessage::MemoryDeleted(DeleteMemoryItemResponse { memory_id }),
        ServerMessage::ScheduledTasks(ScheduledTasksResponse {
            scheduled_tasks: vec![scheduled_task()],
        }),
        ServerMessage::ScheduledTaskSaved(ScheduledTaskSavedResponse {
            scheduled_task: scheduled_task(),
        }),
        ServerMessage::ScheduledTaskEnabled(ScheduledTaskEnabledResponse {
            scheduled_task: scheduled_task(),
        }),
        ServerMessage::ScheduledTaskDeleted(ScheduledTaskDeletedResponse {
            scheduled_task_id: "scheduled_task-001".to_owned(),
        }),
        ServerMessage::ScheduledTaskRun(ScheduledTaskRunResponse {
            run: scheduled_task_run(),
        }),
        ServerMessage::ScheduledTaskRuns(ScheduledTaskRunsResponse {
            runs: vec![scheduled_task_run()],
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
    assert_eq!(
        values[7]["message"]["scheduledTaskId"],
        "scheduled_task-001"
    );
    assert!(values[7]["message"].get("scheduled_task_id").is_none());
    assert_eq!(
        values[9]["message"]["runs"][0]["scheduledTaskId"],
        "scheduled_task-001"
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
