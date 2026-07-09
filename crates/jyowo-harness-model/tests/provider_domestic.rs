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
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, StopReason, UsageSnapshot};
use harness_model::*;
use serde_json::Value;
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

    assert_eq!(provider.prompt_cache_style(), PromptCacheStyle::None);
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == model_id
            && model.conversation_capability.tool_calling
            && !model
                .conversation_capability
                .input_modalities
                .contains(&ModelModality::Image)
            && (provider.provider_id() == "qwen" || !model.conversation_capability.reasoning)));

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
    if matches!(provider.provider_id(), "minimax" | "qwen") {
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
    "/v1/chat/completions"
);

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
    assert!(models.iter().any(|model| model.model_id == "M2-her"));
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
    DASHSCOPE_API_KEY_ENV,
    "DASHSCOPE_API_KEY",
    "qwen3.7-max",
    "/chat/completions"
);

#[cfg(feature = "qwen")]
#[test]
fn provider_qwen_catalog_matches_official_text_generation_capabilities() {
    let provider = QwenProvider::from_api_key("provider-key");
    let models = provider.supported_models();
    let model_ids = models
        .iter()
        .map(|model| model.model_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for model_id in [
        "qwen3-max",
        "qwen3-max-2026-01-23",
        "qwen3.7-max",
        "qwen3.7-max-preview",
        "qwen3.7-max-2026-06-08",
        "qwen3.7-max-2026-05-20",
        "qwen3.7-max-2026-05-17",
        "qwen3.7-plus",
        "qwen3.6-plus",
        "qwen3.5-plus",
        "qwen3.6-flash",
        "qwen3.5-flash",
        "qwen-plus",
        "qwen-flash",
        "qwen3-coder-plus",
        "qwen3-coder-flash",
        "qwen3-coder-next",
    ] {
        assert!(model_ids.contains(model_id), "{model_id} should be listed");
    }
    assert!(!model_ids.contains("qwen3.7-max-2026-01-23"));

    let qwen3_max = models
        .iter()
        .find(|model| model.model_id == "qwen3-max")
        .expect("qwen3-max should be listed");
    assert_eq!(qwen3_max.context_window, 256_000);

    let qwen3_coder_next = models
        .iter()
        .find(|model| model.model_id == "qwen3-coder-next")
        .expect("qwen3-coder-next should be listed");
    assert_eq!(qwen3_coder_next.context_window, 256_000);

    let qwen37_max = models
        .iter()
        .find(|model| model.model_id == "qwen3.7-max")
        .expect("qwen3.7-max should be listed");
    assert!(!qwen37_max.conversation_capability.structured_output);

    let qwen37_plus = models
        .iter()
        .find(|model| model.model_id == "qwen3.7-plus")
        .expect("qwen3.7-plus should be listed");
    assert!(qwen37_plus.conversation_capability.structured_output);
    assert_eq!(qwen37_plus.context_window, 1_000_000);
}
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
