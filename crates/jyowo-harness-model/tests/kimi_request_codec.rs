#![cfg(feature = "km")]

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures::{stream, StreamExt};
use harness_contracts::{
    BlobError, BlobId, BlobMeta, BlobRef, BlobRetention, BlobStore, BudgetMetric, DeferPolicy,
    KimiChatOptions, KimiPartialAssistant, Message, MessageId, MessagePart, MessageRole,
    ModelError, ModelModality, OverflowAction, ProviderRestriction, ResultBudget, ToolDescriptor,
    ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolResultPart, ToolUseId, TrustLevel,
};
use harness_model::{KmProvider, *};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn kimi_merges_extra_web_search_with_function_tools_and_disables_thinking() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.tools = Some(vec![tool_descriptor("search")]);
    req.extra = json!({
        "tools": [
            { "type": "builtin_function", "function": { "name": "$web_search" } }
        ]
    });

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi request should succeed")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    let tools = body["tools"].as_array().expect("tools should be array");
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "search");
    assert_eq!(tools[1]["type"], "builtin_function");
    assert_eq!(tools[1]["function"]["name"], "$web_search");
    assert_eq!(body["thinking"], json!({ "type": "disabled" }));
}

#[tokio::test]
async fn kimi_web_search_forces_disabled_thinking() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.5");
    req.extra = json!({
        "thinking": { "type": "enabled" },
        "tools": [
            { "type": "builtin_function", "function": { "name": "$web_search" } }
        ]
    });

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi request should succeed")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["thinking"], json!({ "type": "disabled" }));
}

#[tokio::test]
async fn kimi_k27_rejects_web_search_before_request() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.7-code");
    req.extra = json!({
        "tools": [
            { "type": "builtin_function", "function": { "name": "$web_search" } }
        ]
    });

    let error = match provider(&server).infer(req, InferContext::for_test()).await {
        Ok(_) => panic!("K2.7 cannot disable thinking for web search"),
        Err(error) => error,
    };

    assert!(
        matches!(error, ModelError::InvalidRequest(message) if message.contains("$web_search"))
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn kimi_k2_allows_official_extra_sampling_parameters() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.extra = json!({
        "temperature": 1.0,
        "top_p": 0.95,
        "n": 1,
        "presence_penalty": 0.0,
        "frequency_penalty": 0.0
    });

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("valid explicit Kimi K2 sampling parameters should pass")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["temperature"], 1.0);
    assert_eq!(body["top_p"], 0.95);
    assert_eq!(body["n"], 1);
}

#[tokio::test]
async fn kimi_k2_rejects_non_official_extra_sampling_parameters_before_request() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.extra = json!({ "top_p": 0.9 });

    let error = match provider(&server).infer(req, InferContext::for_test()).await {
        Ok(_) => panic!("invalid sampling parameter should fail closed"),
        Err(error) => error,
    };

    assert!(matches!(error, ModelError::InvalidRequest(message) if message.contains("top_p")));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn kimi_k2_disabled_thinking_allows_disabled_mode_temperature() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.extra = json!({
        "thinking": { "type": "disabled" },
        "temperature": 0.6
    });

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi K2 disabled thinking temperature should pass")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["thinking"], json!({ "type": "disabled" }));
    assert_eq!(body["temperature"], 0.6);
}

#[tokio::test]
async fn kimi_rejects_invalid_function_tool_name_before_request() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.tools = Some(vec![tool_descriptor("ab")]);

    let error = match provider(&server).infer(req, InferContext::for_test()).await {
        Ok(_) => panic!("invalid tool name should fail closed"),
        Err(error) => error,
    };

    assert!(
        matches!(error, ModelError::InvalidRequest(message) if message.contains("function tool name"))
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn kimi_role_tool_message_includes_name_from_preceding_assistant_tool_call() {
    let server = ok_server().await;
    let tool_use_id = ToolUseId::new();
    let mut req = base_request("moonshot-v1-8k");
    req.messages = vec![
        message(
            MessageRole::Assistant,
            vec![MessagePart::ToolUse {
                id: tool_use_id,
                name: "search".to_owned(),
                input: json!({ "query": "docs" }),
            }],
        ),
        message(
            MessageRole::Tool,
            vec![MessagePart::ToolResult {
                tool_use_id,
                content: ToolResult::Structured(json!({ "answer": "found" })),
            }],
        ),
    ];

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi request should succeed")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["messages"][1]["role"], "tool");
    assert_eq!(body["messages"][1]["name"], "search");
    assert_eq!(body["messages"][1]["content"], "{\"answer\":\"found\"}");
}

#[tokio::test]
async fn kimi_web_search_tool_result_content_is_json_string() {
    let server = ok_server().await;
    let tool_use_id = ToolUseId::new();
    let mut req = base_request("moonshot-v1-8k");
    req.messages = vec![
        message(
            MessageRole::Assistant,
            vec![MessagePart::ToolUse {
                id: tool_use_id,
                name: "$web_search".to_owned(),
                input: json!({ "query": "docs" }),
            }],
        ),
        message(
            MessageRole::Tool,
            vec![MessagePart::ToolResult {
                tool_use_id,
                content: ToolResult::Structured(json!({
                    "query": "docs",
                    "total_tokens": 12
                })),
            }],
        ),
    ];

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi $web_search tool result should encode")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["messages"][1]["role"], "tool");
    assert_eq!(body["messages"][1]["name"], "$web_search");
    assert_eq!(
        body["messages"][1]["content"],
        "{\"query\":\"docs\",\"total_tokens\":12}"
    );
}

#[tokio::test]
async fn kimi_multimodal_tool_result_encodes_content_array() {
    let server = ok_server().await;
    let tool_use_id = ToolUseId::new();
    let image_ref = blob_ref(1, "image/png");
    let video_ref = blob_ref(2, "video/mp4");
    let store = Arc::new(MemoryBlobStore::new([
        (image_ref.id.to_string(), Bytes::from_static(b"image-bytes")),
        (video_ref.id.to_string(), Bytes::from_static(b"video-bytes")),
    ]));
    let mut ctx = InferContext::for_test();
    ctx.blob_store = Some(store);
    let mut req = base_request("moonshot-v1-8k");
    req.messages = vec![
        message(
            MessageRole::Assistant,
            vec![MessagePart::ToolUse {
                id: tool_use_id,
                name: "render".to_owned(),
                input: json!({}),
            }],
        ),
        message(
            MessageRole::Tool,
            vec![MessagePart::ToolResult {
                tool_use_id,
                content: ToolResult::Mixed(vec![
                    ToolResultPart::Text {
                        text: "hello".to_owned(),
                    },
                    ToolResultPart::Structured {
                        value: json!({ "ok": true }),
                        schema_ref: None,
                    },
                    ToolResultPart::Code {
                        language: "json".to_owned(),
                        text: "{}".to_owned(),
                    },
                    ToolResultPart::Reference {
                        reference_kind: harness_contracts::ReferenceKind::Url {
                            url: "https://example.com".to_owned(),
                        },
                        title: Some("Example".to_owned()),
                        summary: Some("summary".to_owned()),
                    },
                    ToolResultPart::Artifact {
                        artifact_kind: ModelModality::Text,
                        content_type: "text/plain".to_owned(),
                        blob_ref: blob_ref(3, "text/plain"),
                        title: "Artifact".to_owned(),
                        preview: Some("preview".to_owned()),
                    },
                    ToolResultPart::Blob {
                        content_type: "image/png".to_owned(),
                        blob_ref: image_ref,
                        summary: None,
                    },
                    ToolResultPart::Blob {
                        content_type: "video/mp4".to_owned(),
                        blob_ref: video_ref,
                        summary: None,
                    },
                ]),
            }],
        ),
    ];

    provider(&server)
        .infer(req, ctx)
        .await
        .expect("Kimi request should encode multimodal tool result")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    let content = body["messages"][1]["content"]
        .as_array()
        .expect("Kimi tool result content should be an array");
    assert_eq!(content[0], json!({ "type": "text", "text": "hello" }));
    assert_eq!(
        content[1],
        json!({ "type": "text", "text": "{\"ok\":true}" })
    );
    assert_eq!(content[5]["type"], "image_url");
    assert_eq!(
        content[5]["image_url"]["url"],
        format!(
            "data:image/png;base64,{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"image-bytes")
        )
    );
    assert_eq!(content[6]["type"], "video_url");
}

#[tokio::test]
async fn kimi_mixed_non_blob_tool_result_encodes_content_string() {
    let server = ok_server().await;
    let tool_use_id = ToolUseId::new();
    let mut req = base_request("moonshot-v1-8k");
    req.messages = vec![
        message(
            MessageRole::Assistant,
            vec![MessagePart::ToolUse {
                id: tool_use_id,
                name: "summarize".to_owned(),
                input: json!({}),
            }],
        ),
        message(
            MessageRole::Tool,
            vec![MessagePart::ToolResult {
                tool_use_id,
                content: ToolResult::Mixed(vec![
                    ToolResultPart::Artifact {
                        artifact_kind: ModelModality::Text,
                        content_type: "text/plain".to_owned(),
                        blob_ref: blob_ref(3, "text/plain"),
                        title: "Report".to_owned(),
                        preview: Some("preview".to_owned()),
                    },
                    ToolResultPart::Table {
                        headers: vec!["name".to_owned()],
                        rows: vec![vec![json!("kimi")]],
                        caption: Some("models".to_owned()),
                    },
                    ToolResultPart::Progress {
                        stage: "done".to_owned(),
                        ratio: Some(1.0),
                        detail: None,
                    },
                    ToolResultPart::Error {
                        code: "E_TEST".to_owned(),
                        message: "recoverable".to_owned(),
                        retriable: true,
                    },
                ]),
            }],
        ),
    ];

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi mixed non-blob tool result should encode")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["messages"][1]["role"], "tool");
    assert_eq!(body["messages"][1]["name"], "summarize");
    assert!(body["messages"][1]["content"].is_string());
    let content = body["messages"][1]["content"].as_str().unwrap();
    assert!(content.contains("preview"));
    assert!(content.contains("\"caption\":\"models\""));
    assert!(content.contains("\"stage\":\"done\""));
    assert!(content.contains("\"code\":\"E_TEST\""));
}

#[tokio::test]
async fn kimi_rejects_unsupported_tool_result_blob_type() {
    let server = ok_server().await;
    let tool_use_id = ToolUseId::new();
    let blob = blob_ref(1, "application/pdf");
    let store = Arc::new(MemoryBlobStore::new([(
        blob.id.to_string(),
        Bytes::from_static(b"pdf"),
    )]));
    let mut ctx = InferContext::for_test();
    ctx.blob_store = Some(store);
    let mut req = base_request("moonshot-v1-8k");
    req.messages = vec![
        message(
            MessageRole::Assistant,
            vec![MessagePart::ToolUse {
                id: tool_use_id,
                name: "render".to_owned(),
                input: json!({}),
            }],
        ),
        message(
            MessageRole::Tool,
            vec![MessagePart::ToolResult {
                tool_use_id,
                content: ToolResult::Blob {
                    content_type: "application/pdf".to_owned(),
                    blob_ref: blob,
                },
            }],
        ),
    ];

    let error = match provider(&server).infer(req, ctx).await {
        Ok(_) => panic!("unsupported blob type should fail closed"),
        Err(error) => error,
    };

    assert!(
        matches!(error, ModelError::InvalidRequest(message) if message.contains("image or video"))
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn kimi_non_stream_reads_top_level_cached_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "kimi_cached",
            "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 3,
                "total_tokens": 13,
                "cached_tokens": 7
            }
        })))
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(base_request("kimi-k2.6"), InferContext::for_test())
        .await
        .expect("Kimi request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::MessageStart { usage, .. }
            if usage.input_tokens == 10
                && usage.output_tokens == 3
                && usage.cache_read_tokens == 7
    )));
}

#[tokio::test]
async fn kimi_provider_file_reference_encodes_ms_url_for_image_and_video() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.messages = vec![message(
        MessageRole::User,
        vec![
            MessagePart::ProviderFileReference {
                provider_id: "km".to_owned(),
                file_id: "file-image".to_owned(),
                mime_type: "image/png".to_owned(),
            },
            MessagePart::ProviderFileReference {
                provider_id: "km".to_owned(),
                file_id: "file-video".to_owned(),
                mime_type: "video/mp4".to_owned(),
            },
            MessagePart::Text("describe".to_owned()),
        ],
    )];

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi provider file references should encode")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image_url");
    assert_eq!(content[0]["image_url"]["url"], "ms://file-image");
    assert_eq!(content[1]["type"], "video_url");
    assert_eq!(content[1]["video_url"]["url"], "ms://file-video");
}

#[tokio::test]
async fn kimi_partial_mode_appends_final_partial_assistant_message() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.options.kimi_chat = Some(KimiChatOptions {
        partial_assistant: Some(KimiPartialAssistant {
            content: "The answer is".to_owned(),
            name: Some("draft".to_owned()),
        }),
    });

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("Kimi partial mode should encode")
        .collect::<Vec<_>>()
        .await;

    let body = single_request_body(&server).await;
    assert_eq!(body["messages"][1]["role"], "assistant");
    assert_eq!(body["messages"][1]["content"], "The answer is");
    assert_eq!(body["messages"][1]["name"], "draft");
    assert_eq!(body["messages"][1]["partial"], true);
}

#[tokio::test]
async fn kimi_partial_mode_rejects_json_response_format_before_request() {
    let server = ok_server().await;
    let mut req = base_request("kimi-k2.6");
    req.extra = json!({ "response_format": { "type": "json_object" } });
    req.options.kimi_chat = Some(KimiChatOptions {
        partial_assistant: Some(KimiPartialAssistant {
            content: "The answer is".to_owned(),
            name: None,
        }),
    });

    let error = match provider(&server).infer(req, InferContext::for_test()).await {
        Ok(_) => panic!("partial mode should reject JSON mode"),
        Err(error) => error,
    };

    assert!(
        matches!(error, ModelError::InvalidRequest(message) if message.contains("Partial Mode"))
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

async fn ok_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "kimi_ok",
            "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;
    server
}

async fn single_request_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    requests[0].body_json().unwrap()
}

fn provider(server: &MockServer) -> KmProvider {
    KmProvider::from_api_key("provider-key").with_base_url(server.uri())
}

fn base_request(model_id: &str) -> ModelRequest {
    ModelRequest {
        model_id: model_id.to_owned(),
        messages: vec![message(
            MessageRole::User,
            vec![MessagePart::Text("hello".to_owned())],
        )],
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

fn message(role: MessageRole, parts: Vec<MessagePart>) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts,
        created_at: Utc::now(),
    }
}

fn tool_descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
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

fn blob_ref(value: u128, content_type: &str) -> BlobRef {
    BlobRef {
        id: BlobId::from_u128(value),
        size: 1,
        content_hash: [value as u8; 32],
        content_type: Some(content_type.to_owned()),
    }
}

struct MemoryBlobStore {
    bytes: Mutex<HashMap<String, Bytes>>,
}

impl MemoryBlobStore {
    fn new(entries: impl IntoIterator<Item = (String, Bytes)>) -> Self {
        Self {
            bytes: Mutex::new(entries.into_iter().collect()),
        }
    }
}

#[async_trait::async_trait]
impl BlobStore for MemoryBlobStore {
    fn store_id(&self) -> &str {
        "memory"
    }

    async fn put(
        &self,
        _tenant: harness_contracts::TenantId,
        _bytes: Bytes,
        _meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Err(BlobError::Backend("not implemented".to_owned()))
    }

    async fn get(
        &self,
        _tenant: harness_contracts::TenantId,
        blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        let bytes = self
            .bytes
            .lock()
            .await
            .get(&blob.id.to_string())
            .cloned()
            .ok_or(BlobError::NotFound(blob.id))?;
        Ok(Box::pin(stream::once(async move { bytes })))
    }

    async fn head(
        &self,
        _tenant: harness_contracts::TenantId,
        _blob: &BlobRef,
    ) -> Result<Option<BlobMeta>, BlobError> {
        Ok(Some(BlobMeta {
            content_type: None,
            size: 0,
            content_hash: [0; 32],
            created_at: Utc::now(),
            retention: BlobRetention::TenantScoped,
        }))
    }

    async fn delete(
        &self,
        _tenant: harness_contracts::TenantId,
        _blob: &BlobRef,
    ) -> Result<(), BlobError> {
        Ok(())
    }
}
