use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;
use serde::Deserialize;
use serde_json::json;

use crate::openai_protocol::{
    chat_messages_for_request, OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt,
};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://api.moonshot.cn";
const PROVIDER_ID: &str = "km";
pub const KM_API_KEY_ENV: &str = "KM_API_KEY";
pub const MOONSHOT_API_KEY_ENV: &str = "MOONSHOT_API_KEY";
pub const KIMI_API_KEY_ENVS: &[&str] = &[MOONSHOT_API_KEY_ENV, KM_API_KEY_ENV];

#[must_use]
pub fn kimi_api_key_from_env() -> Option<String> {
    KIMI_API_KEY_ENVS.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

#[derive(Clone)]
pub struct KmProvider {
    client: OpenAiProtocolClient,
}

impl KmProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::Kimi)
                .with_chat_completions_path("/v1/chat/completions")
                .with_max_tokens_field("max_completion_tokens"),
        }
    }

    #[must_use]
    pub fn from_env() -> Option<Self> {
        kimi_api_key_from_env().map(Self::from_api_key)
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

    pub async fn list_models(&self, ctx: &InferContext) -> Result<KimiModelList, ModelError> {
        let response = self.client.get_json("/v1/models", None, ctx).await?;
        serde_json::from_value(response).map_err(|error| {
            ModelError::UnexpectedResponse(format!("invalid Kimi models response: {error}"))
        })
    }

    pub async fn estimate_token_count(
        &self,
        req: &ModelRequest,
        ctx: &InferContext,
    ) -> Result<u64, ModelError> {
        let encoded = chat_messages_for_request(req, OpenAiChatDialect::Kimi, ctx).await?;
        let response = self
            .client
            .post_json(
                "/v1/tokenizers/estimate-token-count",
                &json!({
                    "model": req.model_id,
                    "messages": encoded.messages,
                }),
                Some(&req.model_id),
                ctx,
            )
            .await?;
        let response: KimiEstimateTokenCountResponse =
            serde_json::from_value(response).map_err(|error| {
                ModelError::UnexpectedResponse(format!(
                    "invalid Kimi token estimate response: {error}"
                ))
            })?;
        Ok(response.data.total_tokens)
    }
}

impl OpenAiProtocolProviderExt for KmProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for KmProvider {
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiModelList {
    #[serde(default)]
    pub data: Vec<KimiModelInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiModelInfo {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub owned_by: Option<String>,
    pub context_length: Option<u32>,
    #[serde(default)]
    pub supports_image_in: bool,
    #[serde(default)]
    pub supports_video_in: bool,
    #[serde(default)]
    pub supports_reasoning: bool,
}

#[derive(Debug, Deserialize)]
struct KimiEstimateTokenCountResponse {
    data: KimiEstimateTokenCountData,
}

#[derive(Debug, Deserialize)]
struct KimiEstimateTokenCountData {
    total_tokens: u64,
}
