#![cfg(feature = "deepseek")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    Message, MessageId, MessagePart, MessageRole, ModelError, RunId, SessionId, TenantId,
    ToolResult, ToolUseId,
};
use harness_model::{DeepSeekProvider, *};
use harness_provider_state::{
    ProviderContinuationKind, ProviderContinuationRecord, ProviderContinuationScope,
};
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn deepseek_stream_captures_reasoning_content_as_private_continuation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"ds_1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think \"},\"finish_reason\":null}]}\n\n",
                        "data: {\"id\":\"ds_1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"again\",\"content\":\"answer\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(user_request(true), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("answer".to_owned()),
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload,
        } if payload == &deepseek_payload("think again")
    )));
}

#[tokio::test]
async fn deepseek_non_stream_captures_reasoning_content_as_private_continuation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "ds_1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "private chain",
                    "content": "visible"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 2
            }
        })))
        .mount(&server)
        .await;

    let events = provider(&server)
        .infer(user_request(false), InferContext::for_test())
        .await
        .expect("non-stream request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("visible".to_owned()),
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload,
        } if payload == &deepseek_payload("private chain")
    )));
}

#[tokio::test]
async fn deepseek_second_request_replays_reasoning_content_for_assistant_tool_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "ds_2",
            "choices": [{
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;
    let assistant_id = MessageId::new();
    let mut req = assistant_tool_replay_request(assistant_id);
    req.provider_context.continuations =
        vec![record(assistant_id, deepseek_payload("saved trace"))];

    provider(&server)
        .infer(req, InferContext::for_test())
        .await
        .expect("request should encode continuation")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        Value::String("saved trace".to_owned())
    );
}

#[tokio::test]
async fn deepseek_missing_required_reasoning_continuation_fails_closed_before_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let error = match provider(&server)
        .infer(
            assistant_tool_replay_request(MessageId::new()),
            InferContext::for_test(),
        )
        .await
    {
        Ok(_) => panic!("missing continuation must fail before dispatch"),
        Err(error) => error,
    };

    assert!(
        matches!(error, ModelError::InvalidRequest(message) if message.contains("provider continuation"))
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn deepseek_invalid_reasoning_continuation_fails_closed_before_request() {
    let assistant_id = MessageId::new();
    let invalid_cases = vec![
        record(MessageId::new(), deepseek_payload("saved trace")),
        record_with_kind(
            assistant_id,
            ProviderContinuationKind::ToolReplay,
            deepseek_payload("saved trace"),
        ),
        record(
            assistant_id,
            json!({
                "format": "deepseek.reasoning-content.v1",
                "reasoningContent": "saved trace",
            }),
        ),
        record(
            assistant_id,
            json!({
                "format": "deepseek.reasoning_content.v1",
            }),
        ),
        record(assistant_id, deepseek_payload("")),
    ];

    for invalid_record in invalid_cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let mut req = assistant_tool_replay_request(assistant_id);
        req.provider_context.continuations = vec![invalid_record];

        let error = match provider(&server).infer(req, InferContext::for_test()).await {
            Ok(_) => panic!("invalid continuation must fail before dispatch"),
            Err(error) => error,
        };

        assert!(
            matches!(error, ModelError::InvalidRequest(message) if message.contains("provider continuation"))
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

fn provider(server: &MockServer) -> DeepSeekProvider {
    DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri())
}

fn user_request(stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "deepseek-v4-flash".to_owned(),
        messages: vec![message(
            MessageRole::User,
            MessageId::new(),
            vec![MessagePart::Text("hello".to_owned())],
        )],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(64),
        stream,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: ProviderRequestContext::default(),
    }
}

fn assistant_tool_replay_request(assistant_id: MessageId) -> ModelRequest {
    let tool_use_id = ToolUseId::new();
    let mut req = user_request(false);
    req.messages = vec![
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
    ];
    req
}

fn message(role: MessageRole, id: MessageId, parts: Vec<MessagePart>) -> Message {
    Message {
        id,
        role,
        parts,
        created_at: Utc::now(),
    }
}

fn record(message_id: MessageId, payload: Value) -> ProviderContinuationRecord {
    record_with_kind(
        message_id,
        ProviderContinuationKind::ReasoningReplay,
        payload,
    )
}

fn record_with_kind(
    message_id: MessageId,
    kind: ProviderContinuationKind,
    payload: Value,
) -> ProviderContinuationRecord {
    ProviderContinuationRecord {
        provider_id: "deepseek".to_owned(),
        model_config_id: None,
        protocol: ModelProtocol::ChatCompletions,
        dialect: "deepseek".to_owned(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        producing_run_id: RunId::new(),
        message_id,
        scope: ProviderContinuationScope::Conversation,
        kind,
        payload,
        created_at: Utc::now(),
    }
}

fn deepseek_payload(reasoning: &str) -> Value {
    json!({
        "format": "deepseek.reasoning_content.v1",
        "reasoningContent": reasoning,
    })
}
