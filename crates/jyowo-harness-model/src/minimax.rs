use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const PROVIDER_ID: &str = "minimax";
pub const MINIMAX_API_KEY_ENV: &str = "MINIMAX_API_KEY";

#[derive(Clone)]
pub struct MinimaxProvider {
    client: OpenAiProtocolClient,
}

impl MinimaxProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::MiniMax)
                .with_chat_completions_path("/v1/chat/completions")
                .with_max_tokens_field("max_completion_tokens"),
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

impl OpenAiProtocolProviderExt for MinimaxProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for MinimaxProvider {
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
        ModelProtocol::ChatCompletions
    }
}
