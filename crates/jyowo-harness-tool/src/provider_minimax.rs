use std::collections::BTreeMap;
use std::sync::Arc;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::{
    AuthorizedNetworkPermit, HttpMethod, ToolHttpJsonRequest, ToolHttpResponse,
    ToolNetworkBrokerCap,
};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const MULTIPART_BOUNDARY: &str = "jyowo-minimax-boundary";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB

#[derive(Debug, thiserror::Error)]
pub(crate) enum MinimaxProviderClientError {
    #[error("invalid MiniMax API request: {0}")]
    InvalidRequest(String),
    #[error("MiniMax API authentication header is invalid: {0}")]
    AuthExpired(String),
    #[error("MiniMax API request failed: {0}")]
    ProviderUnavailable(String),
    #[error("invalid MiniMax API response: {0}")]
    UnexpectedResponse(String),
}

/// Internal transport for MiniMax API calls.
enum MinimaxTransport {
    /// Production: authorized broker.
    Broker(Arc<dyn ToolNetworkBrokerCap>, AuthorizedNetworkPermit),
    /// Direct reqwest (test / legacy credential path).
    Direct(reqwest::Client),
}

pub(crate) struct MinimaxApiClient {
    transport: MinimaxTransport,
    api_key: SecretString,
    base_url: String,
}

impl MinimaxApiClient {
    /// Production constructor using the authorized network broker.
    pub(crate) fn from_broker(
        transport: Arc<dyn ToolNetworkBrokerCap>,
        permit: AuthorizedNetworkPermit,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            transport: MinimaxTransport::Broker(transport, permit),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    /// Legacy constructor using a direct reqwest client. Only used in tests.
    #[cfg(test)]
    pub(crate) fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            transport: MinimaxTransport::Direct(
                reqwest::Client::builder()
                    .pool_max_idle_per_host(0)
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            ),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    #[must_use]
    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub(crate) async fn image_generation(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/image_generation", request).await
    }

    pub(crate) async fn responses(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/responses", request).await
    }

    pub(crate) async fn responses_input_tokens(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/responses/input_tokens", request).await
    }

    pub(crate) async fn anthropic_messages(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/anthropic/v1/messages", request).await
    }

    pub(crate) async fn anthropic_count_tokens(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/anthropic/v1/messages/count_tokens", request)
            .await
    }

    pub(crate) async fn video_generation(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/video_generation", request).await
    }

    pub(crate) async fn query_video_generation(
        &self,
        task_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.get_json(
            "/v1/query/video_generation",
            &[("task_id", task_id.to_owned())],
        )
        .await
    }

    pub(crate) async fn video_template_generation(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/video_template_generation", request)
            .await
    }

    pub(crate) async fn query_video_template_generation(
        &self,
        task_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.get_json(
            "/v1/query/video_template_generation",
            &[("task_id", task_id.to_owned())],
        )
        .await
    }

    pub(crate) async fn text_to_speech(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/t2a_v2", request).await
    }

    pub(crate) async fn text_to_speech_async(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/t2a_async_v2", request).await
    }

    pub(crate) async fn query_text_to_speech_async(
        &self,
        task_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.get_json(
            "/v1/query/t2a_async_query_v2",
            &[("task_id", task_id.to_owned())],
        )
        .await
    }

    pub(crate) async fn voice_clone(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/voice_clone", request).await
    }

    pub(crate) async fn voice_design(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/voice_design", request).await
    }

    pub(crate) async fn get_voice(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/get_voice", request).await
    }

    pub(crate) async fn delete_voice(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/delete_voice", request).await
    }

    pub(crate) async fn lyrics_generation(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/lyrics_generation", request).await
    }

    pub(crate) async fn music_generation(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/music_generation", request).await
    }

    pub(crate) async fn music_cover_preprocess(
        &self,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json("/v1/music_cover_preprocess", request).await
    }

    pub(crate) async fn file_retrieve(
        &self,
        file_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.get_json("/v1/files/retrieve", &[("file_id", file_id.to_owned())])
            .await
    }

    pub(crate) async fn file_list(
        &self,
        purpose: Option<&str>,
    ) -> Result<Value, MinimaxProviderClientError> {
        let query = purpose
            .map(|purpose| vec![("purpose", purpose.to_owned())])
            .unwrap_or_default();
        self.get_json("/v1/files/list", &query).await
    }

    pub(crate) async fn file_upload_with_group_id(
        &self,
        purpose: &str,
        file_name: &str,
        bytes: impl Into<Vec<u8>>,
        group_id: Option<&str>,
    ) -> Result<Value, MinimaxProviderClientError> {
        let body = multipart_body(purpose, file_name, bytes.into());
        let query = group_id
            .map(|group_id| vec![("GroupId", group_id.to_owned())])
            .unwrap_or_default();
        let url = self.query_url("/v1/files/upload", &query)?;
        let content_type = format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}");

        match &self.transport {
            MinimaxTransport::Broker(transport, permit) => {
                let mut headers = BTreeMap::new();
                headers.insert(
                    "authorization".to_owned(),
                    format!("Bearer {}", self.api_key.expose_secret()),
                );
                headers.insert("content-type".to_owned(), content_type);
                let req = ToolHttpJsonRequest {
                    method: HttpMethod::Post,
                    url: url.to_string(),
                    headers,
                    body: Some(body),
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                };
                let resp = transport
                    .execute_json(permit, req)
                    .await
                    .map_err(|e| MinimaxProviderClientError::ProviderUnavailable(e.to_string()))?;
                self.broker_response_json(resp.status, &resp.body).await
            }
            MinimaxTransport::Direct(http) => {
                let response = http
                    .post(url)
                    .headers(self.reqwest_headers_with_content_type(&content_type)?)
                    .body(body)
                    .send()
                    .await
                    .map_err(transport_error)?;
                self.direct_response_json(response).await
            }
        }
    }

    pub(crate) async fn file_delete(
        &self,
        file_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.post_json(
            "/v1/files/delete",
            serde_json::json!({ "file_id": file_id }),
        )
        .await
    }

    pub(crate) async fn list_models(&self) -> Result<Value, MinimaxProviderClientError> {
        self.get_json("/v1/models", &[]).await
    }

    pub(crate) async fn retrieve_model(
        &self,
        model_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        self.get_json(&format!("/v1/models/{}", path_segment(model_id)), &[])
            .await
    }

    pub(crate) async fn list_anthropic_models(
        &self,
        limit: Option<u32>,
        after_id: Option<&str>,
        before_id: Option<&str>,
    ) -> Result<Value, MinimaxProviderClientError> {
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
        let url = self.query_url("/anthropic/v1/models", &query)?;

        match &self.transport {
            MinimaxTransport::Broker(transport, permit) => {
                let mut headers = BTreeMap::new();
                headers.insert(
                    "x-api-key".to_owned(),
                    self.api_key.expose_secret().to_string(),
                );
                headers.insert("content-type".to_owned(), "application/json".to_owned());
                let req = ToolHttpJsonRequest {
                    method: HttpMethod::Get,
                    url,
                    headers,
                    body: None,
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                };
                let resp = transport
                    .execute_json(permit, req)
                    .await
                    .map_err(|e| MinimaxProviderClientError::ProviderUnavailable(e.to_string()))?;
                self.broker_response_json(resp.status, &resp.body).await
            }
            MinimaxTransport::Direct(http) => {
                let response = http
                    .get(url)
                    .headers(self.x_api_key_headers()?)
                    .send()
                    .await
                    .map_err(transport_error)?;
                self.direct_response_json(response).await
            }
        }
    }

    pub(crate) async fn retrieve_anthropic_model(
        &self,
        model_id: &str,
    ) -> Result<Value, MinimaxProviderClientError> {
        let url = self.url(&format!("/anthropic/v1/models/{}", path_segment(model_id)));

        match &self.transport {
            MinimaxTransport::Broker(transport, permit) => {
                let mut headers = BTreeMap::new();
                headers.insert(
                    "x-api-key".to_owned(),
                    self.api_key.expose_secret().to_string(),
                );
                headers.insert("content-type".to_owned(), "application/json".to_owned());
                let req = ToolHttpJsonRequest {
                    method: HttpMethod::Get,
                    url,
                    headers,
                    body: None,
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                };
                let resp = transport
                    .execute_json(permit, req)
                    .await
                    .map_err(|e| MinimaxProviderClientError::ProviderUnavailable(e.to_string()))?;
                self.broker_response_json(resp.status, &resp.body).await
            }
            MinimaxTransport::Direct(http) => {
                let response = http
                    .get(url)
                    .headers(self.x_api_key_headers()?)
                    .send()
                    .await
                    .map_err(transport_error)?;
                self.direct_response_json(response).await
            }
        }
    }

    async fn post_json(
        &self,
        path: &str,
        request: Value,
    ) -> Result<Value, MinimaxProviderClientError> {
        let body = serde_json::to_vec(&request)
            .map_err(|e| MinimaxProviderClientError::InvalidRequest(e.to_string()))?;
        let headers = self.broker_headers()?;
        let url = self.url(path);

        match &self.transport {
            MinimaxTransport::Broker(transport, permit) => {
                let req = ToolHttpJsonRequest {
                    method: HttpMethod::Post,
                    url,
                    headers,
                    body: Some(body),
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                };
                let resp = transport
                    .execute_json(permit, req)
                    .await
                    .map_err(|e| MinimaxProviderClientError::ProviderUnavailable(e.to_string()))?;
                self.broker_response_json(resp.status, &resp.body).await
            }
            MinimaxTransport::Direct(http) => {
                let response = http
                    .post(url)
                    .headers(self.reqwest_headers()?)
                    .json(&request)
                    .send()
                    .await
                    .map_err(transport_error)?;
                self.direct_response_json(response).await
            }
        }
    }

    async fn get_json(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, MinimaxProviderClientError> {
        let url = self.query_url(path, query)?;
        let headers = self.broker_headers()?;

        match &self.transport {
            MinimaxTransport::Broker(transport, permit) => {
                let req = ToolHttpJsonRequest {
                    method: HttpMethod::Get,
                    url,
                    headers,
                    body: None,
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                };
                let resp = transport
                    .execute_json(permit, req)
                    .await
                    .map_err(|e| MinimaxProviderClientError::ProviderUnavailable(e.to_string()))?;
                self.broker_response_json(resp.status, &resp.body).await
            }
            MinimaxTransport::Direct(http) => {
                let response = http
                    .get(url)
                    .headers(self.reqwest_headers()?)
                    .send()
                    .await
                    .map_err(transport_error)?;
                self.direct_response_json(response).await
            }
        }
    }

    async fn broker_response_json(
        &self,
        status: u16,
        body: &[u8],
    ) -> Result<Value, MinimaxProviderClientError> {
        let body_str = String::from_utf8_lossy(body).into_owned();
        if !(200..300).contains(&status) {
            return Err(MinimaxProviderClientError::ProviderUnavailable(format!(
                "MiniMax API request failed with status {status}: {}",
                redact_secret(&body_str, self.api_key.expose_secret())
            )));
        }
        serde_json::from_str(&body_str)
            .map_err(|error| MinimaxProviderClientError::UnexpectedResponse(error.to_string()))
    }

    async fn direct_response_json(
        &self,
        response: reqwest::Response,
    ) -> Result<Value, MinimaxProviderClientError> {
        let status = response.status();
        let body = response.text().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(MinimaxProviderClientError::ProviderUnavailable(format!(
                "MiniMax API request failed with status {status}: {}",
                redact_secret(&body, self.api_key.expose_secret())
            )));
        }
        serde_json::from_str(&body)
            .map_err(|error| MinimaxProviderClientError::UnexpectedResponse(error.to_string()))
    }

    fn broker_headers(&self) -> Result<BTreeMap<String, String>, MinimaxProviderClientError> {
        let mut headers = BTreeMap::new();
        headers.insert(
            "authorization".to_owned(),
            format!("Bearer {}", self.api_key.expose_secret()),
        );
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        Ok(headers)
    }

    fn reqwest_headers(&self) -> Result<HeaderMap, MinimaxProviderClientError> {
        self.reqwest_headers_with_content_type("application/json")
    }

    fn reqwest_headers_with_content_type(
        &self,
        content_type: &str,
    ) -> Result<HeaderMap, MinimaxProviderClientError> {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", self.api_key.expose_secret());
        let auth = HeaderValue::from_str(&value)
            .map_err(|error| MinimaxProviderClientError::AuthExpired(error.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type)
                .map_err(|error| MinimaxProviderClientError::InvalidRequest(error.to_string()))?,
        );
        Ok(headers)
    }

    fn x_api_key_headers(&self) -> Result<HeaderMap, MinimaxProviderClientError> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(self.api_key.expose_secret())
            .map_err(|error| MinimaxProviderClientError::AuthExpired(error.to_string()))?;
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

    fn query_url(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<reqwest::Url, MinimaxProviderClientError> {
        let mut url = reqwest::Url::parse(&self.url(path))
            .map_err(|error| MinimaxProviderClientError::InvalidRequest(error.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }
}

fn transport_error(error: reqwest::Error) -> MinimaxProviderClientError {
    MinimaxProviderClientError::ProviderUnavailable(error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{json, Value};
    use wiremock::{
        matchers::{body_json, body_string_contains, header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    #[ignore = "requires MINIMAX_API_KEY and JYOWO_LIVE_MINIMAX=1; may incur provider charges"]
    async fn minimax_live_smoke_uses_official_modules() {
        if std::env::var("JYOWO_LIVE_MINIMAX").ok().as_deref() != Some("1") {
            return;
        }
        let api_key = std::env::var("MINIMAX_API_KEY").expect("MINIMAX_API_KEY is required");
        let client = MinimaxApiClient::from_api_key(api_key);

        client
            .image_generation(json!({
                "model": "image-01",
                "prompt": "a tiny monochrome square icon",
                "response_format": "url"
            }))
            .await
            .expect("image generation should succeed");

        client
            .text_to_speech(json!({
                "model": "speech-2.8-turbo",
                "text": "hello",
                "voice_setting": {"voice_id": "Wise_Woman", "speed": 1.0, "vol": 1.0, "pitch": 0},
                "audio_setting": {"sample_rate": 32000, "bitrate": 128000, "format": "mp3"}
            }))
            .await
            .expect("sync tts should succeed");

        client
            .lyrics_generation(json!({"prompt": "two short lines about morning"}))
            .await
            .expect("lyrics generation should succeed");

        client
            .get_voice(json!({"voice_type": "all"}))
            .await
            .expect("voice list should succeed");

        if std::env::var("JYOWO_LIVE_MINIMAX_EXPENSIVE")
            .ok()
            .as_deref()
            == Some("1")
        {
            client
                .video_generation(json!({
                    "model": "MiniMax-Hailuo-2.3-Fast",
                    "prompt": "a single blue cube rotating slowly"
                }))
                .await
                .expect("video task creation should succeed");

            client
                .music_generation(json!({
                    "model": "music-2.6",
                    "prompt": "short calm instrumental loop",
                    "is_instrumental": true
                }))
                .await
                .expect("music generation should succeed");
        }
    }

    #[tokio::test]
    async fn minimax_provider_client_covers_generation_model_and_file_endpoints() {
        let server = MockServer::start().await;
        let client = MinimaxApiClient::from_api_key("provider-key").with_base_url(server.uri());

        assert_post(
            &server,
            "/v1/image_generation",
            json!({"model": "image-01", "prompt": "tiny icon"}),
            json!({"id": "image-task"}),
        )
        .await;
        assert_eq!(
            client
                .image_generation(json!({"model": "image-01", "prompt": "tiny icon"}))
                .await
                .unwrap()["id"],
            "image-task"
        );

        assert_post(
            &server,
            "/v1/responses",
            json!({"model": "MiniMax-M3", "input": "hi"}),
            json!({"id": "response-1"}),
        )
        .await;
        client
            .responses(json!({"model": "MiniMax-M3", "input": "hi"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/responses/input_tokens",
            json!({"model": "MiniMax-M3", "input": "hi"}),
            json!({"input_tokens": 4}),
        )
        .await;
        client
            .responses_input_tokens(json!({"model": "MiniMax-M3", "input": "hi"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/anthropic/v1/messages",
            json!({"model": "MiniMax-M3", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 8}),
            json!({"id": "msg-1"}),
        )
        .await;
        client
            .anthropic_messages(json!({
                "model": "MiniMax-M3",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 8
            }))
            .await
            .unwrap();

        assert_post(
            &server,
            "/anthropic/v1/messages/count_tokens",
            json!({"model": "MiniMax-M3", "messages": [{"role": "user", "content": "hi"}]}),
            json!({"input_tokens": 4}),
        )
        .await;
        client
            .anthropic_count_tokens(json!({
                "model": "MiniMax-M3",
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/video_generation",
            json!({"model": "MiniMax-Hailuo-2.3-Fast", "prompt": "wave"}),
            json!({"task_id": "video-task"}),
        )
        .await;
        client
            .video_generation(json!({"model": "MiniMax-Hailuo-2.3-Fast", "prompt": "wave"}))
            .await
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/query/video_generation"))
            .and(query_param("task_id", "video-task"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "Success"})))
            .mount(&server)
            .await;
        client.query_video_generation("video-task").await.unwrap();

        assert_post(
            &server,
            "/v1/video_template_generation",
            json!({"template_id": "tpl", "inputs": {}}),
            json!({"task_id": "template-task"}),
        )
        .await;
        client
            .video_template_generation(json!({"template_id": "tpl", "inputs": {}}))
            .await
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/query/video_template_generation"))
            .and(query_param("task_id", "template-task"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"video_url": "https://example.test/v.mp4"})),
            )
            .mount(&server)
            .await;
        client
            .query_video_template_generation("template-task")
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/t2a_v2",
            json!({"model": "speech-2.8-turbo", "text": "hi"}),
            json!({"data": {"audio": "AA=="}}),
        )
        .await;
        client
            .text_to_speech(json!({"model": "speech-2.8-turbo", "text": "hi"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/t2a_async_v2",
            json!({"model": "speech-2.8-turbo", "text": "long"}),
            json!({"task_id": "tts-task"}),
        )
        .await;
        client
            .text_to_speech_async(json!({"model": "speech-2.8-turbo", "text": "long"}))
            .await
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/query/t2a_async_query_v2"))
            .and(query_param("task_id", "tts-task"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "Success"})))
            .mount(&server)
            .await;
        client.query_text_to_speech_async("tts-task").await.unwrap();

        assert_post(
            &server,
            "/v1/voice_clone",
            json!({"file_id": "file-1", "voice_id": "clone-1"}),
            json!({"voice_id": "clone-1"}),
        )
        .await;
        client
            .voice_clone(json!({"file_id": "file-1", "voice_id": "clone-1"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/voice_design",
            json!({"prompt": "warm narrator"}),
            json!({"voice_id": "voice-1"}),
        )
        .await;
        client
            .voice_design(json!({"prompt": "warm narrator"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/get_voice",
            json!({"voice_type": "all"}),
            json!({"voices": []}),
        )
        .await;
        client
            .get_voice(json!({"voice_type": "all"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/delete_voice",
            json!({"voice_id": "voice-1"}),
            json!({"ok": true}),
        )
        .await;
        client
            .delete_voice(json!({"voice_id": "voice-1"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/lyrics_generation",
            json!({"prompt": "short"}),
            json!({"lyrics": "la"}),
        )
        .await;
        client
            .lyrics_generation(json!({"prompt": "short"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/music_generation",
            json!({"model": "music-2.6", "prompt": "short"}),
            json!({"audio_url": "https://example.test/music.mp3"}),
        )
        .await;
        client
            .music_generation(json!({"model": "music-2.6", "prompt": "short"}))
            .await
            .unwrap();

        assert_post(
            &server,
            "/v1/music_cover_preprocess",
            json!({"audio_url": "https://example.test/input.mp3"}),
            json!({"preview_url": "https://example.test/preview.mp3"}),
        )
        .await;
        client
            .music_cover_preprocess(json!({"audio_url": "https://example.test/input.mp3"}))
            .await
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        client.list_models().await.unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/models/MiniMax-M3"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "MiniMax-M3"})))
            .mount(&server)
            .await;
        client.retrieve_model("MiniMax-M3").await.unwrap();

        Mock::given(method("GET"))
            .and(path("/anthropic/v1/models"))
            .and(query_param("limit", "10"))
            .and(header("x-api-key", "provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        client
            .list_anthropic_models(Some(10), None, None)
            .await
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/anthropic/v1/models/MiniMax-M3"))
            .and(header("x-api-key", "provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "MiniMax-M3"})))
            .mount(&server)
            .await;
        client.retrieve_anthropic_model("MiniMax-M3").await.unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/models/minimax%2Fcustom"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "minimax/custom"})))
            .mount(&server)
            .await;
        client.retrieve_model("minimax/custom").await.unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/files/upload"))
            .and(header("authorization", "Bearer provider-key"))
            .and(header(
                "content-type",
                "multipart/form-data; boundary=jyowo-minimax-boundary",
            ))
            .and(body_string_contains("voice_clone"))
            .and(body_string_contains("voice.wav"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file_id": "file-1"})))
            .mount(&server)
            .await;
        client
            .file_upload_with_group_id("voice_clone", "voice.wav", b"audio".to_vec(), None)
            .await
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/files/upload"))
            .and(header("authorization", "Bearer provider-key"))
            .and(body_string_contains("bad__name.wav"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file_id": "file-2"})))
            .mount(&server)
            .await;
        client
            .file_upload_with_group_id("voice_clone", "bad\r\nname.wav", b"audio".to_vec(), None)
            .await
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/files/upload"))
            .and(query_param("GroupId", "group-1"))
            .and(header("authorization", "Bearer provider-key"))
            .and(body_string_contains("vision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file_id": "file-3"})))
            .mount(&server)
            .await;
        client
            .file_upload_with_group_id("vision", "image.png", b"image".to_vec(), Some("group-1"))
            .await
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/files/retrieve"))
            .and(query_param("file_id", "file-1"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"file": {"id": "file-1"}})),
            )
            .mount(&server)
            .await;
        client.file_retrieve("file-1").await.unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/files/list"))
            .and(query_param("purpose", "voice_clone"))
            .and(header("authorization", "Bearer provider-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        client.file_list(Some("voice_clone")).await.unwrap();

        assert_post(
            &server,
            "/v1/files/delete",
            json!({"file_id": "file-1"}),
            json!({"deleted": true}),
        )
        .await;
        client.file_delete("file-1").await.unwrap();
    }

    #[tokio::test]
    async fn minimax_provider_client_redacts_secret_from_error_bodies() {
        let server = MockServer::start().await;
        let client = MinimaxApiClient::from_api_key("provider-key").with_base_url(server.uri());

        Mock::given(method("POST"))
            .and(path("/v1/image_generation"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_string("upstream echoed provider-key unexpectedly"),
            )
            .mount(&server)
            .await;

        let error = client
            .image_generation(json!({"model": "image-01", "prompt": "tiny icon"}))
            .await
            .expect_err("provider error should be returned");
        let message = error.to_string();
        assert!(message.contains("[REDACTED]"));
        assert!(!message.contains("provider-key"));
    }

    async fn assert_post(
        server: &MockServer,
        expected_path: &str,
        expected_body: Value,
        body: Value,
    ) {
        Mock::given(method("POST"))
            .and(path(expected_path))
            .and(header("authorization", "Bearer provider-key"))
            .and(body_json(expected_body))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
}
