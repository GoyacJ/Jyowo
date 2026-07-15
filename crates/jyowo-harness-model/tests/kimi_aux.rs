#![cfg(feature = "km")]

use std::future::Future;

use chrono::Utc;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError};
use harness_model::{
    InferContext, KmProvider, ModelProtocol, ModelRequest, ModelRequestOptions,
    ProviderRequestContext,
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

#[tokio::test]
async fn kimi_files_api_supports_official_file_lifecycle() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/files"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "file-123",
            "object": "file",
            "bytes": 11,
            "created_at": 1780000000,
            "filename": "note.txt",
            "purpose": "file-extract",
            "status": "ready"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": []
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/files/file-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "file-123",
            "object": "file",
            "bytes": 11,
            "created_at": 1780000000,
            "filename": "note.txt",
            "purpose": "file-extract",
            "status": "ready"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/files/file-123/content"))
        .respond_with(ResponseTemplate::new(200).set_body_string("extracted text"))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/files/file-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "file-123",
            "object": "file",
            "deleted": true
        })))
        .mount(&server)
        .await;

    let provider = provider(&server);
    let uploaded = provider
        .upload_file(
            "file-extract",
            "note.txt",
            b"hello world".to_vec(),
            &InferContext::for_test(),
        )
        .await
        .expect("file upload should parse");
    assert_eq!(uploaded.id, "file-123");
    assert_eq!(uploaded.purpose, "file-extract");
    let context = InferContext::for_test();
    assert!(retry_local_mock(|| provider.list_files(None, &context))
        .await
        .expect("file list should parse")
        .data
        .is_empty());
    assert_eq!(
        retry_local_mock(|| provider.retrieve_file("file-123", &context))
            .await
            .expect("file retrieve should parse")
            .filename,
        "note.txt"
    );
    assert_eq!(
        retry_local_mock(|| provider.file_content("file-123", &context))
            .await
            .expect("file content should parse"),
        "extracted text"
    );
    assert!(
        retry_local_mock(|| provider.delete_file("file-123", &context))
            .await
            .expect("file delete should parse")
            .deleted
    );
}

#[tokio::test]
async fn kimi_batch_api_supports_official_endpoints() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/batches"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(batch_json("batch-123", "validating")),
        )
        .mount(&server)
        .await;
    assert_eq!(
        provider(&server)
            .create_batch(
                "file-batch",
                "/v1/chat/completions",
                "24h",
                None,
                &InferContext::for_test()
            )
            .await
            .expect("batch create should parse")
            .id,
        "batch-123"
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/batches"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [batch_json("batch-123", "completed")]
        })))
        .mount(&server)
        .await;
    assert_eq!(
        provider(&server)
            .list_batches(None, None, &InferContext::for_test())
            .await
            .expect("batch list should parse")
            .data
            .len(),
        1
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/batches/batch-123"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(batch_json("batch-123", "completed")),
        )
        .mount(&server)
        .await;
    assert_eq!(
        provider(&server)
            .retrieve_batch("batch-123", &InferContext::for_test())
            .await
            .expect("batch retrieve should parse")
            .status,
        "completed"
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/batches/batch-123/cancel"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(batch_json("batch-123", "cancelling")),
        )
        .mount(&server)
        .await;
    assert_eq!(
        provider(&server)
            .cancel_batch("batch-123", &InferContext::for_test())
            .await
            .expect("batch cancel should parse")
            .status,
        "cancelling"
    );
}

fn provider(server: &MockServer) -> KmProvider {
    KmProvider::from_api_key("provider-key").with_base_url(server.uri())
}

async fn retry_local_mock<T, F, Fut>(mut request: F) -> Result<T, ModelError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ModelError>>,
{
    match request().await {
        Err(ModelError::ProviderUnavailable(_)) => request().await,
        result => result,
    }
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
        options: ModelRequestOptions::default(),
        provider_context: ProviderRequestContext::default(),
    }
}

fn batch_json(id: &str, status: &str) -> Value {
    json!({
        "id": id,
        "object": "batch",
        "endpoint": "/v1/chat/completions",
        "input_file_id": "file-batch",
        "completion_window": "24h",
        "status": status,
        "created_at": 1780000000,
        "request_counts": {
            "completed": 1,
            "failed": 0,
            "total": 1
        }
    })
}
