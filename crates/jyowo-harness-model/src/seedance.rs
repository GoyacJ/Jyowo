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

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use harness_contracts::ModelError;

pub const SEEDANCE_DEFAULT_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";
pub const SEEDANCE_PROVIDER_ID: &str = "doubao";

#[derive(Clone)]
pub struct SeedanceApiClient {
    http: reqwest::Client,
    api_key: SecretString,
    base_url: String,
}

impl SeedanceApiClient {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
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

    async fn response_json(&self, response: reqwest::Response) -> Result<Value, ModelError> {
        let status = response.status();
        let body = response.text().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(ModelError::ProviderUnavailable(format!(
                "Seedance API request failed with status {status}: {}",
                redact_secret(&body, self.api_key.expose_secret())
            )));
        }
        serde_json::from_str(&body).map_err(|error| {
            ModelError::UnexpectedResponse(format!("invalid Seedance API response: {error}"))
        })
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
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

fn path_segment(value: &str) -> String {
    value
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}
