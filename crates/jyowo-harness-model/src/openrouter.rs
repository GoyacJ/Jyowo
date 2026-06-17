use async_trait::async_trait;
use harness_contracts::ModelError;
use std::sync::Arc;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    InferContext, ModelCapabilities, ModelCredentialResolver, ModelDescriptor, ModelProvider,
    ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api";
const PROVIDER_ID: &str = "openrouter";
pub const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";

#[derive(Clone)]
pub struct OpenRouterProvider {
    client: OpenAiCompatibleClient,
}

impl OpenRouterProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID),
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

impl OpenAiCompatibleProviderExt for OpenRouterProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![
            descriptor("openai/gpt-4o-mini", "OpenAI GPT-4o mini via OpenRouter"),
            descriptor(
                "anthropic/claude-3.5-sonnet",
                "Claude 3.5 Sonnet via OpenRouter",
            ),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    fn supports_thinking(&self) -> bool {
        false
    }
}

fn descriptor(model_id: &str, display_name: &str) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "openrouter".to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        context_window: 128_000,
        max_output_tokens: 8192,
        capabilities: ModelCapabilities {
            supports_tools: true,
            supports_vision: true,
            supports_thinking: false,
            supports_prompt_cache: false,
            supports_tool_reference: false,
            tool_reference_beta_header: None,
        },
        pricing: None,
    }
}
