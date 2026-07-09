#![cfg(any(
    feature = "deepseek",
    feature = "minimax",
    feature = "qwen",
    feature = "doubao",
    feature = "zhipu",
    feature = "km"
))]

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    Message, MessageId, MessagePart, MessageRole, ModelError, StopReason, UsageSnapshot,
};
use harness_model::*;
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(model_id: &str) -> ModelRequest {
    ModelRequest {
        model_id: model_id.to_owned(),
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
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

async fn assert_streaming_provider<P>(
    provider: P,
    model_id: &str,
    expected_path: &str,
    server: &MockServer,
) where
    P: ModelProvider,
{
    Mock::given(method("POST"))
        .and(path(expected_path))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"chat_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(server)
        .await;

    if matches!(provider.provider_id(), "deepseek" | "minimax") {
        assert_eq!(
            provider.prompt_cache_style(),
            PromptCacheStyle::OpenAi { auto: true }
        );
    } else {
        assert_eq!(provider.prompt_cache_style(), PromptCacheStyle::None);
    }
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == model_id
            && model.conversation_capability.tool_calling
            && !model
                .conversation_capability
                .input_modalities
                .contains(&ModelModality::Image)
            && (provider.provider_id() == "deepseek" || !model.conversation_capability.reasoning)));

    let events = provider
        .infer(request(model_id), InferContext::for_test())
        .await
        .expect("stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("hi".to_owned()),
    }));
    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 3,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
    assert!(events.contains(&ModelStreamEvent::MessageStop));

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], model_id);
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
    if provider.provider_id() == "minimax" {
        assert_eq!(body["max_completion_tokens"], 64);
        assert!(body.get("max_tokens").is_none());
    } else {
        assert_eq!(body["max_tokens"], 64);
    }
}

macro_rules! provider_test {
    ($cfg:literal, $test_name:ident, $provider:ident, $provider_id:literal, $env:path, $env_value:literal, $model:literal, $path:literal) => {
        #[cfg(feature = $cfg)]
        #[tokio::test]
        async fn $test_name() {
            let server = MockServer::start().await;
            let provider = $provider::from_api_key("provider-key").with_base_url(server.uri());

            assert_eq!(provider.provider_id(), $provider_id);
            assert_eq!($env, $env_value);

            assert_streaming_provider(provider, $model, $path, &server).await;
        }
    };
}

provider_test!(
    "deepseek",
    provider_deepseek_streams_chat_completions,
    DeepSeekProvider,
    "deepseek",
    DEEPSEEK_API_KEY_ENV,
    "DEEPSEEK_API_KEY",
    "deepseek-v4-flash",
    "/chat/completions"
);

#[cfg(feature = "deepseek")]
#[test]
fn provider_deepseek_catalog_matches_official_capabilities_and_pricing() {
    let provider = DeepSeekProvider::from_api_key("provider-key");
    let models = provider.supported_models();
    let flash = models
        .iter()
        .find(|model| model.model_id == "deepseek-v4-flash")
        .expect("DeepSeek V4 Flash should be listed");
    assert!(flash.conversation_capability.reasoning);
    assert!(flash.conversation_capability.prompt_cache);
    assert!(flash.conversation_capability.structured_output);
    let flash_pricing = flash.pricing.as_ref().expect("Flash pricing should be set");
    assert_eq!(flash_pricing.input_per_million.to_string(), "0.14");
    assert_eq!(
        flash_pricing.cache_read_per_million.unwrap().to_string(),
        "0.0028"
    );
    assert_eq!(flash_pricing.output_per_million.to_string(), "0.28");

    let pro = models
        .iter()
        .find(|model| model.model_id == "deepseek-v4-pro")
        .expect("DeepSeek V4 Pro should be listed");
    assert!(pro.conversation_capability.reasoning);
    assert!(pro.conversation_capability.prompt_cache);
    assert!(pro.conversation_capability.structured_output);
    let pro_pricing = pro.pricing.as_ref().expect("Pro pricing should be set");
    assert_eq!(pro_pricing.input_per_million.to_string(), "0.435");
    assert_eq!(
        pro_pricing.cache_read_per_million.unwrap().to_string(),
        "0.003625"
    );
    assert_eq!(pro_pricing.output_per_million.to_string(), "0.87");
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_v1_base_url_naturally_extends_chat_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(deepseek_ok_json())
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key")
        .with_base_url(format!("{}/v1", server.uri()));

    provider
        .infer(deepseek_request(false), InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_default_thinking_removes_sampling_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(deepseek_ok_json())
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri());
    let mut req = deepseek_request(false);
    req.temperature = Some(0.7);
    req.extra = json!({
        "top_p": 0.8,
        "presence_penalty": 0.1,
        "frequency_penalty": 0.2,
        "reasoning_effort": "high"
    });

    provider
        .infer(req, InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert!(body.get("thinking").is_none());
    assert!(body.get("temperature").is_none());
    assert!(body.get("top_p").is_none());
    assert!(body.get("presence_penalty").is_none());
    assert!(body.get("frequency_penalty").is_none());
    assert_eq!(body["reasoning_effort"], "high");
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_thinking_disabled_keeps_sampling_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(deepseek_ok_json())
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri());
    let mut req = deepseek_request(false);
    req.temperature = Some(0.7);
    req.extra = json!({
        "thinking": { "disabled": true },
        "top_p": 0.8,
        "presence_penalty": 0.1,
        "frequency_penalty": 0.2
    });

    provider
        .infer(req, InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    let requests = server.received_requests().await.unwrap();
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["thinking"]["disabled"], true);
    assert!((body["temperature"].as_f64().unwrap() - 0.7).abs() < 0.0001);
    assert_eq!(body["top_p"], json!(0.8));
    assert_eq!(body["presence_penalty"], json!(0.1));
    assert_eq!(body["frequency_penalty"], json!(0.2));
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_non_stream_maps_cache_hit_miss_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "ds_1",
            "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "prompt_cache_miss_tokens": 3,
                "prompt_cache_hit_tokens": 7,
                "completion_tokens": 2
            }
        })))
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri());

    let events = provider
        .infer(deepseek_request(false), InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 3,
            output_tokens: 2,
            cache_read_tokens: 7,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_stream_maps_cache_hit_miss_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"ds_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"prompt_cache_miss_tokens\":3,\"prompt_cache_hit_tokens\":7,\"completion_tokens\":2}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri());

    let events = provider
        .infer(deepseek_request(true), InferContext::for_test())
        .await
        .expect("request should succeed")
        .collect::<Vec<_>>()
        .await;

    assert!(events.contains(&ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: UsageSnapshot {
            input_tokens: 3,
            output_tokens: 2,
            cache_read_tokens: 7,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    }));
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_http_402_maps_to_insufficient_balance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(402).set_body_json(json!({
            "error": { "message": "Insufficient Balance" }
        })))
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri());

    let error = match provider
        .infer(deepseek_request(false), InferContext::for_test())
        .await
    {
        Ok(_) => panic!("402 should fail"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ModelError::InsufficientBalance(message) if message == "Insufficient Balance"
    ));
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_maps_official_finish_reasons() {
    for (finish_reason, expected) in [
        ("content_filter", StopReason::ContentFiltered),
        (
            "insufficient_system_resource",
            StopReason::ProviderResourceExhausted,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "ds_1",
                "choices": [{
                    "message": { "role": "assistant", "content": "" },
                    "finish_reason": finish_reason
                }]
            })))
            .mount(&server)
            .await;
        let provider = DeepSeekProvider::from_api_key("provider-key").with_base_url(server.uri());

        let events = provider
            .infer(deepseek_request(false), InferContext::for_test())
            .await
            .expect("request should succeed")
            .collect::<Vec<_>>()
            .await;

        assert!(events.contains(&ModelStreamEvent::MessageDelta {
            stop_reason: Some(expected),
            usage_delta: UsageSnapshot::default(),
        }));
    }
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn provider_deepseek_model_concurrency_permit_is_held_until_stream_drop() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(deepseek_ok_json())
        .mount(&server)
        .await;
    let provider = DeepSeekProvider::from_api_key("provider-key")
        .with_base_url(server.uri())
        .with_model_concurrency_limits(0, 1);

    let first_stream = provider
        .infer(deepseek_request(false), InferContext::for_test())
        .await
        .expect("first request should acquire the only flash permit");
    let second = tokio::spawn({
        let provider = provider.clone();
        async move {
            provider
                .infer(deepseek_request(false), InferContext::for_test())
                .await
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!second.is_finished());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    drop(first_stream);
    let second_stream = tokio::time::timeout(std::time::Duration::from_secs(1), second)
        .await
        .expect("second request should acquire permit after first stream drop")
        .expect("second task should not panic")
        .expect("second request should succeed");
    second_stream.collect::<Vec<_>>().await;
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[cfg(feature = "deepseek")]
fn deepseek_request(stream: bool) -> ModelRequest {
    let mut req = request("deepseek-v4-flash");
    req.stream = stream;
    req
}

#[cfg(feature = "deepseek")]
fn deepseek_ok_json() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "ds_1",
        "choices": [{
            "message": { "role": "assistant", "content": "ok" },
            "finish_reason": "stop"
        }],
        "usage": {}
    }))
}

#[cfg(feature = "minimax")]
#[test]
fn provider_minimax_catalog_matches_official_capabilities() {
    let provider = MinimaxProvider::from_api_key("provider-key");
    let models = provider.supported_models();
    let m3 = models
        .iter()
        .find(|model| model.model_id == "MiniMax-M3")
        .expect("MiniMax-M3 should be listed");
    assert_eq!(m3.context_window, 1_000_000);
    assert_eq!(m3.max_output_tokens, 524_288);
    assert!(m3.conversation_capability.tool_calling);
    assert!(m3.conversation_capability.reasoning);
    assert!(m3.conversation_capability.prompt_cache);
    assert_eq!(
        m3.conversation_capability.input_modalities,
        vec![
            ModelModality::Text,
            ModelModality::Image,
            ModelModality::Video,
        ]
    );

    let m27 = models
        .iter()
        .find(|model| model.model_id == "MiniMax-M2.7")
        .expect("MiniMax-M2.7 should be listed");
    assert_eq!(m27.context_window, 204_800);
    assert_eq!(m27.max_output_tokens, 204_800);
    assert_eq!(
        m27.conversation_capability.input_modalities,
        vec![ModelModality::Text]
    );
    assert!(!models.iter().any(|model| model.model_id == "M2-her"));
}
provider_test!(
    "minimax",
    provider_minimax_streams_chat_completions,
    MinimaxProvider,
    "minimax",
    MINIMAX_API_KEY_ENV,
    "MINIMAX_API_KEY",
    "MiniMax-M2.7",
    "/v1/chat/completions"
);

#[cfg(feature = "minimax")]
#[tokio::test]
#[ignore = "requires MINIMAX_API_KEY and JYOWO_LIVE_MINIMAX=1; uses real MiniMax streaming API"]
async fn provider_minimax_live_streams_chat_completions() {
    if std::env::var("JYOWO_LIVE_MINIMAX").ok().as_deref() != Some("1") {
        return;
    }
    let api_key = std::env::var("MINIMAX_API_KEY").expect("MINIMAX_API_KEY is required");
    let mut provider = MinimaxProvider::from_api_key(api_key);
    if let Ok(base_url) = std::env::var("MINIMAX_BASE_URL") {
        provider = provider.with_base_url(base_url);
    }

    let mut req = request("MiniMax-M2.7");
    req.max_tokens = Some(16);
    req.temperature = Some(0.0);
    if let Some(message) = req.messages.first_mut() {
        message.parts = vec![MessagePart::Text("Reply with exactly OK.".to_owned())];
    }

    let events = provider
        .infer(req, InferContext::for_test())
        .await
        .expect("live stream request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ModelStreamEvent::StreamError { .. })),
        "live stream should not emit stream errors"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ContentBlockDelta {
                delta: ContentDelta::Text(text),
                ..
            } if !text.trim().is_empty()
        )),
        "live stream should emit text deltas"
    );
    assert!(events.contains(&ModelStreamEvent::MessageStop));
}
provider_test!(
    "qwen",
    provider_qwen_streams_chat_completions,
    QwenProvider,
    "qwen",
    QWEN_API_KEY_ENV,
    "QWEN_API_KEY",
    "qwen3.7-max",
    "/v1/chat/completions"
);
provider_test!(
    "doubao",
    provider_doubao_streams_chat_completions,
    DoubaoProvider,
    "doubao",
    DOUBAO_API_KEY_ENV,
    "DOUBAO_API_KEY",
    "doubao-seed-1.6",
    "/chat/completions"
);
provider_test!(
    "zhipu",
    provider_zhipu_streams_chat_completions,
    ZhipuProvider,
    "zhipu",
    ZHIPU_API_KEY_ENV,
    "ZHIPU_API_KEY",
    "glm-5",
    "/chat/completions"
);
provider_test!(
    "km",
    provider_km_streams_chat_completions,
    KmProvider,
    "km",
    KM_API_KEY_ENV,
    "KM_API_KEY",
    "kimi-k2.5",
    "/v1/chat/completions"
);
