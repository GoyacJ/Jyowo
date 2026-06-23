use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const PROVIDER_ID: &str = "minimax";
pub const MINIMAX_API_KEY_ENV: &str = "MINIMAX_API_KEY";
const MULTIPART_BOUNDARY: &str = "jyowo-minimax-boundary";

#[derive(Clone)]
pub struct MinimaxApiClient {
    http: reqwest::Client,
    api_key: SecretString,
    base_url: String,
}

impl MinimaxApiClient {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub async fn image_generation(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/image_generation", request).await
    }

    pub async fn text_generation(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/text/chatcompletion_v2", request).await
    }

    pub async fn responses(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/responses", request).await
    }

    pub async fn responses_input_tokens(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/responses/input_tokens", request).await
    }

    pub async fn anthropic_messages(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/anthropic/v1/messages", request).await
    }

    pub async fn anthropic_count_tokens(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/anthropic/v1/messages/count_tokens", request)
            .await
    }

    pub async fn video_generation(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/video_generation", request).await
    }

    pub async fn query_video_generation(&self, task_id: &str) -> Result<Value, ModelError> {
        self.get_json(
            "/v1/query/video_generation",
            &[("task_id", task_id.to_owned())],
        )
        .await
    }

    pub async fn video_template_generation(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/video_template_generation", request)
            .await
    }

    pub async fn query_video_template_generation(
        &self,
        task_id: &str,
    ) -> Result<Value, ModelError> {
        self.get_json(
            "/v1/query/video_template_generation",
            &[("task_id", task_id.to_owned())],
        )
        .await
    }

    pub async fn text_to_speech(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/t2a_v2", request).await
    }

    pub async fn text_to_speech_async(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/t2a_async_v2", request).await
    }

    pub async fn query_text_to_speech_async(&self, task_id: &str) -> Result<Value, ModelError> {
        self.get_json(
            "/v1/query/t2a_async_query_v2",
            &[("task_id", task_id.to_owned())],
        )
        .await
    }

    pub async fn voice_clone(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/voice_clone", request).await
    }

    pub async fn voice_design(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/voice_design", request).await
    }

    pub async fn get_voice(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/get_voice", request).await
    }

    pub async fn delete_voice(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/delete_voice", request).await
    }

    pub async fn lyrics_generation(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/lyrics_generation", request).await
    }

    pub async fn music_generation(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/music_generation", request).await
    }

    pub async fn music_cover_preprocess(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/v1/music_cover_preprocess", request).await
    }

    pub async fn file_retrieve(&self, file_id: &str) -> Result<Value, ModelError> {
        self.get_json("/v1/files/retrieve", &[("file_id", file_id.to_owned())])
            .await
    }

    pub async fn file_retrieve_content(
        &self,
        file_id: &str,
        group_id: Option<&str>,
    ) -> Result<Vec<u8>, ModelError> {
        let mut query = vec![("file_id", file_id.to_owned())];
        if let Some(group_id) = group_id {
            query.push(("GroupId", group_id.to_owned()));
        }
        self.get_bytes("/v1/files/retrieve_content", &query).await
    }

    pub async fn file_list(&self, purpose: Option<&str>) -> Result<Value, ModelError> {
        let query = purpose
            .map(|purpose| vec![("purpose", purpose.to_owned())])
            .unwrap_or_default();
        self.get_json("/v1/files/list", &query).await
    }

    pub async fn file_upload(
        &self,
        purpose: &str,
        file_name: &str,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Value, ModelError> {
        self.file_upload_with_group_id(purpose, file_name, bytes, None)
            .await
    }

    pub async fn file_upload_with_group_id(
        &self,
        purpose: &str,
        file_name: &str,
        bytes: impl Into<Vec<u8>>,
        group_id: Option<&str>,
    ) -> Result<Value, ModelError> {
        let body = multipart_body(purpose, file_name, bytes.into());
        let query = group_id
            .map(|group_id| vec![("GroupId", group_id.to_owned())])
            .unwrap_or_default();
        let response = self
            .http
            .post(self.query_url("/v1/files/upload", &query)?)
            .headers(self.headers_with_content_type(&format!(
                "multipart/form-data; boundary={MULTIPART_BOUNDARY}"
            ))?)
            .body(body)
            .send()
            .await
            .map_err(transport_error)?;
        self.response_json(response).await
    }

    pub async fn file_delete(&self, file_id: &str) -> Result<Value, ModelError> {
        self.post_json(
            "/v1/files/delete",
            serde_json::json!({ "file_id": file_id }),
        )
        .await
    }

    pub async fn list_models(&self) -> Result<Value, ModelError> {
        self.get_json("/v1/models", &[]).await
    }

    pub async fn retrieve_model(&self, model_id: &str) -> Result<Value, ModelError> {
        self.get_json(&format!("/v1/models/{}", path_segment(model_id)), &[])
            .await
    }

    pub async fn list_anthropic_models(
        &self,
        limit: Option<u32>,
        after_id: Option<&str>,
        before_id: Option<&str>,
    ) -> Result<Value, ModelError> {
        let mut query = Vec::new();
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        if let Some(after_id) = after_id {
            query.push(("after_id", after_id.to_owned()));
        }
        if let Some(before_id) = before_id {
            query.push(("before_id", before_id.to_owned()));
        }
        let response = self
            .http
            .get(self.query_url("/anthropic/v1/models", &query)?)
            .headers(self.x_api_key_headers()?)
            .send()
            .await
            .map_err(transport_error)?;
        self.response_json(response).await
    }

    pub async fn retrieve_anthropic_model(&self, model_id: &str) -> Result<Value, ModelError> {
        let response = self
            .http
            .get(self.url(&format!("/anthropic/v1/models/{}", path_segment(model_id))))
            .headers(self.x_api_key_headers()?)
            .send()
            .await
            .map_err(transport_error)?;
        self.response_json(response).await
    }

    async fn post_json(&self, path: &str, request: Value) -> Result<Value, ModelError> {
        let response = self
            .http
            .post(self.url(path))
            .headers(self.headers()?)
            .json(&request)
            .send()
            .await
            .map_err(transport_error)?;
        self.response_json(response).await
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value, ModelError> {
        let url = self.query_url(path, query)?;
        let response = self
            .http
            .get(url)
            .headers(self.headers()?)
            .send()
            .await
            .map_err(transport_error)?;
        self.response_json(response).await
    }

    async fn get_bytes(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<u8>, ModelError> {
        let response = self
            .http
            .get(self.query_url(path, query)?)
            .headers(self.headers()?)
            .send()
            .await
            .map_err(transport_error)?;
        self.response_bytes(response).await
    }

    async fn response_json(&self, response: reqwest::Response) -> Result<Value, ModelError> {
        let status = response.status();
        let body = response.text().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(ModelError::ProviderUnavailable(format!(
                "MiniMax API request failed with status {status}: {}",
                redact_secret(&body, self.api_key.expose_secret())
            )));
        }
        serde_json::from_str(&body).map_err(|error| {
            ModelError::UnexpectedResponse(format!("invalid MiniMax API response: {error}"))
        })
    }

    async fn response_bytes(&self, response: reqwest::Response) -> Result<Vec<u8>, ModelError> {
        let status = response.status();
        let body = response.bytes().await.map_err(transport_error)?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&body);
            return Err(ModelError::ProviderUnavailable(format!(
                "MiniMax API request failed with status {status}: {}",
                redact_secret(&body, self.api_key.expose_secret())
            )));
        }
        Ok(body.to_vec())
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
        self.headers_with_content_type("application/json")
    }

    fn headers_with_content_type(&self, content_type: &str) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", self.api_key.expose_secret());
        let auth = HeaderValue::from_str(&value)
            .map_err(|error| ModelError::AuthExpired(error.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type)
                .map_err(|error| ModelError::InvalidRequest(error.to_string()))?,
        );
        Ok(headers)
    }

    fn x_api_key_headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(self.api_key.expose_secret())
            .map_err(|error| ModelError::AuthExpired(error.to_string()))?;
        headers.insert("x-api-key", auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            }
        )
    }

    fn query_url(&self, path: &str, query: &[(&str, String)]) -> Result<reqwest::Url, ModelError> {
        let mut url = reqwest::Url::parse(&self.url(path))
            .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }
}

fn transport_error(error: reqwest::Error) -> ModelError {
    ModelError::ProviderUnavailable(error.to_string())
}

fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_owned()
    } else {
        text.replace(secret, "[REDACTED]")
    }
}

fn multipart_body(purpose: &str, file_name: &str, bytes: Vec<u8>) -> Vec<u8> {
    let mut body = Vec::new();
    let file_name = multipart_header_value(file_name);
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"purpose\"\r\n\r\n");
    body.extend_from_slice(purpose.as_bytes());
    body.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

fn multipart_header_value(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\r' | '\n' | '"' => '_',
            _ => ch,
        })
        .collect()
}

fn path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[derive(Clone)]
pub struct MinimaxProvider {
    client: OpenAiCompatibleClient,
}

impl MinimaxProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
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

impl OpenAiCompatibleProviderExt for MinimaxProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for MinimaxProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-22: https://platform.minimax.io/docs/guides/models-intro
        vec![
            descriptor(
                "MiniMax-M3",
                "MiniMax M3",
                1_000_000,
                524_288,
                CapabilityProfile::M3,
            ),
            descriptor(
                "MiniMax-M2.7",
                "MiniMax M2.7",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.7-highspeed",
                "MiniMax M2.7 Highspeed",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.5",
                "MiniMax M2.5",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.5-highspeed",
                "MiniMax M2.5 Highspeed",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.1",
                "MiniMax M2.1",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.1-highspeed",
                "MiniMax M2.1 Highspeed",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2",
                "MiniMax M2",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "M2-her",
                "MiniMax M2 Her",
                65_536,
                65_536,
                CapabilityProfile::Text,
            ),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }
}

fn descriptor(
    model_id: &str,
    display_name: &str,
    context_window: u32,
    max_output_tokens: u32,
    profile: CapabilityProfile,
) -> ModelDescriptor {
    let input_modalities = match profile {
        CapabilityProfile::M3 => vec![
            ModelModality::Text,
            ModelModality::Image,
            ModelModality::Video,
        ],
        CapabilityProfile::Text => vec![ModelModality::Text],
    };
    ModelDescriptor {
        provider_id: PROVIDER_ID.to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        conversation_capability: ConversationModelCapability {
            context_window,
            max_output_tokens,
            tool_calling: true,
            reasoning: matches!(profile, CapabilityProfile::M3),
            prompt_cache: true,
            streaming: true,
            structured_output: false,
            input_modalities,
            output_modalities: vec![ModelModality::Text],
        },
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}

#[derive(Debug, Clone, Copy)]
enum CapabilityProfile {
    Text,
    M3,
}
