#![cfg(feature = "anthropic")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError};
use harness_model::{anthropic::AnthropicProvider, *};
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn message(id: MessageId, text: &str) -> Message {
    Message {
        id,
        role: MessageRole::User,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: Utc::now(),
    }
}

fn request_with_messages(messages: Vec<Message>) -> ModelRequest {
    ModelRequest {
        model_id: "claude-3-5-sonnet-20241022".to_owned(),
        messages,
        tools: None,
        system: Some("System prompt".to_owned()),
        temperature: None,
        max_tokens: Some(128),
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Messages,
        extra: Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

fn provider(server: &MockServer) -> AnthropicProvider {
    AnthropicProvider::from_api_key("test-key").with_base_url(server.uri())
}

fn ok_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_01",
        "type": "message",
        "role": "assistant",
        "content": [
            { "type": "text", "text": "ok" }
        ],
        "model": "claude-3-5-sonnet-20241022",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 7,
            "output_tokens": 3,
            "cache_creation_input_tokens": 5,
            "cache_read_input_tokens": 2
        }
    }))
}

#[tokio::test]
async fn anthropic_cache_breakpoints_inject_cache_control_and_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ok_response())
        .mount(&server)
        .await;

    let first_id = MessageId::new();
    let second_id = MessageId::new();
    let mut req = request_with_messages(vec![
        message(first_id, "first"),
        message(second_id, "second"),
    ]);
    req.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: second_id,
        reason: BreakpointReason::RecentMessage,
    });

    let events = provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(
        body["system"],
        json!([{ "type": "text", "text": "System prompt", "cache_control": { "type": "ephemeral" } }])
    );
    assert_eq!(
        body["messages"][1]["content"][0]["cache_control"],
        json!({ "type": "ephemeral" })
    );
    assert_eq!(body["messages"][0]["content"][0].get("cache_control"), None);
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(harness_contracts::StopReason::EndTurn),
        usage_delta: harness_contracts::UsageSnapshot {
            input_tokens: 7,
            output_tokens: 3,
            cache_read_tokens: 2,
            cache_write_tokens: 5,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
}

#[tokio::test]
async fn anthropic_cache_validation_rejects_missing_duplicate_and_over_limit_breakpoints() {
    let server = MockServer::start().await;
    let target_id = MessageId::new();
    let missing_id = MessageId::new();

    let mut missing = request_with_messages(vec![message(target_id, "target")]);
    missing.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: missing_id,
        reason: BreakpointReason::RecentMessage,
    });
    let err = provider(&server)
        .infer(missing, InferContext::for_test())
        .await
        .err()
        .expect("missing breakpoint should fail");
    assert!(matches!(err, ModelError::InvalidRequest(_)));

    let mut duplicate = request_with_messages(vec![message(target_id, "target")]);
    duplicate.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: target_id,
        reason: BreakpointReason::RecentMessage,
    });
    duplicate.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: target_id,
        reason: BreakpointReason::Custom("again".to_owned()),
    });
    let err = provider(&server)
        .infer(duplicate, InferContext::for_test())
        .await
        .err()
        .expect("duplicate breakpoint should fail");
    assert!(matches!(err, ModelError::InvalidRequest(_)));

    let ids = (0..4).map(|_| MessageId::new()).collect::<Vec<_>>();
    let mut over_limit = request_with_messages(
        ids.iter()
            .map(|id| message(*id, "target"))
            .collect::<Vec<_>>(),
    );
    over_limit.cache_breakpoints = ids
        .into_iter()
        .map(|after_message_id| CacheBreakpoint {
            after_message_id,
            reason: BreakpointReason::RecentMessage,
        })
        .collect();
    let err = provider(&server)
        .infer(over_limit, InferContext::for_test())
        .await
        .err()
        .expect("over-limit breakpoint count should fail");
    assert!(matches!(err, ModelError::InvalidRequest(_)));
}

#[test]
fn anthropic_prompt_cache_style_and_token_counter_are_stable() {
    let provider = AnthropicProvider::from_api_key("test-key");
    assert_eq!(
        provider.prompt_cache_style(),
        PromptCacheStyle::Anthropic {
            mode: AnthropicCacheMode::SystemAnd3
        }
    );

    let counter = harness_model::anthropic::AnthropicTokenCounter;
    assert_eq!(counter.count_tokens("", "claude-3-5-sonnet-20241022"), 0);
    assert_eq!(
        counter.count_tokens("abcd", "claude-3-5-sonnet-20241022"),
        1
    );
    assert_eq!(
        counter.count_tokens("abcde", "claude-3-5-sonnet-20241022"),
        2
    );
    assert_eq!(
        counter.count_messages(
            &[message(MessageId::new(), "abcdefgh")],
            "claude-3-5-sonnet-20241022",
        ),
        8
    );
}
