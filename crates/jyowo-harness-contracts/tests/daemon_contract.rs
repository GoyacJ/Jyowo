use harness_contracts::{
    daemon_protocol_schema, ClientFrame, ClientRequest, ServerFrame, PROTOCOL_VERSION,
};
use serde_json::json;

#[test]
fn daemon_protocol_exports_one_versioned_schema() {
    assert_eq!(PROTOCOL_VERSION, 1);

    let value = serde_json::to_value(daemon_protocol_schema()).expect("serialize daemon schema");
    let text = serde_json::to_string(&value).expect("render daemon schema");
    for required in [
        "handshake",
        "submit_message",
        "edit_queued_message",
        "delete_queued_message",
        "promote_queued_message",
        "resolve_permission",
        "subscribe_events",
        "read_blob",
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
