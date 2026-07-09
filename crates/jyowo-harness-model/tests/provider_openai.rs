#![cfg(feature = "openai")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    BudgetMetric, DeferPolicy, Message, MessageId, MessagePart, MessageRole, ModelError,
    OverflowAction, ProviderRestriction, ResultBudget, StopReason, ToolDescriptor, ToolGroup,
    ToolOrigin, ToolProperties, TrustLevel, UsageSnapshot,
};
use harness_model::{openai::OpenAiProvider, *};
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

fn message(role: MessageRole, parts: Vec<MessagePart>) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts,
        created_at: Utc::now(),
    }
}

fn request(stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "gpt-5.4-mini".to_owned(),
        messages: vec![message(
            MessageRole::User,
            vec![MessagePart::Text("hello".to_owned())],
        )],
        tools: Some(vec![tool_descriptor()]),
        system: Some("You are precise.".to_owned()),
        temperature: Some(0.2),
        max_tokens: Some(128),
        stream,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Responses,
        extra: Value::Null,
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

fn tool_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "search".to_owned(),
        display_name: "Search".to_owned(),
        description: "Search docs".to_owned(),
        category: "search".to_owned(),
        group: ToolGroup::Search,
        version: "1.0.0".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"],
        }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: Vec::new(),
        budget: ResultBudget {
            metric: BudgetMetric::Chars,
            limit: 4096,
            on_overflow: OverflowAction::Offload,
            preview_head_chars: 512,
            preview_tail_chars: 512,
        },
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
        service_binding: None,
        metadata: harness_contracts::ToolDescriptorMetadata::default(),
    }
}

fn provider(server: &MockServer) -> OpenAiProvider {
    OpenAiProvider::from_api_key("test-key").with_base_url(server.uri())
}

struct HeaderCaptureMiddleware {
    seen_request_id: Arc<Mutex<Option<String>>>,
}

#[async_trait::async_trait]
impl InferMiddleware for HeaderCaptureMiddleware {
    fn middleware_id(&self) -> &str {
        "header-capture"
    }

    async fn on_response_headers(
        &self,
        headers: &http::HeaderMap,
        _ctx: &InferContext,
    ) -> Result<(), ModelError> {
        let request_id = headers
            .get("x-provider-request-id")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        *self.seen_request_id.lock().unwrap() = request_id;
        Ok(())
    }
}

#[test]
fn openai_provider_metadata_is_stable() {
    let provider = OpenAiProvider::from_api_key("test-key");

    assert_eq!(provider.provider_id(), "openai");
    assert_eq!(
        provider.prompt_cache_style(),
        PromptCacheStyle::OpenAi { auto: true }
    );
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == "gpt-5.4-mini"
            && model.conversation_capability.tool_calling
            && !model.conversation_capability.reasoning));
    assert_eq!(provider.default_protocol(), ModelProtocol::Responses);
}

#[tokio::test]
async fn openai_non_stream_request_posts_responses_and_maps_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "content": [{
                        "type": "output_text",
                        "text": "world"
                    }]
                },
                {
                    "type": "function_call",
                    "id": "item_1",
                    "call_id": "call_1",
                    "name": "search",
                    "arguments": "{\"query\":\"docs\"}"
                }
            ],
            "usage": {
                "input_tokens": 7,
                "output_tokens": 3,
                "input_tokens_details": { "cached_tokens": 2 }
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
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "call_1".to_owned(),
            name: "search".to_owned(),
        },
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson("{\"query\":\"docs\"}".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 7,
            output_tokens: 3,
            cache_read_tokens: 2,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events.contains(&ModelStreamEvent::MessageStop));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], "gpt-5.4-mini");
    assert_eq!(body["stream"], false);
    assert_eq!(body["max_output_tokens"], 128);
    assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 0.0001);
    assert_eq!(body["input"][0]["role"], "system");
    assert_eq!(body["input"][0]["content"], "You are precise.");
    assert_eq!(body["input"][1]["role"], "user");
    assert_eq!(body["input"][1]["content"], "hello");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "search");
}

#[tokio::test]
async fn openai_calls_response_header_middlewares() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-provider-request-id", "req_123")
                .set_body_json(json!({
                    "id": "resp_1",
                    "output": [{
                        "type": "message",
                        "id": "msg_1",
                        "content": [{ "type": "output_text", "text": "world" }]
                    }],
                    "usage": { "input_tokens": 7, "output_tokens": 3 }
                })),
        )
        .mount(&server)
        .await;
    let seen_request_id = Arc::new(Mutex::new(None));
    let mut ctx = InferContext::for_test();
    ctx.middlewares.push(Arc::new(HeaderCaptureMiddleware {
        seen_request_id: Arc::clone(&seen_request_id),
    }));

    provider(&server)
        .infer(request(false), ctx)
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(seen_request_id.lock().unwrap().as_deref(), Some("req_123"));
}

#[tokio::test]
async fn openai_stream_response_maps_text_tool_usage_and_done() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: response.output_text.delta\n",
                        "data: {\"delta\":\"hel\"}\n\n",
                        "event: response.output_text.delta\n",
                        "data: {\"delta\":\"lo\"}\n\n",
                        "event: response.output_item.added\n",
                        "data: {\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"search\"}}\n\n",
                        "event: response.function_call_arguments.delta\n",
                        "data: {\"delta\":\"{\\\"query\\\":\"}\n\n",
                        "event: response.function_call_arguments.delta\n",
                        "data: {\"delta\":\"\\\"docs\\\"}\"}\n\n",
                        "event: response.completed\n",
                        "data: {\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":8,\"output_tokens\":4,\"input_tokens_details\":{\"cached_tokens\":1}}}}\n\n",
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
        delta: ContentDelta::Text("hel".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("lo".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "call_1".to_owned(),
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
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 8,
            output_tokens: 4,
            cache_read_tokens: 1,
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
async fn openai_retries_rate_limit_and_transient_errors_but_not_auth() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(move |_req: &Request| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_json(json!({ "error": { "message": "rate limited" } }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "resp_1",
                    "output": [{
                        "type": "message",
                        "id": "msg_1",
                        "content": [{ "type": "output_text", "text": "ok" }]
                    }],
                    "usage": {}
                }))
            }
        })
        .mount(&server)
        .await;

    let stream = provider(&server)
        .infer(request(false), InferContext::for_test())
        .await
        .expect("rate limit should be retried");
    drop(stream);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    let auth_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "error": { "message": "bad key" } })),
        )
        .mount(&auth_server)
        .await;

    let err = provider(&auth_server)
        .infer(request(false), InferContext::for_test())
        .await
        .err()
        .expect("auth failure should fail");
    assert!(matches!(err, ModelError::AuthExpired(_)));
    assert_eq!(auth_server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn openai_rejects_unsupported_request_shapes() {
    let provider = OpenAiProvider::from_api_key("test-key");

    let mut unsupported_mode = request(false);
    unsupported_mode.protocol = ModelProtocol::ChatCompletions;
    assert!(matches!(
        provider
            .infer(unsupported_mode, InferContext::for_test())
            .await
            .err()
            .expect("unsupported mode should fail"),
        ModelError::InvalidRequest(_)
    ));

    let mut cache = request(false);
    cache.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: cache.messages[0].id,
        reason: BreakpointReason::RecentMessage,
    });
    assert!(matches!(
        provider
            .infer(cache, InferContext::for_test())
            .await
            .err()
            .expect("cache breakpoints should fail"),
        ModelError::InvalidRequest(_)
    ));

    let mut thinking = request(false);
    thinking.messages[0].parts = vec![MessagePart::Thinking(harness_contracts::ThinkingBlock {
        text: Some("think".to_owned()),
        provider_id: "openai".to_owned(),
        provider_native: None,
        signature: None,
    })];
    assert!(matches!(
        provider
            .infer(thinking, InferContext::for_test())
            .await
            .err()
            .expect("thinking parts should fail"),
        ModelError::InvalidRequest(_)
    ));
}
