use async_trait::async_trait;
use harness_contracts::ModelError;
use std::sync::Arc;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    InferContext, ModelCapabilities, ModelCredentialResolver, ModelDescriptor, ModelProvider,
    ModelRequest, ModelStream, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const PROVIDER_ID: &str = "codex";
pub const CODEX_API_KEY_ENV: &str = "CODEX_API_KEY";

#[derive(Clone)]
pub struct CodexResponsesProvider {
    client: OpenAiCompatibleClient,
}

impl CodexResponsesProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_responses_path("/v1/responses"),
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

impl OpenAiCompatibleProviderExt for CodexResponsesProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for CodexResponsesProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![
            descriptor("gpt-5.4-codex", "GPT-5.4 Codex"),
            descriptor("gpt-5.3-codex", "GPT-5.3 Codex"),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_thinking(&self) -> bool {
        true
    }
}

fn descriptor(model_id: &str, display_name: &str) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "codex".to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            supports_tools: true,
            supports_vision: false,
            supports_thinking: true,
            supports_prompt_cache: true,
            supports_tool_reference: false,
            tool_reference_beta_header: None,
        },
        pricing: None,
    }
}
