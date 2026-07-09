#![cfg(feature = "minimax")]

use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures::{stream, stream::BoxStream, StreamExt};
use harness_contracts::{
    BlobError, BlobId, BlobMeta, BlobRef, BlobStore, Message, MessageId, MessagePart, MessageRole,
    RunId, SessionId, TenantId, ToolResult, ToolUseId,
};
use harness_model::{MinimaxProvider, *};
use harness_provider_state::{
    ProviderContinuationKind, ProviderContinuationRecord, ProviderContinuationScope,
};
use serde_json::{json, Map, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn minimax_dialect_does_not_emit_or_replay_deepseek_private_payload() {
    let private_key = private_reasoning_key();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(minimax_stream_body(&private_key), "text/event-stream"),
        )
        .mount(&server)
        .await;
    let assistant_id = MessageId::new();
    let mut req = assistant_tool_replay_request(assistant_id, true);
    req.provider_context.continuations = vec![record(assistant_id)];

    let events = provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("MiniMax should ignore private continuation records")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("visible".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "call_1".to_owned(),
            name: "search".to_owned(),
        },
    }));
    let continuation_payload = events
        .iter()
        .find_map(|event| match event {
            ModelStreamEvent::ProviderContinuationDelta { payload, .. } => Some(payload),
            _ => None,
        })
        .expect("MiniMax reasoning_content should be captured as MiniMax continuation");
    assert_eq!(
        continuation_payload["format"],
        json!("minimax.reasoning_details.v1")
    );
    assert_eq!(continuation_payload["reasoningContent"], json!("ignored"));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert!(body["messages"][0].get(&private_key).is_none());
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["id"],
        assistant_tool_id().to_string()
    );
}

#[tokio::test]
async fn minimax_tool_replay_request_uses_real_codec_without_private_continuation() {
    let private_key = private_reasoning_key();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "mm_1",
            "choices": [{
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;

    provider(&server)
        .infer(
            assistant_tool_replay_request(MessageId::new(), false),
            InferContext::for_test(),
        )
        .await
        .expect("MiniMax should not require provider continuation records")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["messages"][0]["role"], "assistant");
    assert_eq!(body["messages"][0]["content"], Value::Null);
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["id"],
        assistant_tool_id().to_string()
    );
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["function"]["arguments"],
        "{\"query\":\"docs\"}"
    );
    assert!(body["messages"][0].get(&private_key).is_none());
}

#[tokio::test]
async fn minimax_m3_runtime_modalities_match_openai_content_part_encoding() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "mm_1",
            "choices": [{
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;

    let provider = provider(&server);
    let m3 = provider
        .supported_models()
        .into_iter()
        .find(|model| model.model_id == "MiniMax-M3")
        .expect("MiniMax-M3 should be listed");
    assert_eq!(
        m3.conversation_capability.input_modalities,
        vec![
            ModelModality::Text,
            ModelModality::Image,
            ModelModality::Video,
        ]
    );

    let mut ctx = InferContext::for_test();
    ctx.blob_store = Some(Arc::new(StaticBlobStore));
    provider
        .infer(multimodal_m3_request(), ctx)
        .await
        .expect("MiniMax-M3 multimodal request should encode")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], "MiniMax-M3");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][0]["text"], "describe");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,bWVkaWE="
    );
    assert_eq!(body["messages"][0]["content"][2]["type"], "video_url");
    assert_eq!(
        body["messages"][0]["content"][2]["video_url"]["url"],
        "data:video/mp4;base64,bWVkaWE="
    );
}

fn provider(server: &MockServer) -> MinimaxProvider {
    MinimaxProvider::from_api_key("provider-key").with_base_url(server.uri())
}

fn multimodal_m3_request() -> ModelRequest {
    ModelRequest {
        model_id: "MiniMax-M3".to_owned(),
        messages: vec![message(
            MessageRole::User,
            MessageId::new(),
            vec![
                MessagePart::Text("describe".to_owned()),
                MessagePart::Image {
                    mime_type: "image/png".to_owned(),
                    blob_ref: blob_ref("image/png"),
                },
                MessagePart::Video {
                    mime_type: "video/mp4".to_owned(),
                    blob_ref: blob_ref("video/mp4"),
                },
            ],
        )],
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

fn assistant_tool_replay_request(assistant_id: MessageId, stream: bool) -> ModelRequest {
    let tool_use_id = assistant_tool_id();
    ModelRequest {
        model_id: "MiniMax-M2.7".to_owned(),
        messages: vec![
            message(
                MessageRole::Assistant,
                assistant_id,
                vec![MessagePart::ToolUse {
                    id: tool_use_id,
                    name: "search".to_owned(),
                    input: json!({ "query": "docs" }),
                }],
            ),
            message(
                MessageRole::Tool,
                MessageId::new(),
                vec![MessagePart::ToolResult {
                    tool_use_id,
                    content: ToolResult::Structured(json!({ "answer": "found" })),
                }],
            ),
        ],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(64),
        stream,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        provider_context: ProviderRequestContext::default(),
    }
}

fn message(role: MessageRole, id: MessageId, parts: Vec<MessagePart>) -> Message {
    Message {
        id,
        role,
        parts,
        created_at: Utc::now(),
    }
}

fn record(message_id: MessageId) -> ProviderContinuationRecord {
    ProviderContinuationRecord {
        provider_id: "minimax".to_owned(),
        model_config_id: None,
        protocol: ModelProtocol::ChatCompletions,
        dialect: "minimax".to_owned(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        producing_run_id: RunId::new(),
        message_id,
        scope: ProviderContinuationScope::Conversation,
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({
            "format": "deepseek.reasoning-content.v1",
            "reasoningContent": "must stay private",
        }),
        created_at: Utc::now(),
    }
}

fn minimax_stream_body(private_key: &str) -> String {
    let mut delta = Map::new();
    delta.insert(private_key.to_owned(), Value::String("ignored".to_owned()));
    delta.insert("content".to_owned(), Value::String("visible".to_owned()));
    delta.insert(
        "tool_calls".to_owned(),
        json!([{
            "index": 0,
            "id": "call_1",
            "function": {
                "name": "search",
                "arguments": "{\"query\":\"docs\"}"
            }
        }]),
    );
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({
            "id": "mm_1",
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 2
            }
        })
    )
}

fn private_reasoning_key() -> String {
    ["reasoning", "_", "content"].concat()
}

fn assistant_tool_id() -> ToolUseId {
    ToolUseId::from_u128(42)
}

fn blob_ref(content_type: &str) -> BlobRef {
    BlobRef {
        id: BlobId::new(),
        size: 5,
        content_hash: [7; 32],
        content_type: Some(content_type.to_owned()),
    }
}

struct StaticBlobStore;

#[async_trait::async_trait]
impl BlobStore for StaticBlobStore {
    fn store_id(&self) -> &str {
        "static"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Ok(BlobRef {
            id: BlobId::new(),
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        Ok(Box::pin(stream::once(async {
            Bytes::from_static(b"media")
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
