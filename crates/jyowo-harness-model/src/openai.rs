use async_trait::async_trait;
use harness_contracts::ModelError;
use std::sync::Arc;

use crate::openai_protocol::{OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const PROVIDER_ID: &str = "openai";
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";

#[derive(Clone)]
pub struct OpenAiProvider {
    client: OpenAiProtocolClient,
    protocol: ModelProtocol,
}

impl OpenAiProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_responses_path("/v1/responses"),
            protocol: ModelProtocol::Responses,
        }
    }

    #[must_use]
    pub(crate) fn with_chat_completions(mut self) -> Self {
        self.client = self
            .client
            .with_chat_completions_path("/v1/chat/completions");
        self.protocol = ModelProtocol::ChatCompletions;
        self
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

impl OpenAiProtocolProviderExt for OpenAiProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        crate::catalog::provider_model_descriptors(PROVIDER_ID)
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_protocol(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        self.protocol
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
    }
}
