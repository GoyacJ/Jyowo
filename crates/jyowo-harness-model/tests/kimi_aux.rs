#![cfg(feature = "km")]

use chrono::Utc;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole};
use harness_model::{
    InferContext, KmProvider, ModelProtocol, ModelRequest, ProviderRequestContext,
};
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn kimi_list_models_parses_runtime_capability_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {
                    "id": "kimi-k2.6",
                    "object": "model",
                    "created": 1780000000,
                    "owned_by": "moonshot",
                    "context_length": 256000,
                    "supports_image_in": true,
                    "supports_video_in": true,
                    "supports_reasoning": true,
                }
            ]
        })))
        .mount(&server)
        .await;

    let models = provider(&server)
        .list_models(&InferContext::for_test())
        .await
        .expect("Kimi model list should parse");

    assert_eq!(models.data.len(), 1);
    let model = &models.data[0];
    assert_eq!(model.id, "kimi-k2.6");
    assert_eq!(model.object.as_deref(), Some("model"));
    assert_eq!(model.created, Some(1_780_000_000));
    assert_eq!(model.owned_by.as_deref(), Some("moonshot"));
    assert_eq!(model.context_length, Some(256_000));
    assert!(model.supports_image_in);
    assert!(model.supports_video_in);
    assert!(model.supports_reasoning);
}

#[tokio::test]
async fn kimi_estimate_token_count_reads_total_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tokenizers/estimate-token-count"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "total_tokens": 42 }
        })))
        .mount(&server)
        .await;

    let tokens = provider(&server)
        .estimate_token_count(&request(), &InferContext::for_test())
        .await
        .expect("Kimi token estimate should parse");

    assert_eq!(tokens, 42);
    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], "kimi-k2.6");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
}

fn provider(server: &MockServer) -> KmProvider {
    KmProvider::from_api_key("provider-key").with_base_url(server.uri())
}

fn request() -> ModelRequest {
    ModelRequest {
        model_id: "kimi-k2.6".to_owned(),
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
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        provider_context: ProviderRequestContext::default(),
    }
}
