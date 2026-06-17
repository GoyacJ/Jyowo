#![cfg(feature = "bedrock")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError};
use harness_model::{bedrock::BedrockProvider, *};
use serde_json::Value;

fn request() -> ModelRequest {
    ModelRequest {
        model_id: "anthropic.claude-3-5-sonnet-20241022-v2:0".to_owned(),
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
        stream: true,
        cache_breakpoints: Vec::new(),
        api_mode: ApiMode::Messages,
        extra: Value::Null,
    }
}

#[test]
fn bedrock_provider_metadata_is_stable() {
    let provider = BedrockProvider::from_events(vec![ModelStreamEvent::MessageStop]);

    assert_eq!(provider.provider_id(), "bedrock");
    assert!(provider.supports_tools());
    assert!(provider.supports_vision());
    assert!(provider.supports_thinking());
    assert!(provider.supported_models().iter().any(|model| {
        model.provider_id == "bedrock"
            && model.model_id == "anthropic.claude-3-5-sonnet-20241022-v2:0"
            && model.capabilities.supports_tools
            && model.capabilities.supports_thinking
    }));
}

#[tokio::test]
async fn bedrock_uses_transport_without_aws_environment_in_tests() {
    let events = BedrockProvider::from_events(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("hi".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])
    .infer(request(), InferContext::for_test())
    .await
    .expect("fake transport should stream")
    .collect::<Vec<_>>()
    .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("hi".to_owned()),
    }));
}

#[tokio::test]
async fn bedrock_rejects_non_messages_mode() {
    let mut req = request();
    req.api_mode = ApiMode::ChatCompletions;

    let error = match BedrockProvider::from_events(vec![ModelStreamEvent::MessageStop])
        .infer(req, InferContext::for_test())
        .await
    {
        Ok(_) => panic!("wrong mode should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, ModelError::InvalidRequest(_)));
}
