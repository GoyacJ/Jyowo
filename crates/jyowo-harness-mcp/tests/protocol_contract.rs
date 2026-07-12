use harness_mcp::{
    InitializeParams, InitializeResult, McpContent, McpMessage, McpPrompt, McpResource,
    McpResourceContents, McpToolDescriptor, McpToolResult, LATEST_PROTOCOL_VERSION,
};
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
fn request_and_notification_params_must_be_objects() {
    for id in [Some(json!(1)), None] {
        for params in [json!(null), json!(1), json!("bad"), json!([])] {
            let mut fixture = json!({
                "jsonrpc": "2.0",
                "method": "ping",
                "params": params
            });
            if let Some(id) = id.clone() {
                fixture["id"] = id;
            }
            assert!(serde_json::from_value::<McpMessage>(fixture).is_err());
        }
    }
}

#[test]
fn success_result_must_be_an_object() {
    for result in [json!(null), json!(1), json!("bad"), json!([])] {
        let fixture = json!({ "jsonrpc": "2.0", "id": 1, "result": result });
        assert!(serde_json::from_value::<McpMessage>(fixture).is_err());
    }

    let fixture = json!({ "jsonrpc": "2.0", "id": 1, "result": {} });
    assert!(matches!(
        serde_json::from_value::<McpMessage>(fixture).unwrap(),
        McpMessage::SuccessResponse(_)
    ));
}

#[test]
fn official_initialize_capabilities_and_implementation_round_trip() {
    let client_fixture = json!({
        "protocolVersion": "2025-11-25",
        "capabilities": {
            "sampling": { "context": {}, "tools": {} },
            "tasks": {
                "list": {},
                "cancel": {},
                "requests": {
                    "sampling": { "createMessage": {} },
                    "elicitation": { "create": {} }
                }
            }
        },
        "clientInfo": {
            "name": "fixture-client",
            "title": "Fixture Client",
            "version": "1.0.0",
            "icons": [{
                "src": "https://example.com/icon.svg",
                "mimeType": "image/svg+xml",
                "sizes": ["any"],
                "theme": "dark"
            }]
        }
    });
    let client: InitializeParams = serde_json::from_value(client_fixture.clone()).unwrap();
    let sampling = client.capabilities.sampling.as_ref().unwrap();
    assert!(sampling.context.is_some());
    assert!(sampling.tools.is_some());
    let tasks = client.capabilities.tasks.as_ref().unwrap();
    assert!(tasks.list.is_some());
    assert!(tasks.cancel.is_some());
    assert!(tasks.requests.as_ref().unwrap().sampling.is_some());
    assert_eq!(
        client.client_info.icons.as_ref().unwrap()[0].sizes,
        Some(vec!["any".to_owned()])
    );
    assert_eq!(serde_json::to_value(client).unwrap(), client_fixture);

    let server_fixture = json!({
        "protocolVersion": "2025-11-25",
        "capabilities": {
            "tools": { "listChanged": true },
            "tasks": {
                "list": {},
                "cancel": {},
                "requests": { "tools": { "call": {} } }
            }
        },
        "serverInfo": {
            "name": "fixture-server",
            "version": "2.0.0",
            "icons": [{ "src": "data:image/png;base64,AA==", "theme": "light" }]
        }
    });
    let server: InitializeResult = serde_json::from_value(server_fixture.clone()).unwrap();
    let tasks = server.capabilities.tasks.as_ref().unwrap();
    assert!(tasks.list.is_some());
    assert!(tasks.cancel.is_some());
    assert!(tasks.requests.as_ref().unwrap().tools.is_some());
    assert_eq!(server.server_info.icons.as_ref().unwrap().len(), 1);
    assert_eq!(serde_json::to_value(server).unwrap(), server_fixture);
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

#[test]
fn official_tool_descriptor_and_result_round_trip() {
    let tool_fixture = json!({
        "name": "weather_current",
        "title": "Current weather",
        "description": "Read current weather conditions",
        "icons": [{
            "src": "https://example.com/weather.png",
            "mimeType": "image/png",
            "sizes": []
        }],
        "inputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"]
        },
        "execution": { "taskSupport": "optional" },
        "outputSchema": {
            "type": "object",
            "properties": { "temperature": { "type": "number" } }
        },
        "annotations": {
            "title": "Weather lookup",
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": true
        },
        "_meta": { "vendor/tool-id": "weather-v2" }
    });
    let tool: McpToolDescriptor = serde_json::from_value(tool_fixture.clone()).unwrap();
    assert_eq!(tool.title.as_deref(), Some("Current weather"));
    assert_eq!(tool.icons.as_ref().unwrap()[0].sizes, Some(Vec::new()));
    assert_eq!(
        tool.execution
            .as_ref()
            .unwrap()
            .task_support
            .unwrap()
            .to_string(),
        "optional"
    );
    assert_eq!(serde_json::to_value(tool).unwrap(), tool_fixture);

    let result_fixture = json!({
        "content": [
            { "type": "text", "text": "18 C", "annotations": { "audience": ["assistant"] } },
            { "type": "image", "data": "AA==", "mimeType": "image/png" },
            { "type": "audio", "data": "AQ==", "mimeType": "audio/wav" },
            {
                "type": "resource_link",
                "uri": "weather://london",
                "name": "London weather",
                "title": "Observation",
                "size": 128
            },
            {
                "type": "resource",
                "resource": {
                    "uri": "weather://london/raw",
                    "mimeType": "application/json",
                    "text": "{\"temperature\":18}"
                }
            },
            { "type": "vendor_chart", "series": [18, 19], "vendor": true }
        ],
        "structuredContent": { "temperature": 18 },
        "isError": false,
        "_meta": { "trace": "abc" }
    });
    let result: McpToolResult = serde_json::from_value(result_fixture.clone()).unwrap();
    assert_eq!(
        result.structured_content,
        Some(json!({ "temperature": 18 }))
    );
    assert!(matches!(
        result.content.last(),
        Some(McpContent::Unknown(_))
    ));
    assert_eq!(serde_json::to_value(result).unwrap(), result_fixture);
}

#[test]
fn official_resource_and_prompt_models_round_trip() {
    let resource_fixture = json!({
        "uri": "file:///tmp/report.pdf",
        "name": "report",
        "title": "Quarterly report",
        "description": "Published report",
        "mimeType": "application/pdf",
        "icons": [{ "src": "data:image/png;base64,AA==" }],
        "annotations": {
            "audience": ["user", "assistant"],
            "priority": 0.8,
            "lastModified": "2025-11-25T12:00:00Z"
        },
        "size": 4096,
        "_meta": { "etag": "v2" }
    });
    let resource: McpResource = serde_json::from_value(resource_fixture.clone()).unwrap();
    assert_eq!(resource.size, Some(4096));
    assert_eq!(serde_json::to_value(resource).unwrap(), resource_fixture);

    for fixture in [
        json!({
            "uri": "file:///tmp/readme.txt",
            "mimeType": "text/plain",
            "text": "hello",
            "_meta": { "part": 1 }
        }),
        json!({
            "uri": "file:///tmp/image.png",
            "mimeType": "image/png",
            "blob": "AA=="
        }),
    ] {
        let contents: McpResourceContents = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(serde_json::to_value(contents).unwrap(), fixture);
    }

    let prompt_fixture = json!({
        "name": "summarize",
        "title": "Summarize a document",
        "description": "Builds a summary prompt",
        "icons": [{ "src": "https://example.com/prompt.svg", "theme": "dark" }],
        "arguments": [{
            "name": "style",
            "title": "Summary style",
            "description": "Desired writing style",
            "required": true
        }],
        "_meta": { "version": 2 }
    });
    let prompt: McpPrompt = serde_json::from_value(prompt_fixture.clone()).unwrap();
    assert_eq!(prompt.arguments.as_ref().unwrap()[0].required, Some(true));
    assert_eq!(serde_json::to_value(prompt).unwrap(), prompt_fixture);
}
