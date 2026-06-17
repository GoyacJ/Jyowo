#![cfg(feature = "mock")]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError, TenantId};
use harness_model::*;
use secrecy::ExposeSecret;
use serde_json::Value;

fn message(text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: Utc::now(),
    }
}

fn request(text: &str) -> ModelRequest {
    ModelRequest {
        model_id: "mock-model".to_owned(),
        messages: vec![message(text)],
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

fn credential_key(label: &str) -> CredentialKey {
    CredentialKey {
        tenant_id: TenantId::SINGLE,
        provider_id: "mock".to_owned(),
        key_label: label.to_owned(),
    }
}

#[tokio::test]
async fn mock_provider_defaults_are_contract_friendly() {
    let provider = MockProvider::default();

    assert_eq!(provider.provider_id(), "mock");
    assert_eq!(provider.supported_models().len(), 1);
    assert_eq!(provider.supported_models()[0].model_id, "mock-model");
    assert_eq!(provider.health().await, HealthStatus::Healthy);

    let events = provider
        .infer(request("hello"), InferContext::for_test())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(events, vec![ModelStreamEvent::MessageStop]);
}

#[tokio::test]
async fn mock_provider_replays_events_and_records_requests() {
    let provider = MockProvider::default().with_events(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("hello".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ]);

    let first = provider
        .infer(request("first"), InferContext::for_test())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    let second = provider
        .infer(request("second"), InferContext::for_test())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(first, second);
    assert_eq!(provider.requests().await.len(), 2);
    assert_eq!(
        provider.requests().await[0].messages[0].parts[0],
        MessagePart::Text("first".to_owned())
    );
    assert_eq!(
        provider.requests().await[1].messages[0].parts[0],
        MessagePart::Text("second".to_owned())
    );
}

#[tokio::test]
async fn scripted_provider_consumes_stream_error_and_exhaustion_in_order() {
    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
        ScriptedResponse::Error(ModelError::ProviderUnavailable("offline".to_owned())),
    ]);

    let events = provider
        .infer(request("stream"), InferContext::for_test())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(events, vec![ModelStreamEvent::MessageStop]);

    let error = provider
        .infer(request("error"), InferContext::for_test())
        .await
        .err()
        .expect("scripted error response should fail");
    assert_eq!(error, ModelError::ProviderUnavailable("offline".to_owned()));

    let exhausted = provider
        .infer(request("exhausted"), InferContext::for_test())
        .await
        .err()
        .expect("exhausted scripted provider should fail");
    assert!(matches!(exhausted, ModelError::InvalidRequest(_)));
}

#[tokio::test]
async fn scripted_provider_waits_for_cancel() {
    let provider = ScriptedProvider::new(vec![ScriptedResponse::WaitForCancel]);
    let ctx = InferContext::for_test();
    let cancel = ctx.cancel.clone();

    let task = tokio::spawn(async move { provider.infer(request("cancel"), ctx).await });
    cancel.cancel();

    let error = task
        .await
        .unwrap()
        .err()
        .expect("cancelled scripted provider should fail");
    assert_eq!(error, ModelError::Cancelled);
}

#[tokio::test]
async fn mock_credential_source_is_memory_only_and_records_rotation() {
    let source = MockCredentialSource::default();
    let key = credential_key("primary");
    source.insert_secret(key.clone(), "secret-value").await;

    let value = source.fetch(key.clone()).await.unwrap();
    assert_eq!(value.secret.expose_secret(), "secret-value");

    source.rotate(key.clone()).await.unwrap();
    assert_eq!(source.rotated_keys().await, vec![key.clone()]);

    let missing = credential_key("missing");
    assert!(matches!(
        source.fetch(missing.clone()).await.unwrap_err(),
        CredentialError::Missing(_)
    ));
    assert!(matches!(
        source.rotate(missing).await.unwrap_err(),
        CredentialError::Missing(_)
    ));
}
