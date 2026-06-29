#![cfg(feature = "openai")]

use std::fmt::Write as _;
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
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
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
    }
}

fn provider(server: &MockServer) -> OpenAiProvider {
    OpenAiProvider::from_api_key("test-key")
        .with_chat_completions_api()
        .with_base_url(server.uri())
}

fn openai_tool_argument_frame(fragment: &str, include_identity: bool) -> String {
    let tool_call = if include_identity {
        json!({
            "index": 0,
            "id": "call_char",
            "type": "function",
            "function": {
                "name": "search",
                "arguments": fragment,
            },
        })
    } else {
        json!({
            "index": 0,
            "function": {
                "arguments": fragment,
            },
        })
    };
    format!(
        "data: {}\n\n",
        json!({
            "id": "chatcmpl_1",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [tool_call],
                },
                "finish_reason": null,
            }],
            "usage": null,
        })
    )
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
async fn openai_non_stream_request_posts_chat_completions_and_maps_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "world",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\":\"docs\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 7,
                "completion_tokens": 3,
                "prompt_tokens_details": { "cached_tokens": 2 }
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
        stop_reason: Some(StopReason::ToolUse),
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
    assert_eq!(body["max_tokens"], 128);
    assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 0.0001);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "You are precise.");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], "hello");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "search");
}

#[tokio::test]
async fn openai_calls_response_header_middlewares() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-provider-request-id", "req_123")
                .set_body_json(json!({
                    "id": "chatcmpl_1",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "world"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 7,
                        "completion_tokens": 3
                    }
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
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hel\"},\"finish_reason\":null}],\"usage\":null}\n\n",
                        "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}],\"usage\":null}\n\n",
                        "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"{\\\"query\\\":\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
                        "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"docs\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4,\"prompt_tokens_details\":{\"cached_tokens\":1}}}\n\n",
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
        stop_reason: Some(StopReason::ToolUse),
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
    assert_eq!(body["stream_options"]["include_usage"], true);
}

#[tokio::test]
async fn openai_stream_response_preserves_character_level_tool_json_fragments() {
    let server = MockServer::start().await;
    let mut body = String::new();
    let argument_chars = ["{", "\"", "q", "\"", ":", "\"", "d", "\"", "}"];
    for (index, fragment) in argument_chars.iter().enumerate() {
        body.push_str(&openai_tool_argument_frame(fragment, index == 0));
    }
    write!(
        &mut body,
        "data: {}\n\n",
        json!({
            "id": "chatcmpl_1",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1
            }
        })
    )
    .expect("write OpenAI stream frame");
    body.push_str("data: [DONE]\n\n");
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(body, "text/event-stream"),
        )
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(request(true), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;
    let input_json_fragments = events
        .iter()
        .filter_map(|event| match event {
            ModelStreamEvent::ContentBlockDelta {
                delta: ContentDelta::ToolUseInputJson(fragment),
                ..
            } => Some(fragment.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::ToolUseStart { id, name },
            ..
        } if id == "call_char" && name == "search"
    )));
    assert_eq!(input_json_fragments, argument_chars);
    assert!(events.contains(&ModelStreamEvent::ContentBlockStop { index: 1 }));
}

#[tokio::test]
async fn openai_retries_rate_limit_and_transient_errors_but_not_auth() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_req: &Request| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_json(json!({ "error": { "message": "rate limited" } }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "chatcmpl_1",
                    "choices": [{ "message": { "content": "ok" }, "finish_reason": "stop" }],
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
        .and(path("/v1/chat/completions"))
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

    let unsupported_mode = request(false);
    assert!(matches!(
        provider
            .infer(unsupported_mode, InferContext::for_test())
            .await
            .err()
            .expect("unsupported mode should fail"),
        ModelError::InvalidRequest(_)
    ));

    let mut cache = request(false);
    cache.protocol = ModelProtocol::Responses;
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
    thinking.protocol = ModelProtocol::Responses;
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
