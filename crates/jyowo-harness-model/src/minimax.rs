use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::anthropic::AnthropicClient;
use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const PROVIDER_ID: &str = "minimax";
pub const MINIMAX_API_KEY_ENV: &str = "MINIMAX_API_KEY";

#[derive(Clone)]
pub struct MinimaxProvider {
    chat_client: OpenAiProtocolClient,
    responses_client: OpenAiProtocolClient,
    messages_client: AnthropicClient,
}

impl MinimaxProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            chat_client: OpenAiProtocolClient::from_api_key(api_key.clone(), DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::MiniMax)
                .with_chat_completions_path("/v1/chat/completions")
                .with_max_tokens_field("max_completion_tokens"),
            responses_client: OpenAiProtocolClient::from_api_key(api_key.clone(), DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_responses_path("/v1/responses"),
            messages_client: AnthropicClient::from_api_key(api_key)
                .with_provider_id(PROVIDER_ID)
                .with_base_url(DEFAULT_BASE_URL)
                .with_messages_path("/anthropic/v1/messages"),
        }
    }

    #[cfg(test)]
    pub(crate) fn chat_dialect_for_test(&self) -> OpenAiChatDialect {
        self.chat_client.chat_dialect()
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        self.chat_client = self.chat_client.with_base_url(base_url.clone());
        self.responses_client = self.responses_client.with_base_url(base_url.clone());
        self.messages_client = self.messages_client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.chat_client = self
            .chat_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.responses_client = self
            .responses_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.messages_client = self
            .messages_client
            .with_credential_resolver(Arc::clone(&resolver));
        self
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
        match req.protocol {
            ModelProtocol::Responses => self.responses_client.infer(req, ctx).await,
            ModelProtocol::ChatCompletions => self.chat_client.infer(req, ctx).await,
            ModelProtocol::Messages => self.messages_client.infer(req, ctx).await,
            protocol => Err(ModelError::InvalidRequest(format!(
                "MiniMax provider supports Responses, ChatCompletions, and Messages, got {protocol:?}"
            ))),
        }
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
    }
}
