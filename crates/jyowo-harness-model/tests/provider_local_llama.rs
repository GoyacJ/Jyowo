#![cfg(feature = "local-llama")]

use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, StopReason, UsageSnapshot};
use harness_model::{local_llama::LocalLlamaProvider, *};
use serde_json::Value;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request() -> ModelRequest {
    ModelRequest {
        model_id: "llama3.1".to_owned(),
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
        stream: true,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

#[test]
fn local_llama_provider_metadata_is_stable() {
    let provider = LocalLlamaProvider::default();

    assert_eq!(provider.provider_id(), "local-llama");
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == "llama3.1"
            && model.conversation_capability.tool_calling
            && !model
                .conversation_capability
                .input_modalities
                .contains(&ModelModality::Image)));
}

#[tokio::test]
async fn local_llama_uses_openai_protocol_local_endpoint_without_auth_by_default() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"chat_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = LocalLlamaProvider::new(server.uri())
        .with_timeout(Duration::from_secs(5))
        .with_max_concurrency(1)
        .infer(request(), InferContext::for_test())
        .await
        .expect("stream should start")
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

    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn local_llama_can_use_optional_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer local-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chat_1",
            "choices": [{ "message": { "content": "ok" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })))
        .mount(&server)
        .await;

    let mut req = request();
    req.stream = false;
    LocalLlamaProvider::new(server.uri())
        .with_api_key("local-key")
        .infer(req, InferContext::for_test())
        .await
        .expect("non-stream request should succeed")
        .collect::<Vec<_>>()
        .await;
}
