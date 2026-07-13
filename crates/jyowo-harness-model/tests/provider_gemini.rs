#![cfg(feature = "gemini")]

use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    BlobError, BlobMeta, BlobRef, BlobStore, Message, MessageId, MessagePart, MessageRole,
    ModelError, StopReason, TenantId, UsageSnapshot,
};
use harness_model::{gemini::GeminiProvider, *};
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "gemini-2.5-flash".to_owned(),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text("hello".to_owned())],
            created_at: Utc::now(),
        }],
        tools: None,
        system: Some("Be terse.".to_owned()),
        temperature: Some(0.2),
        max_tokens: Some(64),
        stream,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::GenerateContent,
        extra: json!({ "cached_content": "cachedContents/abc123" }),
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

struct StaticBlobStore {
    bytes: Bytes,
}

#[async_trait::async_trait]
impl BlobStore for StaticBlobStore {
    fn store_id(&self) -> &str {
        "static"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        _meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Err(BlobError::Backend("not implemented".to_owned()))
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        Ok(Box::pin(futures::stream::once({
            let bytes = self.bytes.clone();
            async move { bytes }
        })))
    }

    async fn head(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<Option<BlobMeta>, BlobError> {
        Ok(None)
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

fn blob_ref(size: u64, content_type: &str) -> BlobRef {
    BlobRef {
        id: harness_contracts::BlobId::new(),
        size,
        content_hash: [7; 32],
        content_type: Some(content_type.to_owned()),
    }
}

#[test]
fn gemini_provider_metadata_is_stable() {
    let provider = GeminiProvider::from_api_key("test-key");

    assert_eq!(provider.provider_id(), "gemini");
    assert_eq!(GEMINI_API_KEY_ENV, "GEMINI_API_KEY");
    assert_eq!(
        provider.prompt_cache_style(),
        PromptCacheStyle::Gemini {
            mode: GeminiCacheMode::ExternalReferenceOnly,
        }
    );
    let models = provider.supported_models();
    let flash = models
        .iter()
        .find(|model| model.model_id == "gemini-2.5-flash")
        .expect("Gemini 2.5 Flash should be listed");
    assert!(flash.conversation_capability.tool_calling);
    assert!(flash
        .conversation_capability
        .input_modalities
        .contains(&ModelModality::Image));
}

#[tokio::test]
async fn gemini_streams_text_tool_usage_and_request_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
        ))
        .and(header("x-goog-api-key", "test-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"responseId\":\"resp_1\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"candidatesTokenCount\":1,\"cachedContentTokenCount\":2}}\n\n",
                        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"search\",\"args\":{\"query\":\"docs\"}}}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":4,\"candidatesTokenCount\":2,\"cachedContentTokenCount\":2}}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = GeminiProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(request(true), InferContext::for_test())
        .await
        .expect("stream should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("hi".to_owned()),
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::ToolUseStart { name, .. },
            ..
        } if name == "search"
    )));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson("{\"query\":\"docs\"}".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: harness_contracts::UsageSnapshot {
            input_tokens: 4,
            output_tokens: 2,
            cache_read_tokens: 2,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be terse.");
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(body["contents"][0]["parts"][0]["text"], "hello");
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 64);
    assert!((body["generationConfig"]["temperature"].as_f64().unwrap() - 0.2).abs() < 0.0001);
    assert_eq!(body["cachedContent"], "cachedContents/abc123");
}

#[tokio::test]
async fn gemini_stream_eof_without_finish_reason_does_not_emit_message_stop() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "data: {\"responseId\":\"resp_1\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n\n",
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = GeminiProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(request(true), InferContext::for_test())
        .await
        .expect("stream should start")
        .collect::<Vec<_>>()
        .await;

    assert!(!events.contains(&ModelStreamEvent::MessageStop));
}

#[tokio::test]
async fn gemini_request_merges_provider_defaults_extra() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
        })))
        .mount(&server)
        .await;

    let mut req = request(false);
    req.extra = json!({
        "thinkingConfig": { "thinkingBudget": 1024 },
        "stopSequences": ["DONE"],
        "topP": 0.8,
        "topK": 32,
        "seed": 7,
        "responseMimeType": "application/json",
        "safetySettings": [{ "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_ONLY_HIGH" }],
        "cachedContent": "cachedContents/provider-default"
    });

    let events = GeminiProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(req, InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;
    let total_usage = events
        .iter()
        .fold(UsageSnapshot::default(), |mut total, event| {
            let usage = match event {
                ModelStreamEvent::MessageStart { usage, .. } => usage,
                ModelStreamEvent::MessageDelta { usage_delta, .. } => usage_delta,
                _ => return total,
            };
            total.input_tokens += usage.input_tokens;
            total.output_tokens += usage.output_tokens;
            total
        });
    assert_eq!(total_usage.input_tokens, 1);
    assert_eq!(total_usage.output_tokens, 1);

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        1024
    );
    assert_eq!(body["generationConfig"]["stopSequences"], json!(["DONE"]));
    assert_eq!(body["generationConfig"]["topP"], json!(0.8));
    assert_eq!(body["generationConfig"]["topK"], json!(32));
    assert_eq!(body["generationConfig"]["seed"], json!(7));
    assert_eq!(
        body["generationConfig"]["responseMimeType"],
        "application/json"
    );
    assert_eq!(body["safetySettings"][0]["threshold"], "BLOCK_ONLY_HIGH");
    assert_eq!(body["cachedContent"], "cachedContents/provider-default");
}

#[tokio::test]
async fn gemini_request_encodes_multimodal_parts_and_official_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
        })))
        .mount(&server)
        .await;

    let image = blob_ref(4, "image/png");
    let file = blob_ref(4, "application/pdf");
    let mut req = request(false);
    req.messages[0].parts = vec![
        MessagePart::Text("describe".to_owned()),
        MessagePart::Image {
            mime_type: "image/png".to_owned(),
            blob_ref: image,
        },
        MessagePart::File {
            mime_type: "application/pdf".to_owned(),
            blob_ref: file,
        },
        MessagePart::ProviderFileReference {
            provider_id: "gemini".to_owned(),
            file_id: "files/report".to_owned(),
            mime_type: "application/pdf".to_owned(),
        },
    ];
    req.extra = json!({
        "thinkingConfig": { "includeThoughts": true, "thinkingLevel": "HIGH" },
        "responseJsonSchema": { "type": "object", "properties": { "ok": { "type": "boolean" } } },
        "serviceTier": "standard",
        "store": false,
        "toolConfig": { "functionCallingConfig": { "mode": "AUTO" } }
    });

    let mut ctx = InferContext::for_test();
    ctx.blob_store = Some(Arc::new(StaticBlobStore {
        bytes: Bytes::from_static(b"data"),
    }));

    GeminiProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(req, ctx)
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["contents"][0]["parts"][0]["text"], "describe");
    assert_eq!(
        body["contents"][0]["parts"][1]["inlineData"]["mimeType"],
        "image/png"
    );
    assert_eq!(
        body["contents"][0]["parts"][1]["inlineData"]["data"],
        "ZGF0YQ=="
    );
    assert_eq!(
        body["contents"][0]["parts"][2]["inlineData"]["mimeType"],
        "application/pdf"
    );
    assert_eq!(
        body["contents"][0]["parts"][3]["fileData"]["fileUri"],
        "files/report"
    );
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "HIGH"
    );
    assert_eq!(
        body["generationConfig"]["responseJsonSchema"]["type"],
        "object"
    );
    assert_eq!(body["serviceTier"], "standard");
    assert_eq!(body["store"], false);
    assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
}

#[tokio::test]
async fn gemini_rejects_oversized_inline_multimodal_blob() {
    let image = blob_ref(20 * 1024 * 1024 + 1, "image/png");
    let mut req = request(false);
    req.messages[0].parts = vec![MessagePart::Image {
        mime_type: "image/png".to_owned(),
        blob_ref: image,
    }];
    let mut ctx = InferContext::for_test();
    ctx.blob_store = Some(Arc::new(StaticBlobStore {
        bytes: Bytes::from_static(b"data"),
    }));

    let error = match GeminiProvider::from_api_key("test-key")
        .infer(req, ctx)
        .await
    {
        Ok(_) => panic!("oversized inline blob should be rejected"),
        Err(error) => error,
    };

    assert!(matches!(error, ModelError::InvalidRequest(message) if message.contains("20 MiB")));
}

#[tokio::test]
async fn gemini_rejects_cache_breakpoints() {
    let mut req = request(false);
    req.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: req.messages[0].id,
        reason: BreakpointReason::RecentMessage,
    });

    let error = match GeminiProvider::from_api_key("test-key")
        .infer(req, InferContext::for_test())
        .await
    {
        Ok(_) => panic!("cache breakpoints are not created by GeminiProvider"),
        Err(error) => error,
    };

    assert!(matches!(error, ModelError::InvalidRequest(_)));
}
