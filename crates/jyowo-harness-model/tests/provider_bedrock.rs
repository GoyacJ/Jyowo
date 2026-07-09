#![cfg(all(feature = "bedrock", feature = "testing"))]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    BlobId, BlobRef, Message, MessageId, MessagePart, MessageRole, ModelError,
};
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
        protocol: ModelProtocol::Messages,
        extra: Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

#[test]
fn bedrock_provider_metadata_is_stable() {
    let provider = BedrockProvider::from_events(vec![ModelStreamEvent::MessageStop]);

    assert_eq!(provider.provider_id(), "bedrock");
    assert!(provider.supported_models().iter().any(|model| {
        model.provider_id == "bedrock"
            && model.model_id == "anthropic.claude-3-5-sonnet-20241022-v2:0"
            && model.conversation_capability.tool_calling
            && model.conversation_capability.input_modalities == vec![ModelModality::Text]
            && model.conversation_capability.reasoning
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
    req.protocol = ModelProtocol::ChatCompletions;

    let error = match BedrockProvider::from_events(vec![ModelStreamEvent::MessageStop])
        .infer(req, InferContext::for_test())
        .await
    {
        Ok(_) => panic!("wrong mode should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, ModelError::InvalidRequest(_)));
}

#[tokio::test]
async fn bedrock_rejects_image_parts_until_blob_materialization_exists() {
    let mut req = request();
    req.messages[0].parts = vec![MessagePart::Image {
        mime_type: "image/png".to_owned(),
        blob_ref: BlobRef {
            id: BlobId::new(),
            size: 12,
            content_hash: [7; 32],
            content_type: Some("image/png".to_owned()),
        },
    }];

    let error = match BedrockProvider::from_events(vec![ModelStreamEvent::MessageStop])
        .infer(req, InferContext::for_test())
        .await
    {
        Ok(_) => panic!("image parts should fail"),
        Err(error) => error,
    };

    assert!(
        matches!(error, ModelError::InvalidRequest(message) if message.contains("image blocks require blob materialization"))
    );
}
