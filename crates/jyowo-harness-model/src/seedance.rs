//! Volcengine Ark Seedance video generation API client.
//!
//! Source facts (verified 2026-06-29):
//! - API base URL: `https://ark.cn-beijing.volces.com/api/v3`
//! - Auth: `Authorization: Bearer <api_key>`
//! - Create video: `POST /contents/generations/tasks`
//! - Query video: `GET /contents/generations/tasks/{task_id}`
//! - Create request fields: `model`, `content`, optional `resolution`, `ratio`, `duration`, `watermark`
//! - Create success: `{ "id": "<task_id>" }`
//! - Query success: `{ "status": "succeeded", "content": { "video_url": "<url>" }, ... }`
//! - Query pending statuses: `queued`, `running`
//! - Query terminal statuses: `succeeded`, `failed`, `expired`, `cancelled`
//! - Video output MIME: `video/mp4`
//! - Official docs:
//!   - https://www.volcengine.com/docs/82379/1520758
//!   - https://www.volcengine.com/docs/82379/1521309

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
#[cfg(test)]
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use harness_contracts::ModelError;

pub const SEEDANCE_DEFAULT_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";
pub const SEEDANCE_PROVIDER_ID: &str = "doubao";

/// Transport trait for Seedance HTTP calls.
///
/// Implementation in `jyowo-harness-tool` wraps `ToolNetworkBrokerCap`.
#[async_trait]
pub trait SeedanceHttpTransport: Send + Sync + 'static {
    async fn post_json(
        &self,
        url: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), ModelError>;

    async fn get_json(
        &self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> Result<(u16, Vec<u8>), ModelError>;
}

/// Internal transport for Seedance API calls.
#[derive(Clone)]
enum SeedanceTransport {
    /// Production: authorized broker transport.
    Transport(Arc<dyn SeedanceHttpTransport>),
    /// Direct reqwest (test / old credential path).
    #[cfg(test)]
    Direct(reqwest::Client),
}

#[derive(Clone)]
pub struct SeedanceApiClient {
    transport: SeedanceTransport,
    api_key: SecretString,
    base_url: String,
}

impl SeedanceApiClient {
    /// Production constructor using the authorized transport.
    pub fn from_transport(
        transport: Arc<dyn SeedanceHttpTransport>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            transport: SeedanceTransport::Transport(transport),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: SEEDANCE_DEFAULT_BASE_URL.to_owned(),
        }
    }

    /// Old constructor using a direct reqwest client. Only used in tests.
    #[cfg(test)]
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            transport: SeedanceTransport::Direct(
                reqwest::Client::builder()
                    .pool_max_idle_per_host(0)
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            ),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: SEEDANCE_DEFAULT_BASE_URL.to_owned(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub async fn create_video_generation_task(&self, request: Value) -> Result<Value, ModelError> {
        self.post_json("/contents/generations/tasks", request).await
    }

    pub async fn query_video_generation_task(&self, task_id: &str) -> Result<Value, ModelError> {
        self.get_json(
            &format!("/contents/generations/tasks/{}", path_segment(task_id)),
            &[],
        )
        .await
    }

    async fn post_json(&self, path: &str, request: Value) -> Result<Value, ModelError> {
        let body =
            serde_json::to_vec(&request).map_err(|e| ModelError::InvalidRequest(e.to_string()))?;
        let headers = self.transport_headers();
        let url = self.url(path);

        match &self.transport {
            SeedanceTransport::Transport(transport) => {
                let (status, resp_body) = transport.post_json(&url, headers, body).await?;
                transport_response_json(status, &resp_body, self.api_key.expose_secret())
            }
            #[cfg(test)]
            SeedanceTransport::Direct(http) => {
                let response = http
                    .post(url)
                    .headers(self.reqwest_headers()?)
                    .json(&request)
                    .send()
                    .await
                    .map_err(transport_error)?;
                direct_response_json(response, self.api_key.expose_secret()).await
            }
        }
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value, ModelError> {
        let url = self.query_url(path, query)?;
        let headers = self.transport_headers();

        match &self.transport {
            SeedanceTransport::Transport(transport) => {
                let (status, resp_body) = transport.get_json(&url, headers).await?;
                transport_response_json(status, &resp_body, self.api_key.expose_secret())
            }
            #[cfg(test)]
            SeedanceTransport::Direct(http) => {
                let response = http
                    .get(url)
                    .headers(self.reqwest_headers()?)
                    .send()
                    .await
                    .map_err(transport_error)?;
                direct_response_json(response, self.api_key.expose_secret()).await
            }
        }
    }

    fn transport_headers(&self) -> BTreeMap<String, String> {
        let mut headers = BTreeMap::new();
        headers.insert(
            "authorization".to_owned(),
            format!("Bearer {}", self.api_key.expose_secret()),
        );
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        headers
    }

    #[cfg(test)]
    fn reqwest_headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", self.api_key.expose_secret());
        let auth = HeaderValue::from_str(&value)
            .map_err(|error| ModelError::AuthExpired(error.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
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

    fn query_url(&self, path: &str, query: &[(&str, String)]) -> Result<String, ModelError> {
        let mut url = reqwest::Url::parse(&self.url(path))
            .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url.to_string())
    }
}

fn transport_response_json(status: u16, body: &[u8], secret: &str) -> Result<Value, ModelError> {
    let body_str = String::from_utf8_lossy(body).into_owned();
    if !(200..300).contains(&status) {
        return Err(ModelError::ProviderUnavailable(format!(
            "Seedance API request failed with status {status}: {}",
            redact_secret(&body_str, secret)
        )));
    }
    serde_json::from_str(&body_str).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid Seedance API response: {error}"))
    })
}

#[cfg(test)]
async fn direct_response_json(
    response: reqwest::Response,
    secret: &str,
) -> Result<Value, ModelError> {
    let status = response.status();
    let body = response.text().await.map_err(transport_error)?;
    if !status.is_success() {
        return Err(ModelError::ProviderUnavailable(format!(
            "Seedance API request failed with status {status}: {}",
            redact_secret(&body, secret)
        )));
    }
    serde_json::from_str(&body).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid Seedance API response: {error}"))
    })
}

#[cfg(test)]
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
