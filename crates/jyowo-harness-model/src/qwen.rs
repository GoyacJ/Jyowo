use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream,
};

pub const DEFAULT_BASE_URL: &str = "https://dashscope-us.aliyuncs.com/compatible-mode/v1";
pub const LEGACY_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode";
pub const LEGACY_BASE_URL_V1: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const PROVIDER_ID: &str = "qwen";
pub const DASHSCOPE_API_KEY_ENV: &str = "DASHSCOPE_API_KEY";
pub const QWEN_API_KEY_ENV: &str = "QWEN_API_KEY";

#[derive(Clone)]
pub struct QwenProvider {
    chat_client: OpenAiProtocolClient,
    responses_client: OpenAiProtocolClient,
}

impl QwenProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            chat_client: qwen_chat_client(api_key.clone(), DEFAULT_BASE_URL),
            responses_client: qwen_responses_client(api_key, DEFAULT_BASE_URL),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base_url = normalize_qwen_base_url(base_url.into());
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

    #[must_use]
    pub fn with_default_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.chat_client = self.chat_client.with_extra_headers(headers.clone());
        self.responses_client = self.responses_client.with_extra_headers(headers);
        self
    }
}

impl OpenAiProtocolProviderExt for QwenProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.chat_client
    }
}

#[async_trait]
impl ModelProvider for QwenProvider {
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
                "QwenProvider only supports chat_completions and responses, got {protocol:?}"
            ))),
        }
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }
}

fn qwen_chat_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_chat_dialect(OpenAiChatDialect::Qwen)
        .with_chat_completions_path("/chat/completions")
        .with_max_tokens_field("max_completion_tokens")
}

fn qwen_responses_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_responses_path("/responses")
}

pub fn normalize_qwen_base_url(base_url: impl Into<String>) -> String {
    let base_url = base_url.into().trim_end_matches('/').to_owned();
    if base_url == LEGACY_BASE_URL {
        LEGACY_BASE_URL_V1.to_owned()
    } else {
        base_url
    }
}
