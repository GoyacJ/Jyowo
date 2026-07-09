use std::sync::Arc;

use async_trait::async_trait;
use chrono::NaiveDate;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
    PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const PROVIDER_ID: &str = "zhipu";
pub const ZHIPU_API_KEY_ENV: &str = "ZHIPU_API_KEY";
pub const ZAI_API_KEY_ENV: &str = "ZAI_API_KEY";
pub const ZHIPU_API_KEY_ENVS: [&str; 2] = [ZHIPU_API_KEY_ENV, ZAI_API_KEY_ENV];

#[derive(Clone)]
pub struct ZhipuProvider {
    client: OpenAiProtocolClient,
}

impl ZhipuProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::Zhipu)
                .with_chat_completions_path("/chat/completions"),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.client = self.client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.client = self.client.with_credential_resolver(resolver);
        self
    }
}

impl OpenAiProtocolProviderExt for ZhipuProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for ZhipuProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-07-09: https://docs.bigmodel.cn/api-reference/模型-api/对话补全
        vec![
            descriptor(
                "glm-5.2",
                "GLM-5.2",
                1_000_000,
                131_072,
                true,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-5.1",
                "GLM-5.1",
                200_000,
                131_072,
                true,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-5-turbo",
                "GLM-5 Turbo",
                200_000,
                131_072,
                false,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-5",
                "GLM-5",
                200_000,
                131_072,
                true,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.7",
                "GLM-4.7",
                200_000,
                131_072,
                true,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.7-flash",
                "GLM-4.7 Flash",
                200_000,
                131_072,
                false,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.7-flashx",
                "GLM-4.7 FlashX",
                200_000,
                131_072,
                false,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.6",
                "GLM-4.6",
                200_000,
                131_072,
                false,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.5-air",
                "GLM-4.5 Air",
                128_000,
                98_304,
                false,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.5-airx",
                "GLM-4.5 AirX",
                128_000,
                98_304,
                false,
                true,
                true,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4.5-flash",
                "GLM-4.5 Flash",
                128_000,
                98_304,
                false,
                true,
                true,
                true,
                retiring_on(2026, 1, 30),
            ),
            descriptor(
                "glm-4-flash-250414",
                "GLM-4 Flash 250414",
                128_000,
                16_384,
                false,
                false,
                false,
                true,
                ModelLifecycle::Stable,
            ),
            descriptor(
                "glm-4-flashx-250414",
                "GLM-4 FlashX 250414",
                128_000,
                16_384,
                false,
                false,
                false,
                true,
                ModelLifecycle::Stable,
            ),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_protocol(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
    }
}

#[must_use]
pub fn zhipu_api_key_from_env() -> Option<String> {
    env_api_key(ZHIPU_API_KEY_ENV).or_else(|| env_api_key(ZAI_API_KEY_ENV))
}

fn env_api_key(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn descriptor(
    model_id: &str,
    display_name: &str,
    context_window: u32,
    max_output_tokens: u32,
    tool_calling: bool,
    reasoning: bool,
    prompt_cache: bool,
    structured_output: bool,
    lifecycle: ModelLifecycle,
) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: PROVIDER_ID.to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        conversation_capability: ConversationModelCapability {
            context_window,
            max_output_tokens,
            tool_calling,
            reasoning,
            prompt_cache,
            streaming: true,
            structured_output,
            input_modalities: vec![
                ModelModality::Text,
                ModelModality::Image,
                ModelModality::Video,
            ],
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_chat_zhipu(),
        lifecycle,
        pricing: None,
    }
}

fn retiring_on(year: i32, month: u32, day: u32) -> ModelLifecycle {
    ModelLifecycle::Retiring {
        retirement_date: NaiveDate::from_ymd_opt(year, month, day)
            .expect("retirement date should be valid"),
    }
}
