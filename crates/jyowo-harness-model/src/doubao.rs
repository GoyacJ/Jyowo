use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";
const PROVIDER_ID: &str = "doubao";
pub const DOUBAO_API_KEY_ENV: &str = "DOUBAO_API_KEY";

#[derive(Clone)]
pub struct DoubaoProvider {
    chat_client: OpenAiProtocolClient,
    responses_client: OpenAiProtocolClient,
}

impl DoubaoProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            chat_client: doubao_chat_client(api_key.clone(), DEFAULT_BASE_URL),
            responses_client: doubao_responses_client(api_key, DEFAULT_BASE_URL),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        self.chat_client = self.chat_client.with_base_url(base_url.clone());
        self.responses_client = self.responses_client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.chat_client = self
            .chat_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.responses_client = self.responses_client.with_credential_resolver(resolver);
        self
    }
}

impl OpenAiProtocolProviderExt for DoubaoProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.chat_client
    }
}

#[async_trait]
impl ModelProvider for DoubaoProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        crate::catalog::provider_model_descriptors(PROVIDER_ID)
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        match req.protocol {
            ModelProtocol::ChatCompletions => self.chat_client.infer(req, ctx).await,
            ModelProtocol::Responses => self.responses_client.infer(req, ctx).await,
            protocol => Err(ModelError::InvalidRequest(format!(
                "Doubao provider supports chat_completions and responses, got {protocol:?}"
            ))),
        }
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }
}

fn doubao_chat_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_chat_dialect(OpenAiChatDialect::Doubao)
        .with_chat_completions_path("/chat/completions")
        .with_max_tokens_field("max_completion_tokens")
}

fn doubao_responses_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_responses_path("/responses")
}
