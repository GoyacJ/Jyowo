use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const PROVIDER_ID: &str = "zhipu";
pub const ZHIPU_API_KEY_ENV: &str = "ZHIPU_API_KEY";

#[derive(Clone)]
pub struct ZhipuProvider {
    client: OpenAiCompatibleClient,
}

impl ZhipuProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
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

impl OpenAiCompatibleProviderExt for ZhipuProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for ZhipuProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-21: https://docs.bigmodel.cn/api-reference/模型-api/对话补全
        vec![
            descriptor("glm-5.2", "GLM-5.2", 1_000_000, 131_072),
            descriptor("glm-5.1", "GLM-5.1", 1_000_000, 131_072),
            descriptor("glm-5-turbo", "GLM-5 Turbo", 1_000_000, 131_072),
            descriptor("glm-5", "GLM-5", 1_000_000, 131_072),
            descriptor("glm-4.7", "GLM-4.7", 1_000_000, 131_072),
            descriptor("glm-4.6", "GLM-4.6", 128_000, 16_384),
            descriptor("glm-4.5-flash", "GLM-4.5 Flash", 128_000, 16_384),
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
            reasoning: false,
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
