#![cfg(feature = "openrouter")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, StopReason, UsageSnapshot};
use harness_model::{openrouter::OpenRouterProvider, *};
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "openai/gpt-4o-mini".to_owned(),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text("hello".to_owned())],
            created_at: Utc::now(),
        }],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(64),
        stream,
        cache_breakpoints: Vec::new(),
        api_mode: ApiMode::ChatCompletions,
        extra: Value::Null,
    }
}

fn provider(server: &MockServer) -> OpenRouterProvider {
    OpenRouterProvider::from_api_key("router-key").with_base_url(server.uri())
}

#[test]
fn openrouter_provider_metadata_is_stable() {
    let provider = OpenRouterProvider::from_api_key("router-key");

    assert_eq!(provider.provider_id(), "openrouter");
    assert_eq!(provider.prompt_cache_style(), PromptCacheStyle::None);
    assert!(provider.supports_tools());
    assert!(provider.supports_vision());
    assert!(!provider.supports_thinking());
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == "openai/gpt-4o-mini"));
}

#[tokio::test]
async fn openrouter_posts_chat_completions_with_provider_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer router-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "or_1",
            "choices": [{
                "message": { "role": "assistant", "content": "world" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 2
            }
        })))
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(request(false), InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("world".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 5,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], "openai/gpt-4o-mini");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
}

#[tokio::test]
async fn openrouter_stream_response_uses_openai_compatible_mapping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"or_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(request(true), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("hi".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 3,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events.contains(&ModelStreamEvent::MessageStop));
}

#[tokio::test]
async fn openrouter_uses_shared_error_mapping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": { "code": "overloaded", "message": "busy" }
        })))
        .mount(&server)
        .await;

    let mut ctx = InferContext::for_test();
    ctx.retry_policy.max_attempts = 1;

    let err = provider(&server)
        .infer(request(false), ctx)
        .await
        .err()
        .expect("provider error should fail");

    assert!(
        matches!(err, harness_contracts::ModelError::ProviderUnavailable(message) if message == "busy")
    );
}
