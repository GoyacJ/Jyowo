use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;
use serde::Deserialize;
use serde_json::{json, Value};

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

    pub async fn upload_file(
        &self,
        purpose: &str,
        file_name: &str,
        bytes: Vec<u8>,
        ctx: &InferContext,
    ) -> Result<KimiFileObject, ModelError> {
        let response = self
            .client
            .post_multipart_bytes(
                "/v1/files",
                &[("purpose", purpose.to_owned())],
                "file",
                file_name,
                bytes,
                None,
                ctx,
            )
            .await?;
        parse_response(response, "Kimi file upload")
    }

    pub async fn list_files(
        &self,
        purpose: Option<&str>,
        ctx: &InferContext,
    ) -> Result<KimiFileListResponse, ModelError> {
        let query = purpose
            .map(|purpose| vec![("purpose", purpose.to_owned())])
            .unwrap_or_default();
        let response = self
            .client
            .get_json_with_query("/v1/files", &query, None, ctx)
            .await?;
        parse_response(response, "Kimi file list")
    }

    pub async fn retrieve_file(
        &self,
        file_id: &str,
        ctx: &InferContext,
    ) -> Result<KimiFileObject, ModelError> {
        let response = self
            .client
            .get_json(&format!("/v1/files/{file_id}"), None, ctx)
            .await?;
        parse_response(response, "Kimi file retrieve")
    }

    pub async fn file_content(
        &self,
        file_id: &str,
        ctx: &InferContext,
    ) -> Result<String, ModelError> {
        self.client
            .get_text(&format!("/v1/files/{file_id}/content"), None, ctx)
            .await
    }

    pub async fn delete_file(
        &self,
        file_id: &str,
        ctx: &InferContext,
    ) -> Result<KimiFileDeleteResponse, ModelError> {
        let response = self
            .client
            .delete_json(&format!("/v1/files/{file_id}"), None, ctx)
            .await?;
        parse_response(response, "Kimi file delete")
    }

    pub async fn create_batch(
        &self,
        input_file_id: &str,
        endpoint: &str,
        completion_window: &str,
        metadata: Option<Value>,
        ctx: &InferContext,
    ) -> Result<KimiBatchObject, ModelError> {
        let mut body = json!({
            "input_file_id": input_file_id,
            "endpoint": endpoint,
            "completion_window": completion_window,
        });
        if let Some(metadata) = metadata {
            body["metadata"] = metadata;
        }
        let response = self
            .client
            .post_json("/v1/batches", &body, None, ctx)
            .await?;
        parse_response(response, "Kimi batch create")
    }

    pub async fn list_batches(
        &self,
        after: Option<&str>,
        limit: Option<u32>,
        ctx: &InferContext,
    ) -> Result<KimiBatchListResponse, ModelError> {
        let mut query = Vec::new();
        if let Some(after) = after {
            query.push(("after", after.to_owned()));
        }
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        let response = self
            .client
            .get_json_with_query("/v1/batches", &query, None, ctx)
            .await?;
        parse_response(response, "Kimi batch list")
    }

    pub async fn retrieve_batch(
        &self,
        batch_id: &str,
        ctx: &InferContext,
    ) -> Result<KimiBatchObject, ModelError> {
        let response = self
            .client
            .get_json(&format!("/v1/batches/{batch_id}"), None, ctx)
            .await?;
        parse_response(response, "Kimi batch retrieve")
    }

    pub async fn cancel_batch(
        &self,
        batch_id: &str,
        ctx: &InferContext,
    ) -> Result<KimiBatchObject, ModelError> {
        let response = self
            .client
            .post_json(
                &format!("/v1/batches/{batch_id}/cancel"),
                &json!({}),
                None,
                ctx,
            )
            .await?;
        parse_response(response, "Kimi batch cancel")
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiFileObject {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub created_at: Option<u64>,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub status_details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiFileListResponse {
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub data: Vec<KimiFileObject>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiFileDeleteResponse {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiBatchObject {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    pub endpoint: String,
    pub input_file_id: String,
    pub completion_window: String,
    pub status: String,
    #[serde(default)]
    pub output_file_id: Option<String>,
    #[serde(default)]
    pub error_file_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<u64>,
    #[serde(default)]
    pub in_progress_at: Option<u64>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub finalizing_at: Option<u64>,
    #[serde(default)]
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub failed_at: Option<u64>,
    #[serde(default)]
    pub expired_at: Option<u64>,
    #[serde(default)]
    pub cancelling_at: Option<u64>,
    #[serde(default)]
    pub cancelled_at: Option<u64>,
    #[serde(default)]
    pub request_counts: Option<KimiBatchRequestCounts>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub errors: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiBatchRequestCounts {
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub completed: u64,
    #[serde(default)]
    pub failed: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KimiBatchListResponse {
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub data: Vec<KimiBatchObject>,
    #[serde(default)]
    pub first_id: Option<String>,
    #[serde(default)]
    pub last_id: Option<String>,
    #[serde(default)]
    pub has_more: Option<bool>,
}

fn parse_response<T: for<'de> Deserialize<'de>>(
    response: Value,
    label: &str,
) -> Result<T, ModelError> {
    serde_json::from_value(response).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid {label} response: {error}"))
    })
}
