use harness_contracts::{
    daemon_protocol_schema, AgentCapabilities, ClientFrame, ClientRequest, HandshakeResponse,
    ServerFrame, TaskProjection, TimelineItemProjection, WorkspaceMode, PROTOCOL_VERSION,
};
use serde_json::json;

#[test]
fn daemon_protocol_exports_one_versioned_schema() {
    assert_eq!(PROTOCOL_VERSION, 3);

    let value = serde_json::to_value(daemon_protocol_schema()).expect("serialize daemon schema");
    let text = serde_json::to_string(&value).expect("render daemon schema");
    for required in [
        "handshake",
        "submit_message",
        "edit_queued_message",
        "delete_queued_message",
        "promote_queued_message",
        "rename_task",
        "set_task_pinned",
        "set_task_archived",
        "remove_task",
        "resolve_permission",
        "subscribe_events",
        "load_events",
        "load_task_events",
        "event_history_page",
        "read_blob",
        "list_runtime_tools",
        "list_memory_items",
        "get_memory_item",
        "delete_memory_item",
        "get_model_request_preview",
        "list_automations",
        "save_automation",
        "set_automation_enabled",
        "delete_automation",
        "run_automation_now",
        "list_automation_runs",
        "detached",
    ] {
        assert!(text.contains(required), "missing {required}");
    }

    let frame = ClientFrame {
        request_id: "req-1".into(),
        protocol_version: PROTOCOL_VERSION,
        request: ClientRequest::SubscribeEvents { after_offset: 42 },
    };
    let value = serde_json::to_value(frame).expect("serialize client frame");
    assert_eq!(value["request"]["type"], "subscribe_events");
    assert_eq!(value["request"]["afterOffset"], 42);
    assert!(value["request"].get("after_offset").is_none());
}

#[test]
fn handshake_publishes_executable_agent_capabilities() {
    let response = HandshakeResponse {
        daemon_version: "0.1.0".into(),
        user_instance_id: "user-a".into(),
        latest_global_offset: 7,
        agent_capabilities: AgentCapabilities {
            subagents: true,
            agent_teams: true,
            background_agents: true,
        },
    };

    let value = serde_json::to_value(response).expect("serialize handshake");
    assert_eq!(value["agentCapabilities"]["subagents"], true);
    assert_eq!(value["agentCapabilities"]["agentTeams"], true);
    assert_eq!(value["agentCapabilities"]["backgroundAgents"], true);

    let schema = serde_json::to_value(daemon_protocol_schema()).expect("serialize schema");
    let properties = &schema["$defs"]["AgentCapabilities"]["properties"];
    assert!(properties.get("subagents").is_some());
    assert!(properties.get("agentTeams").is_some());
    assert!(properties.get("backgroundAgents").is_some());
}

#[test]
fn task_audit_events_use_a_task_scoped_backward_cursor() {
    let frame = json!({
        "requestId": "req-audit",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "load_task_events",
            "taskId": "00000000000000000000000002",
            "beforeGlobalOffset": 42,
            "limit": 16
        }
    });

    let parsed = serde_json::from_value::<ClientFrame>(frame).expect("audit page request parses");
    let encoded = serde_json::to_value(parsed).expect("audit page request serializes");

    assert_eq!(encoded["request"]["type"], "load_task_events");
    assert_eq!(encoded["request"]["beforeGlobalOffset"], 42);
    assert_eq!(encoded["request"]["limit"], 16);
}

#[test]
fn timeline_items_preserve_optional_semantic_group_identity() {
    let value = json!({
        "id": "00000000000000000000000001",
        "kind": "assistant_text",
        "globalOffset": 7,
        "runSegmentId": "00000000000000000000000002",
        "semanticGroupId": "00000000000000000000000003",
        "summary": "streamed answer",
        "blobId": null,
        "incomplete": true
    });

    let item: TimelineItemProjection =
        serde_json::from_value(value.clone()).expect("semantic timeline item parses");

    assert_eq!(serde_json::to_value(item).unwrap(), value);
}

#[test]
fn task_metadata_commands_use_task_scoped_camel_case_contracts() {
    for (request_type, extra) in [
        ("rename_task", json!({ "title": "Renamed" })),
        ("set_task_pinned", json!({ "pinned": true })),
        ("set_task_archived", json!({ "archived": true })),
        ("remove_task", json!({})),
    ] {
        let mut request = json!({
            "type": request_type,
            "metadata": {
                "commandId": "00000000000000000000000001",
                "idempotencyKey": format!("{request_type}-1"),
                "expectedStreamVersion": 4
            },
            "taskId": "00000000000000000000000002"
        });
        request
            .as_object_mut()
            .expect("request object")
            .extend(extra.as_object().expect("extra object").clone());
        let frame = json!({
            "requestId": "req-1",
            "protocolVersion": PROTOCOL_VERSION,
            "request": request
        });

        let parsed = serde_json::from_value::<ClientFrame>(frame).expect("metadata command parses");
        let encoded = serde_json::to_value(parsed).expect("metadata command serializes");

        assert_eq!(encoded["request"]["type"], request_type);
        assert_eq!(encoded["request"]["expectedStreamVersion"], json!(null));
        assert_eq!(encoded["request"]["metadata"]["expectedStreamVersion"], 4);
    }
}

#[test]
fn daemon_request_ids_are_bounded_printable_ascii() {
    let value = serde_json::to_value(daemon_protocol_schema()).expect("serialize daemon schema");
    let request_id = &value["$defs"]["ClientFrame"]["properties"]["requestId"];

    assert_eq!(request_id["minLength"], 1);
    assert_eq!(request_id["maxLength"], 128);
    assert_eq!(request_id["pattern"], r"^[\x20-\x7E]+$");
}

#[test]
fn daemon_protocol_exports_permission_routing() {
    let value = serde_json::to_value(daemon_protocol_schema()).expect("serialize daemon schema");
    let permission_route = &value["$defs"]["PermissionRoute"];

    assert_eq!(
        permission_route["enum"],
        json!(["foreground_task", "saved_policy"])
    );
}

#[test]
fn client_frames_reject_unknown_fields() {
    let frame = json!({
        "requestId": "req-1",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "read_blob",
            "blobId": "00000000000000000000000001",
            "blobPath": "/tmp/secret"
        }
    });

    assert!(serde_json::from_value::<ClientFrame>(frame).is_err());
}

#[test]
fn client_command_payloads_reject_unknown_fields() {
    let frame = json!({
        "requestId": "req-1",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "submit_message",
            "metadata": {
                "commandId": "00000000000000000000000001",
                "idempotencyKey": "submit-1",
                "expectedStreamVersion": 0
            },
            "taskId": "00000000000000000000000002",
            "content": "hello",
            "attachments": [],
            "contextReferences": [],
            "blobPath": "/tmp/secret"
        }
    });

    assert!(serde_json::from_value::<ClientFrame>(frame).is_err());
}

#[test]
fn submit_message_carries_runtime_choices() {
    let frame = json!({
        "requestId": "req-1",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "submit_message",
            "metadata": {
                "commandId": "00000000000000000000000001",
                "idempotencyKey": "submit-1",
                "expectedStreamVersion": 0
            },
            "taskId": "00000000000000000000000002",
            "content": "hello",
            "attachments": [],
            "contextReferences": [],
            "modelConfigId": "provider-config-001",
            "permissionMode": "auto"
        }
    });

    let parsed = serde_json::from_value::<ClientFrame>(frame).expect("runtime choices parse");
    let encoded = serde_json::to_value(parsed).expect("runtime choices serialize");
    assert_eq!(encoded["request"]["modelConfigId"], "provider-config-001");
    assert_eq!(encoded["request"]["permissionMode"], "auto");
}

#[test]
fn task_projection_persists_its_workspace_selection() {
    let projection = serde_json::from_value::<TaskProjection>(json!({
        "taskId": "00000000000000000000000002",
        "title": "workspace task",
        "state": "idle",
        "archived": false,
        "streamVersion": 1,
        "lastGlobalOffset": 1,
        "currentRun": null,
        "pendingPermission": null,
        "queue": [],
        "workspace": { "mode": "current", "root": "/tmp/project" }
    }))
    .expect("task projection with workspace");

    let workspace = projection.workspace.expect("workspace persisted");
    assert!(!projection.pinned);
    assert!(!projection.removed);
    assert_eq!(workspace.mode, WorkspaceMode::Current);
    assert_eq!(workspace.root, "/tmp/project");
}

#[test]
fn task_projection_serializes_durable_sidebar_metadata() {
    let projection = serde_json::from_value::<TaskProjection>(json!({
        "taskId": "00000000000000000000000002",
        "title": "sidebar task",
        "state": "idle",
        "pinned": true,
        "archived": true,
        "removed": true,
        "streamVersion": 4,
        "lastGlobalOffset": 4,
        "currentRun": null,
        "pendingPermission": null,
        "queue": []
    }))
    .expect("task projection with sidebar metadata");

    let encoded = serde_json::to_value(projection).expect("serialize task projection");
    assert_eq!(encoded["pinned"], true);
    assert_eq!(encoded["archived"], true);
    assert_eq!(encoded["removed"], true);
}

#[test]
fn blob_payload_exposes_its_content_hash_without_a_path() {
    let schema = serde_json::to_value(schemars::schema_for!(harness_contracts::BlobPayload))
        .expect("serialize blob schema");
    let properties = &schema["properties"];

    assert!(properties.get("contentHash").is_some());
    assert!(properties.get("path").is_none());
}

#[test]
fn client_frames_reject_invalid_ulids() {
    let frame = json!({
        "requestId": "req-1",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "read_blob",
            "blobId": "/tmp/secret"
        }
    });

    assert!(serde_json::from_value::<ClientFrame>(frame).is_err());
}

#[test]
fn client_frames_reject_non_canonical_ulid_overflow() {
    let frame = json!({
        "requestId": "req-1",
        "protocolVersion": PROTOCOL_VERSION,
        "request": {
            "type": "read_blob",
            "blobId": "80000000000000000000000000"
        }
    });

    assert!(serde_json::from_value::<ClientFrame>(frame).is_err());
}

#[test]
fn server_frames_reject_invalid_rfc3339_timestamps() {
    let frame = json!({
        "requestId": null,
        "protocolVersion": PROTOCOL_VERSION,
        "message": {
            "type": "event_batch",
            "afterOffset": 40,
            "latestOffset": 41,
            "gap": false,
            "events": [{
                "globalOffset": 41,
                "taskId": "00000000000000000000000001",
                "streamSequence": 1,
                "eventId": "00000000000000000000000002",
                "eventType": "assistant.text",
                "schemaVersion": 1,
                "recordedAt": "not-a-timestamp",
                "source": {
                    "kind": "assistant",
                    "actorId": null,
                    "clientId": null
                },
                "payload": {}
            }]
        }
    });

    assert!(serde_json::from_value::<ServerFrame>(frame).is_err());
}

#[test]
fn server_frames_reject_non_standard_rfc3339_separators() {
    for recorded_at in [
        "2015-02-18 12:00:00Z",
        "2015-02-18T12:00:00+0500",
        "2015-02-18T12:00:00+05",
    ] {
        let frame = event_batch_frame(recorded_at);
        assert!(
            serde_json::from_value::<ServerFrame>(frame).is_err(),
            "accepted {recorded_at}"
        );
    }
}

#[test]
fn server_frames_accept_rfc3339_offsets_with_colons() {
    let frame = event_batch_frame("2015-02-18T12:00:00+05:00");

    assert!(serde_json::from_value::<ServerFrame>(frame).is_ok());
}

fn event_batch_frame(recorded_at: &str) -> serde_json::Value {
    json!({
        "requestId": null,
        "protocolVersion": PROTOCOL_VERSION,
        "message": {
            "type": "event_batch",
            "afterOffset": 40,
            "latestOffset": 41,
            "gap": false,
            "events": [{
                "globalOffset": 41,
                "taskId": "00000000000000000000000001",
                "streamSequence": 1,
                "eventId": "00000000000000000000000002",
                "eventType": "assistant.text",
                "schemaVersion": 1,
                "recordedAt": recorded_at,
                "source": {
                    "kind": "assistant",
                    "actorId": null,
                    "clientId": null
                },
                "payload": {}
            }]
        }
    })
}
