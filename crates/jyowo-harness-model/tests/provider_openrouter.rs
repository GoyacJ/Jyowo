#![cfg(feature = "openrouter")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, StopReason, UsageSnapshot};
use harness_model::{
    openrouter::inventory_from_models_api_json, openrouter::OpenRouterProvider, *,
};
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "openai/gpt-5.5".to_owned(),
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
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: harness_model::ProviderRequestContext::default(),
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
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == "openai/gpt-5.5"
            && model.conversation_capability.tool_calling
            && !model
                .conversation_capability
                .input_modalities
                .contains(&ModelModality::Image)
            && !model.conversation_capability.reasoning));
}

#[test]
fn openrouter_models_api_inventory_marks_non_text_models_unsupported() {
    let inventory = inventory_from_models_api_json(
        json!({
            "data": [
                {
                    "id": "openai/gpt-text",
                    "name": "GPT Text",
                    "context_length": 128000,
                    "architecture": {
                        "input_modalities": ["text"],
                        "output_modalities": ["text"]
                    },
                    "top_provider": {
                        "max_completion_tokens": 8192
                    },
                    "supported_parameters": [
                        "tools",
                        "response_format",
                        "structured_outputs",
                        "reasoning",
                        "web_search_options"
                    ]
                },
                {
                    "id": "openai/gpt-image",
                    "name": "GPT Image",
                    "context_length": 32000,
                    "architecture": {
                        "input_modalities": ["text"],
                        "output_modalities": ["image"]
                    },
                    "top_provider": {
                        "max_completion_tokens": 4096
                    },
                    "supported_parameters": []
                }
            ]
        })
        .to_string()
        .as_bytes(),
    )
    .expect("models api response should parse");

    let text = inventory
        .iter()
        .find(|model| model.model_id == "openai/gpt-text")
        .expect("text model should be present");
    assert!(matches!(text.runtime_status, ModelRuntimeStatus::Runnable));
    assert!(text.conversation_capability.tool_calling);
    assert!(text.conversation_capability.structured_output);
    assert!(text.conversation_capability.reasoning);
    assert_eq!(
        text.conversation_capability.input_modalities,
        vec![ModelModality::Text]
    );
    assert_eq!(
        text.conversation_capability.output_modalities,
        vec![ModelModality::Text]
    );

    let image = inventory
        .iter()
        .find(|model| model.model_id == "openai/gpt-image")
        .expect("image output model should be present");
    assert!(matches!(
        image.runtime_status,
        ModelRuntimeStatus::Unsupported { .. }
    ));
    assert!(!image.conversation_capability.streaming);
    assert_eq!(
        image.conversation_capability.input_modalities,
        vec![ModelModality::Text]
    );
    assert_eq!(
        image.conversation_capability.output_modalities,
        vec![ModelModality::Image]
    );
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
    assert_eq!(body["model"], "openai/gpt-5.5");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
}

#[tokio::test]
async fn openrouter_stream_response_uses_openai_protocol_mapping() {
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
