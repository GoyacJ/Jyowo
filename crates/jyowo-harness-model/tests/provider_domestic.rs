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
        api_mode: ApiMode::ChatCompletions,
        extra: Value::Null,
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
    assert!(provider.supports_tools());
    assert!(!provider.supports_vision());
    assert!(!provider.supports_thinking());
    assert!(provider
        .supported_models()
        .iter()
        .any(|model| model.model_id == model_id));

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
    "deepseek-chat",
    "/v1/chat/completions"
);
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
provider_test!(
    "qwen",
    provider_qwen_streams_chat_completions,
    QwenProvider,
    "qwen",
    QWEN_API_KEY_ENV,
    "QWEN_API_KEY",
    "qwen3-max",
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
