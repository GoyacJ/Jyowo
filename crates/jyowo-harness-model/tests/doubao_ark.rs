#![cfg(feature = "doubao")]

use chrono::NaiveDate;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelProtocol};
use harness_model::*;
use serde_json::{json, Value};
use wiremock::{
    matchers::{body_partial_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(protocol: ModelProtocol) -> ModelRequest {
    ModelRequest {
        model_id: "doubao-seed-2-1-pro-260628".to_owned(),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text("hello".to_owned())],
            created_at: chrono::Utc::now(),
        }],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(64),
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol,
        extra: Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: ProviderRequestContext::default(),
    }
}

#[test]
fn doubao_catalog_uses_official_seed_snapshot() {
    let entries = model_catalog_entries();
    let doubao: Vec<_> = entries
        .iter()
        .filter(|entry| entry.provider_id == "doubao")
        .collect();

    assert!(doubao.len() >= 20);
    assert!(doubao
        .iter()
        .any(|entry| entry.model_id == "doubao-seed-2-1-pro-260628"));
    assert!(doubao
        .iter()
        .any(|entry| entry.model_id == "doubao-seedream-5-0-pro-260628"));
    assert!(doubao
        .iter()
        .any(|entry| entry.model_id == "doubao-seedance-2-0-260128"));
    assert!(doubao
        .iter()
        .all(|entry| entry.verified_at == NaiveDate::from_ymd_opt(2026, 7, 8).unwrap()));
    assert!(!doubao
        .iter()
        .any(|entry| entry.model_id.starts_with("glm-") || entry.model_id.starts_with("deepseek")));
}

#[test]
fn doubao_seed_21_pro_exposes_reasoning_and_ark_parameters() {
    let descriptor = DoubaoProvider::from_api_key("provider-key")
        .supported_models()
        .into_iter()
        .find(|model| model.model_id == "doubao-seed-2-1-pro-260628")
        .expect("official Seed 2.1 Pro model should be in catalog");

    assert_eq!(descriptor.protocol, ModelProtocol::Responses);
    assert!(descriptor.conversation_capability.reasoning);
    assert!(descriptor.conversation_capability.tool_calling);
    assert!(descriptor.conversation_capability.structured_output);
    assert!(descriptor
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "thinking"));
    assert!(descriptor
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "reasoning_effort"));
    assert!(descriptor
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "service_tier"));
    assert!(descriptor
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "max_completion_tokens"));
}

#[test]
fn doubao_runtime_models_exclude_service_only_outputs() {
    let models = DoubaoProvider::from_api_key("provider-key").supported_models();

    assert!(models
        .iter()
        .any(|model| model.model_id == "doubao-seed-2-1-pro-260628"));
    assert!(!models
        .iter()
        .any(|model| model.model_id == "doubao-seedream-5-0-pro-260628"));
    assert!(!models
        .iter()
        .any(|model| model.model_id == "doubao-seedance-2-0-260128"));
    assert!(!models
        .iter()
        .any(|model| model.model_id == "doubao-seed3d-2-0-260328"));
    assert!(!models
        .iter()
        .any(|model| model.model_id == "doubao-embedding-vision-251215"));
}

#[test]
fn doubao_inventory_marks_service_only_outputs_unsupported() {
    let doubao = provider_inventory_entries()
        .into_iter()
        .find(|entry| entry.provider_id == "doubao")
        .expect("doubao inventory should exist");

    for model_id in [
        "doubao-seedream-5-0-pro-260628",
        "doubao-seedance-2-0-260128",
        "doubao-seed3d-2-0-260328",
        "doubao-embedding-vision-251215",
    ] {
        assert!(
            doubao.models.iter().any(|model| {
                model.model_id == model_id
                    && matches!(model.runtime_status, ModelRuntimeStatus::Unsupported { .. })
            }),
            "{model_id} should be inventory-only"
        );
    }
}

#[test]
fn doubao_supported_parameters_follow_model_capabilities() {
    let models = DoubaoProvider::from_api_key("provider-key").supported_models();
    let character = models
        .iter()
        .find(|model| model.model_id == "doubao-seed-character-260628")
        .expect("character model should be runnable");

    assert!(!character
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "thinking"));
    assert!(!character
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "reasoning_effort"));
    assert!(!character
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "response_format"));
    assert!(character
        .supported_parameters
        .iter()
        .any(|parameter| parameter == "tools"));
}

#[tokio::test]
async fn doubao_provider_routes_responses_protocol_to_ark_responses_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_partial_json(json!({
            "model": "doubao-seed-2-1-pro-260628",
            "max_output_tokens": 64
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1",
            "object": "response",
            "status": "completed",
            "output": [{
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "hi"
                }]
            }],
            "usage": {"input_tokens": 3, "output_tokens": 1}
        })))
        .mount(&server)
        .await;

    let provider = DoubaoProvider::from_api_key("provider-key").with_base_url(server.uri());
    let events: Vec<_> = provider
        .infer(request(ModelProtocol::Responses), InferContext::for_test())
        .await
        .expect("responses request should succeed")
        .collect()
        .await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::Text(text),
            ..
        } if text == "hi"
    )));
}

#[tokio::test]
async fn doubao_chat_response_maps_reasoning_content_to_thinking() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_partial_json(json!({
            "model": "doubao-seed-2-1-pro-260628",
            "max_completion_tokens": 64
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chat_1",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "reasoning_content": "private plan",
                    "content": "answer"
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 3, "completion_tokens": 2}
        })))
        .mount(&server)
        .await;

    let provider = DoubaoProvider::from_api_key("provider-key").with_base_url(server.uri());
    let events: Vec<_> = provider
        .infer(
            request(ModelProtocol::ChatCompletions),
            InferContext::for_test(),
        )
        .await
        .expect("chat request should succeed")
        .collect()
        .await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ContentBlockDelta {
            delta: ContentDelta::Thinking(thinking),
            ..
        } if thinking.text.as_deref() == Some("private plan")
    )));
}
