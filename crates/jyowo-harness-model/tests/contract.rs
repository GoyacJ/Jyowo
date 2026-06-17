#![cfg(any(
    feature = "mock",
    feature = "anthropic",
    feature = "openai",
    feature = "openrouter",
    feature = "gemini",
    feature = "bedrock",
    feature = "codex",
    feature = "local-llama",
    feature = "deepseek",
    feature = "minimax",
    feature = "qwen",
    feature = "doubao",
    feature = "zhipu",
    feature = "km"
))]

use std::time::{Duration, Instant};

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, ModelError};
use harness_model::*;
use serde_json::Value;

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "openrouter",
    feature = "gemini",
    feature = "codex",
    feature = "local-llama",
    feature = "deepseek",
    feature = "minimax",
    feature = "qwen",
    feature = "doubao",
    feature = "zhipu",
    feature = "km"
))]
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request(model_id: &str, api_mode: ApiMode) -> ModelRequest {
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
        api_mode,
        extra: Value::Null,
    }
}

async fn run_contract_tests<P>(provider: P, req: ModelRequest)
where
    P: ModelProvider,
{
    let first_id = provider.provider_id().to_owned();
    assert!(!first_id.is_empty());
    assert_eq!(first_id, provider.provider_id());
    assert!(!provider.supported_models().is_empty());
    assert_eq!(provider.health().await, HealthStatus::Healthy);

    let events = provider
        .infer(req.clone(), InferContext::for_test())
        .await
        .expect("contract stream should start")
        .collect::<Vec<_>>()
        .await;
    assert!(!events.is_empty());

    let cancelled = InferContext::for_test();
    cancelled.cancel.cancel();
    let error = match provider.infer(req.clone(), cancelled).await {
        Ok(_) => panic!("cancelled context should fail"),
        Err(error) => error,
    };
    assert_eq!(error, ModelError::Cancelled);

    let mut expired = InferContext::for_test();
    expired.deadline = Instant::now().checked_sub(Duration::from_millis(1));
    let error = match provider.infer(req, expired).await {
        Ok(_) => panic!("expired deadline should fail"),
        Err(error) => error,
    };
    assert!(matches!(error, ModelError::DeadlineExceeded(_)));
}

#[cfg(feature = "mock")]
#[tokio::test]
async fn contract_mock_provider() {
    run_contract_tests(
        MockProvider::default(),
        request("mock-model", ApiMode::Messages),
    )
    .await;
}

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn contract_anthropic_provider() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: message_start\n",
                        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    run_contract_tests(
        AnthropicProvider::from_api_key("test-key").with_base_url(server.uri()),
        request("claude-3-5-sonnet-20241022", ApiMode::Messages),
    )
    .await;
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn contract_openai_provider() {
    let server = openai_server("/v1/chat/completions").await;
    run_contract_tests(
        OpenAiProvider::from_api_key("test-key").with_base_url(server.uri()),
        request("gpt-4o-mini", ApiMode::ChatCompletions),
    )
    .await;
}

#[cfg(feature = "openrouter")]
#[tokio::test]
async fn contract_openrouter_provider() {
    let server = openai_server("/v1/chat/completions").await;
    run_contract_tests(
        OpenRouterProvider::from_api_key("test-key").with_base_url(server.uri()),
        request("openai/gpt-4o-mini", ApiMode::ChatCompletions),
    )
    .await;
}

#[cfg(feature = "gemini")]
#[tokio::test]
async fn contract_gemini_provider() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "data: {\"responseId\":\"resp_1\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":1,\"candidatesTokenCount\":1}}\n\n",
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    run_contract_tests(
        GeminiProvider::from_api_key("test-key").with_base_url(server.uri()),
        request("gemini-2.5-flash", ApiMode::GenerateContent),
    )
    .await;
}

#[cfg(feature = "bedrock")]
#[tokio::test]
async fn contract_bedrock_provider() {
    run_contract_tests(
        BedrockProvider::from_events(vec![ModelStreamEvent::MessageStop]),
        request(
            "anthropic.claude-3-5-sonnet-20241022-v2:0",
            ApiMode::Messages,
        ),
    )
    .await;
}

#[cfg(feature = "codex")]
#[tokio::test]
async fn contract_codex_provider() {
    let server = responses_server().await;
    run_contract_tests(
        CodexResponsesProvider::from_api_key("test-key").with_base_url(server.uri()),
        request("gpt-5.4-codex", ApiMode::Responses),
    )
    .await;
}

#[cfg(feature = "local-llama")]
#[tokio::test]
async fn contract_local_llama_provider() {
    let server = openai_server("/v1/chat/completions").await;
    run_contract_tests(
        LocalLlamaProvider::new(server.uri()),
        request("llama3.1", ApiMode::ChatCompletions),
    )
    .await;
}

macro_rules! domestic_contract {
    ($cfg:literal, $name:ident, $provider:ident, $model:literal, $path:literal) => {
        #[cfg(feature = $cfg)]
        #[tokio::test]
        async fn $name() {
            let server = openai_server($path).await;
            run_contract_tests(
                $provider::from_api_key("test-key").with_base_url(server.uri()),
                request($model, ApiMode::ChatCompletions),
            )
            .await;
        }
    };
}

domestic_contract!(
    "deepseek",
    contract_deepseek_provider,
    DeepSeekProvider,
    "deepseek-chat",
    "/v1/chat/completions"
);
domestic_contract!(
    "minimax",
    contract_minimax_provider,
    MinimaxProvider,
    "MiniMax-M2.7",
    "/v1/chat/completions"
);
domestic_contract!(
    "qwen",
    contract_qwen_provider,
    QwenProvider,
    "qwen3-max",
    "/v1/chat/completions"
);
domestic_contract!(
    "doubao",
    contract_doubao_provider,
    DoubaoProvider,
    "doubao-seed-1.6",
    "/chat/completions"
);
domestic_contract!(
    "zhipu",
    contract_zhipu_provider,
    ZhipuProvider,
    "glm-5",
    "/chat/completions"
);
domestic_contract!(
    "km",
    contract_km_provider,
    KmProvider,
    "kimi-k2.5",
    "/v1/chat/completions"
);

#[cfg(any(
    feature = "openai",
    feature = "openrouter",
    feature = "local-llama",
    feature = "deepseek",
    feature = "minimax",
    feature = "qwen",
    feature = "doubao",
    feature = "zhipu",
    feature = "km"
))]
async fn openai_server(expected_path: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(expected_path))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"chat_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;
    server
}

#[cfg(feature = "codex")]
async fn responses_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "event: response.output_text.delta\n",
                        "data: {\"delta\":\"ok\"}\n\n",
                        "event: response.completed\n",
                        "data: {\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;
    server
}
