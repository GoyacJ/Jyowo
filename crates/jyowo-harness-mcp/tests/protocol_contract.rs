use harness_mcp::{McpMessage, LATEST_PROTOCOL_VERSION};
use serde_json::json;

#[test]
fn latest_protocol_version_is_2025_11_25() {
    assert_eq!(LATEST_PROTOCOL_VERSION, "2025-11-25");
}

#[test]
fn classifies_requests_with_numeric_and_string_ids() {
    for id in [json!(7), json!("request-7")] {
        let fixture = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {},
            "vendorField": { "trace": true }
        });

        let message: McpMessage = serde_json::from_value(fixture.clone()).unwrap();
        let McpMessage::Request(request) = message else {
            panic!("request fixture was misclassified");
        };
        assert_eq!(request.id, id);
        assert_eq!(request.extra.get("vendorField"), fixture.get("vendorField"));
        assert_eq!(
            serde_json::to_value(McpMessage::Request(request)).unwrap(),
            fixture
        );
    }
}

#[test]
fn notification_has_no_id_and_preserves_unknown_fields() {
    let fixture = json!({
        "jsonrpc": "2.0",
        "method": "notifications/tools/list_changed",
        "params": { "reason": "plugin-loaded" },
        "vendorField": 42
    });

    let message: McpMessage = serde_json::from_value(fixture.clone()).unwrap();
    let McpMessage::Notification(notification) = message else {
        panic!("notification fixture was misclassified");
    };
    assert_eq!(notification.extra.get("vendorField"), Some(&json!(42)));

    let encoded = serde_json::to_value(McpMessage::Notification(notification)).unwrap();
    assert_eq!(encoded, fixture);
    assert!(encoded.get("id").is_none());
}

#[test]
fn classifies_success_and_error_responses() {
    let success_fixture = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "tools": [] },
        "vendorField": "success-extension"
    });
    let error_fixture = json!({
        "jsonrpc": "2.0",
        "error": {
            "code": -32601,
            "message": "Method not found",
            "data": { "method": "missing" },
            "vendorErrorField": true
        },
        "vendorField": "error-extension"
    });

    let success: McpMessage = serde_json::from_value(success_fixture.clone()).unwrap();
    let McpMessage::SuccessResponse(success) = success else {
        panic!("success fixture was misclassified");
    };
    assert_eq!(
        success.extra.get("vendorField"),
        Some(&json!("success-extension"))
    );

    let error: McpMessage = serde_json::from_value(error_fixture.clone()).unwrap();
    let McpMessage::ErrorResponse(error) = error else {
        panic!("error fixture was misclassified");
    };
    assert_eq!(error.id, None);
    assert_eq!(
        error.extra.get("vendorField"),
        Some(&json!("error-extension"))
    );
    assert_eq!(error.error.code, -32601);
    assert_eq!(
        error.error.extra.get("vendorErrorField"),
        Some(&json!(true))
    );

    assert_eq!(
        serde_json::to_value(McpMessage::SuccessResponse(success)).unwrap(),
        success_fixture
    );
    assert_eq!(
        serde_json::to_value(McpMessage::ErrorResponse(error)).unwrap(),
        error_fixture
    );
}

#[test]
fn result_null_is_a_success_response_and_round_trips() {
    let fixture = json!({ "jsonrpc": "2.0", "id": "nullable", "result": null });

    let message: McpMessage = serde_json::from_value(fixture.clone()).unwrap();
    let McpMessage::SuccessResponse(response) = message else {
        panic!("result:null was not classified as success");
    };
    assert!(response.result.is_null());
    assert_eq!(
        serde_json::to_value(McpMessage::SuccessResponse(response)).unwrap(),
        fixture
    );
}

#[test]
fn error_response_id_is_optional_but_never_null() {
    for id in [json!(7), json!("request-7")] {
        let fixture = json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32603, "message": "Internal error" }
        });
        let message: McpMessage = serde_json::from_value(fixture.clone()).unwrap();
        let McpMessage::ErrorResponse(response) = message else {
            panic!("error response was misclassified");
        };
        assert_eq!(response.id.as_ref(), Some(&id));
        assert_eq!(
            serde_json::to_value(McpMessage::ErrorResponse(response)).unwrap(),
            fixture
        );
    }

    for invalid_id in [json!(null), json!(1.5), json!({ "bad": true })] {
        let fixture = json!({
            "jsonrpc": "2.0",
            "id": invalid_id,
            "error": { "code": -32603, "message": "Internal error" }
        });
        assert!(serde_json::from_value::<McpMessage>(fixture).is_err());
    }
}

#[test]
fn rejects_ambiguous_or_invalid_message_shapes() {
    for fixture in [
        json!({ "jsonrpc": "2.0", "id": 1, "result": {}, "error": { "code": -1, "message": "bad" } }),
        json!({ "jsonrpc": "2.0", "id": 1 }),
        json!({ "jsonrpc": "2.0", "method": "tools/list", "id": null }),
        json!({ "jsonrpc": "2.0", "method": "tools/list", "id": { "bad": true } }),
        json!({ "jsonrpc": "2.0", "result": {} }),
        json!({ "jsonrpc": "1.0", "method": "tools/list" }),
    ] {
        assert!(
            serde_json::from_value::<McpMessage>(fixture).is_err(),
            "invalid message was accepted"
        );
    }
}
