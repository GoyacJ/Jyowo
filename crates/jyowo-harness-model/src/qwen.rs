use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode";
const PROVIDER_ID: &str = "qwen";
pub const QWEN_API_KEY_ENV: &str = "QWEN_API_KEY";

#[derive(Clone)]
pub struct QwenProvider {
    client: OpenAiCompatibleClient,
}

impl QwenProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_completions_path("/v1/chat/completions"),
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

impl OpenAiCompatibleProviderExt for QwenProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for QwenProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-21: https://help.aliyun.com/zh/model-studio/models
        vec![
            descriptor("qwen3.7-max", "Qwen3.7 Max", 1_000_000, 32_768, false),
            descriptor(
                "qwen3.7-max-thinking",
                "Qwen3.7 Max Thinking",
                1_000_000,
                32_768,
                true,
            ),
            descriptor("qwen3.7-plus", "Qwen3.7 Plus", 1_000_000, 32_768, false),
            descriptor("qwen3.6-flash", "Qwen3.6 Flash", 128_000, 8192, false),
            descriptor("qwen3-coder-plus", "Qwen3 Coder Plus", 128_000, 8192, false),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }
}

fn descriptor(
    model_id: &str,
    display_name: &str,
    context_window: u32,
    max_output_tokens: u32,
    reasoning: bool,
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
            tool_calling: true,
            reasoning,
            prompt_cache: false,
            streaming: true,
            structured_output: false,
            input_modalities: vec![ModelModality::Text],
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_chat_plain(),
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}
