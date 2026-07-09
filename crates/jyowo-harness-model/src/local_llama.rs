use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080";
const PROVIDER_ID: &str = "local-llama";

#[derive(Clone)]
pub struct LocalLlamaProvider {
    client: OpenAiProtocolClient,
}

impl Default for LocalLlamaProvider {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl LocalLlamaProvider {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::without_api_key(endpoint)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::LocalLlama)
                .with_chat_completions_path("/v1/chat/completions"),
        }
    }

    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.client = self.client.with_api_key(api_key);
        self
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.client = self.client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.client = self.client.with_timeout(timeout);
        self
    }

    #[must_use]
    pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.client = self.client.with_max_concurrency(max_concurrency);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.client = self.client.with_credential_resolver(resolver);
        self
    }
}

impl OpenAiProtocolProviderExt for LocalLlamaProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for LocalLlamaProvider {
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
