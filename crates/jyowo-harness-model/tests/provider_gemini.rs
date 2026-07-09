#![cfg(feature = "gemini")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError, StopReason};
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
    assert_eq!(
        flash.conversation_capability.input_modalities,
        vec![ModelModality::Text]
    );
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

    GeminiProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(req, InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

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
