#![cfg(feature = "anthropic")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, StopReason, UsageSnapshot};
use harness_model::{anthropic::AnthropicProvider, *};
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn message(text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: Utc::now(),
    }
}

fn request() -> ModelRequest {
    ModelRequest {
        model_id: "claude-3-5-sonnet-20241022".to_owned(),
        messages: vec![message("hello")],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(128),
        stream: true,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Messages,
        extra: Value::Null,
    }
}

fn provider(server: &MockServer) -> AnthropicProvider {
    AnthropicProvider::from_api_key("test-key").with_base_url(server.uri())
}

#[tokio::test]
async fn anthropic_stream_text_response_emits_model_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: message_start\n",
                        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream\",\"usage\":{\"input_tokens\":7,\"output_tokens\":0}}}\n\n",
                        "event: content_block_start\n",
                        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
                        "event: content_block_stop\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                        "event: message_delta\n",
                        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(request(), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::MessageStart {
        message_id: "msg_stream".to_owned(),
        usage: UsageSnapshot {
            input_tokens: 7,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("hel".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("lo".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 0,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events.contains(&ModelStreamEvent::MessageStop));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn anthropic_stream_tool_use_emits_start_and_json_deltas() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: content_block_start\n",
                        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"search\",\"input\":{}}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\"}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"docs\\\"}\"}}\n\n",
                        "event: content_block_stop\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(request(), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "toolu_1".to_owned(),
            name: "search".to_owned(),
        },
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson("{\"query\":".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson("\"docs\"}".to_owned()),
    }));
    assert!(!events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::ToolUseComplete { .. },
            ..
        }
    )));
}

#[tokio::test]
async fn anthropic_stream_thinking_signature_error_and_cache_usage_are_mapped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: content_block_start\r\n",
                        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\r\n\r\n",
                        ": keepalive\r\n",
                        "event: content_block_delta\r\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"reason\"}}\r\n\r\n",
                        "event: content_block_delta\r\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-1\"}}\r\n\r\n",
                        "event: message_delta\r\n",
                        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"input_tokens\":20,\"output_tokens\":5,\"cache_creation_input_tokens\":10,\"cache_read_input_tokens\":2}}\r\n\r\n",
                        "event: error\r\n",
                        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"busy\"}}\r\n\r\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(request(), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Thinking(ThinkingDelta {
            text: Some("reason".to_owned()),
            provider_native: Some(json!({"type":"thinking_delta","thinking":"reason"})),
            signature: None,
        }),
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Thinking(ThinkingDelta {
            text: None,
            provider_native: Some(json!({"type":"signature_delta","signature":"sig-1"})),
            signature: Some("sig-1".to_owned()),
        }),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::MaxIterations),
        usage_delta: UsageSnapshot {
            input_tokens: 20,
            output_tokens: 5,
            cache_read_tokens: 2,
            cache_write_tokens: 10,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events
        .iter()
        .any(|event| matches!(event, ModelStreamEvent::StreamError {
        error: harness_contracts::ModelError::ProviderUnavailable(message),
        class: ErrorClass::Transient,
        ..
    } if message == "busy")));
}
