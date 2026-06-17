#![cfg(feature = "codex")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError};
use harness_model::{codex::CodexResponsesProvider, *};
use serde_json::Value;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "gpt-5.4-codex".to_owned(),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text("build".to_owned())],
            created_at: Utc::now(),
        }],
        tools: None,
        system: Some("Think carefully.".to_owned()),
        temperature: None,
        max_tokens: Some(128),
        stream,
        cache_breakpoints: Vec::new(),
        api_mode: ApiMode::Responses,
        extra: Value::Null,
    }
}

#[test]
fn codex_provider_metadata_is_stable() {
    let provider = CodexResponsesProvider::from_api_key("test-key");

    assert_eq!(provider.provider_id(), "codex");
    assert_eq!(CODEX_API_KEY_ENV, "CODEX_API_KEY");
    assert!(provider.supports_tools());
    assert!(provider.supports_thinking());
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == "gpt-5.4-codex"));
}

#[tokio::test]
async fn codex_streams_responses_text_reasoning_tool_and_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: response.output_text.delta\n",
                        "data: {\"delta\":\"done\"}\n\n",
                        "event: response.reasoning_text.delta\n",
                        "data: {\"delta\":\"plan\"}\n\n",
                        "event: response.output_item.added\n",
                        "data: {\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"search\"}}\n\n",
                        "event: response.function_call_arguments.delta\n",
                        "data: {\"delta\":\"{\\\"query\\\":\\\"docs\\\"}\"}\n\n",
                        "event: response.completed\n",
                        "data: {\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":9,\"output_tokens\":4,\"input_tokens_details\":{\"cached_tokens\":3}}}}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let events = CodexResponsesProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(request(true), InferContext::for_test())
        .await
        .expect("stream should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("done".to_owned()),
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::Thinking(ThinkingDelta { text: Some(text), .. }),
            ..
        } if text == "plan"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::ToolUseStart { id, name },
            ..
        } if id == "call_1" && name == "search"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::MessageDelta {
            usage_delta,
            ..
        } if usage_delta.input_tokens == 9
            && usage_delta.output_tokens == 4
            && usage_delta.cache_read_tokens == 3
    )));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], "gpt-5.4-codex");
    assert_eq!(body["stream"], true);
    assert_eq!(body["input"][0]["role"], "system");
    assert_eq!(body["input"][1]["content"], "build");
}

#[tokio::test]
async fn codex_rejects_chat_completions_mode() {
    let mut req = request(false);
    req.api_mode = ApiMode::ChatCompletions;

    let error = match CodexResponsesProvider::from_api_key("test-key")
        .infer(req, InferContext::for_test())
        .await
    {
        Ok(_) => panic!("wrong mode should fail before transport"),
        Err(error) => error,
    };

    assert!(matches!(error, ModelError::InvalidRequest(_)));
}
