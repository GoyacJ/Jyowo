#![cfg(feature = "anthropic")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, UsageSnapshot};
use harness_model::{anthropic::AnthropicProvider, *};
use serde_json::{json, Value};
use std::path::Path;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const LIVE_ANTHROPIC_API_KEY_PATH: &str = ".jyowo/live-secrets/anthropic-api-key";

fn message(id: MessageId, text: &str) -> Message {
    Message {
        id,
        role: MessageRole::User,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: Utc::now(),
    }
}

fn request(messages: Vec<Message>, stream: bool) -> ModelRequest {
    ModelRequest {
        model_id: "claude-3-5-sonnet-20241022".to_owned(),
        messages,
        tools: None,
        system: Some("Stable system prompt".to_owned()),
        temperature: None,
        max_tokens: Some(128),
        stream,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Messages,
        extra: Value::Null,
    }
}

fn live_anthropic_api_key_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("model crate should live under workspace crates directory")
        .join(LIVE_ANTHROPIC_API_KEY_PATH)
}

fn read_live_anthropic_api_key() -> Option<String> {
    let path = live_anthropic_api_key_path();
    match read_live_anthropic_api_key_from_file(&path) {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!(
                "skipping live Anthropic prompt-cache test; cannot read {}: {error}",
                path.display()
            );
            None
        }
    }
}

fn read_live_anthropic_api_key_from_file(path: &Path) -> std::io::Result<Option<String>> {
    let secret = match std::fs::read_to_string(path) {
        Ok(secret) => secret,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let secret = secret.trim().to_owned();
    Ok((!secret.is_empty()).then_some(secret))
}

#[tokio::test]
async fn spike_mock_prompt_cache_injection_and_usage_mapping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: message_start\n",
                        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"cache_creation_input_tokens\":7,\"cache_read_input_tokens\":0}}}\n\n",
                        "event: content_block_start\n",
                        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                        "event: content_block_stop\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                        "event: message_delta\n",
                        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":11,\"output_tokens\":2,\"cache_creation_input_tokens\":7,\"cache_read_input_tokens\":5}}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let first_id = MessageId::new();
    let second_id = MessageId::new();
    let mut req = request(
        vec![message(first_id, "first"), message(second_id, "second")],
        true,
    );
    req.cache_breakpoints.push(CacheBreakpoint {
        after_message_id: second_id,
        reason: BreakpointReason::RecentMessage,
    });

    let events = AnthropicProvider::from_api_key("test-key")
        .with_base_url(server.uri())
        .infer(req, InferContext::for_test())
        .await
        .expect("mock spike request should start")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(count_cache_controls(&body), 2);
    assert_eq!(
        body["system"][0]["cache_control"],
        json!({ "type": "ephemeral" })
    );
    assert_eq!(
        body["messages"][1]["content"][0]["cache_control"],
        json!({ "type": "ephemeral" })
    );

    assert!(events.contains(&ModelStreamEvent::MessageStart {
        message_id: "msg_1".to_owned(),
        usage: UsageSnapshot {
            input_tokens: 11,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 7,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(harness_contracts::StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 11,
            output_tokens: 2,
            cache_read_tokens: 5,
            cache_write_tokens: 7,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
}

#[test]
fn live_anthropic_secret_file_is_trimmed() {
    let path = std::env::temp_dir().join(format!("jyowo-live-anthropic-key-{}", MessageId::new()));
    std::fs::write(&path, "  test-secret\n").expect("secret fixture should be writable");

    let api_key = read_live_anthropic_api_key_from_file(&path)
        .expect("secret fixture should be readable")
        .expect("secret fixture should be present");

    std::fs::remove_file(&path).expect("secret fixture should be removed");
    assert_eq!(api_key, "test-secret");
}

#[ignore = "manual live Anthropic prompt-cache validation; requires local secret file"]
#[tokio::test]
async fn live_anthropic_prompt_cache_reads_after_warmup() {
    let Some(api_key) = read_live_anthropic_api_key() else {
        return;
    };
    let provider = AnthropicProvider::from_api_key(api_key);
    let anchor_id = MessageId::new();
    let anchor = message(anchor_id, "Use this stable anchor in every turn.");
    let mut observed_cache_read = 0;

    for turn in 0..3 {
        let mut req = request(
            vec![
                anchor.clone(),
                message(
                    MessageId::new(),
                    &format!("turn {turn}: answer with one word"),
                ),
            ],
            true,
        );
        req.cache_breakpoints.push(CacheBreakpoint {
            after_message_id: anchor_id,
            reason: BreakpointReason::RecentMessage,
        });

        let events = provider
            .infer(req, InferContext::for_test())
            .await
            .expect("live Anthropic request should start")
            .collect::<Vec<_>>()
            .await;
        observed_cache_read = observed_cache_read.max(max_cache_read(&events));
    }

    println!("live Anthropic prompt-cache observed_cache_read_tokens={observed_cache_read}");
    assert!(observed_cache_read > 0);
}

fn count_cache_controls(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            usize::from(map.contains_key("cache_control"))
                + map.values().map(count_cache_controls).sum::<usize>()
        }
        Value::Array(values) => values.iter().map(count_cache_controls).sum(),
        _ => 0,
    }
}

fn max_cache_read(events: &[ModelStreamEvent]) -> u64 {
    events
        .iter()
        .filter_map(|event| match event {
            ModelStreamEvent::MessageStart { usage, .. } => Some(usage.cache_read_tokens),
            ModelStreamEvent::MessageDelta { usage_delta, .. } => {
                Some(usage_delta.cache_read_tokens)
            }
            _ => None,
        })
        .max()
        .unwrap_or_default()
}
