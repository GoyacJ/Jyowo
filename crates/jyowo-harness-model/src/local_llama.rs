use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_compatible::{
    OpenAiChatDialect, OpenAiCompatibleClient, OpenAiCompatibleProviderExt,
};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080";
const PROVIDER_ID: &str = "local-llama";

#[derive(Clone)]
pub struct LocalLlamaProvider {
    client: OpenAiCompatibleClient,
}

impl Default for LocalLlamaProvider {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl LocalLlamaProvider {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::without_api_key(endpoint)
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

impl OpenAiCompatibleProviderExt for LocalLlamaProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for LocalLlamaProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![
            descriptor("llama3.1", "Local Llama 3.1"),
            descriptor("llama3.1:8b", "Local Llama 3.1 8B"),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }
}

fn descriptor(model_id: &str, display_name: &str) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "local-llama".to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window: 128_000,
        max_output_tokens: 8192,
        conversation_capability: ConversationModelCapability {
            context_window: 128_000,
            max_output_tokens: 8192,
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
