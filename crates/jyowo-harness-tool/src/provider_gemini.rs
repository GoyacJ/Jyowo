use std::collections::BTreeMap;
use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};
use url::Url;

use crate::{
    AuthorizedNetworkPermit, HttpMethod, ToolHttpJsonRequest, ToolHttpResponse,
    ToolNetworkBrokerCap,
};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const API_VERSION: &str = "v1beta";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum GeminiProviderClientError {
    #[error("invalid Gemini API request: {0}")]
    InvalidRequest(String),
    #[error("Gemini API request failed: {0}")]
    ProviderUnavailable(String),
    #[error("invalid Gemini API response: {0}")]
    UnexpectedResponse(String),
}

pub(crate) struct GeminiApiClient {
    transport: Arc<dyn ToolNetworkBrokerCap>,
    permit: AuthorizedNetworkPermit,
    api_key: SecretString,
    base_url: String,
}

impl GeminiApiClient {
    pub(crate) fn from_broker(
        transport: Arc<dyn ToolNetworkBrokerCap>,
        permit: AuthorizedNetworkPermit,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            permit,
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    pub(crate) fn broker_permit(&self) -> &AuthorizedNetworkPermit {
        &self.permit
    }

    #[must_use]
    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub(crate) async fn list_models(&self) -> Result<Value, GeminiProviderClientError> {
        self.get_json("/models", &[]).await
    }

    pub(crate) async fn get_model(
        &self,
        model_name: &str,
    ) -> Result<Value, GeminiProviderClientError> {
        self.get_json(&normalize_resource_path(model_name, "models")?, &[])
            .await
    }

    pub(crate) async fn count_tokens(
        &self,
        mut request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let model = take_model(&mut request)?;
        self.post_json(&format!("/models/{model}:countTokens"), request)
            .await
    }

    pub(crate) async fn upload_file(
        &self,
        file_name: &str,
        mime_type: &str,
        bytes: Vec<u8>,
        metadata: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let mime_type = upload_header_value("mime_type", mime_type)?;
        let content_length = bytes.len().to_string();
        let url = self.upload_url("/files")?;
        let mut start_headers = self.headers_with_json();
        start_headers.insert("x-goog-upload-protocol".to_owned(), "resumable".to_owned());
        start_headers.insert("x-goog-upload-command".to_owned(), "start".to_owned());
        start_headers.insert(
            "x-goog-upload-header-content-length".to_owned(),
            content_length,
        );
        start_headers.insert(
            "x-goog-upload-header-content-type".to_owned(),
            mime_type.clone(),
        );
        let start_body = upload_metadata_body(file_name, metadata)?;
        let start = self
            .request_raw(HttpMethod::Post, url, start_headers, Some(start_body), 1024)
            .await?;
        if !(200..300).contains(&start.status) {
            return self.response_json(start.status, &start.body).await;
        }
        let upload_url = start
            .headers
            .get("x-goog-upload-url")
            .cloned()
            .ok_or_else(|| {
                GeminiProviderClientError::UnexpectedResponse(
                    "Gemini file upload start response did not include x-goog-upload-url"
                        .to_owned(),
                )
            })?;
        let mut upload_headers = self.headers();
        upload_headers.insert("content-type".to_owned(), mime_type);
        upload_headers.insert("x-goog-upload-offset".to_owned(), "0".to_owned());
        upload_headers.insert(
            "x-goog-upload-command".to_owned(),
            "upload, finalize".to_owned(),
        );
        let final_response = self
            .request_raw(
                HttpMethod::Post,
                upload_url,
                upload_headers,
                Some(bytes),
                DEFAULT_MAX_RESPONSE_BYTES,
            )
            .await?;
        self.response_json(final_response.status, &final_response.body)
            .await
    }

    pub(crate) async fn list_files(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<Value, GeminiProviderClientError> {
        let mut query = Vec::new();
        if let Some(page_size) = page_size {
            query.push(("pageSize", page_size.to_string()));
        }
        if let Some(page_token) = page_token {
            query.push(("pageToken", page_token.to_owned()));
        }
        self.get_json("/files", &query).await
    }

    pub(crate) async fn get_file(&self, name: &str) -> Result<Value, GeminiProviderClientError> {
        self.get_json(&normalize_resource_path(name, "files")?, &[])
            .await
    }

    pub(crate) async fn delete_file(&self, name: &str) -> Result<Value, GeminiProviderClientError> {
        self.delete_json(&normalize_resource_path(name, "files")?)
            .await
    }

    pub(crate) async fn create_cached_content(
        &self,
        request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        self.post_json("/cachedContents", request).await
    }

    pub(crate) async fn get_cached_content(
        &self,
        name: &str,
    ) -> Result<Value, GeminiProviderClientError> {
        self.get_json(&normalize_resource_path(name, "cachedContents")?, &[])
            .await
    }

    pub(crate) async fn list_cached_contents(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<Value, GeminiProviderClientError> {
        let mut query = Vec::new();
        if let Some(page_size) = page_size {
            query.push(("pageSize", page_size.to_string()));
        }
        if let Some(page_token) = page_token {
            query.push(("pageToken", page_token.to_owned()));
        }
        self.get_json("/cachedContents", &query).await
    }

    pub(crate) async fn delete_cached_content(
        &self,
        name: &str,
    ) -> Result<Value, GeminiProviderClientError> {
        self.delete_json(&normalize_resource_path(name, "cachedContents")?)
            .await
    }

    pub(crate) async fn embed_content(
        &self,
        mut request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let model = take_model(&mut request)?;
        self.post_json(&format!("/models/{model}:embedContent"), request)
            .await
    }

    pub(crate) async fn batch_embed_contents(
        &self,
        mut request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let model = take_model(&mut request)?;
        self.post_json(&format!("/models/{model}:batchEmbedContents"), request)
            .await
    }

    pub(crate) async fn create_batch(
        &self,
        mut request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let kind = request
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("generate_content")
            .to_owned();
        remove_key(&mut request, "kind");
        let model = take_model(&mut request)?;
        let suffix = match kind.as_str() {
            "generate_content" => "batchGenerateContent",
            "embed_content" => "asyncBatchEmbedContent",
            other => {
                return Err(GeminiProviderClientError::InvalidRequest(format!(
                    "unsupported Gemini batch kind: {other}"
                )));
            }
        };
        self.post_json(&format!("/models/{model}:{suffix}"), request)
            .await
    }

    pub(crate) async fn get_batch(&self, name: &str) -> Result<Value, GeminiProviderClientError> {
        self.get_json(&normalize_resource_path(name, "batches")?, &[])
            .await
    }

    pub(crate) async fn list_batches(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<Value, GeminiProviderClientError> {
        let mut query = Vec::new();
        if let Some(page_size) = page_size {
            query.push(("pageSize", page_size.to_string()));
        }
        if let Some(page_token) = page_token {
            query.push(("pageToken", page_token.to_owned()));
        }
        self.get_json("/batches", &query).await
    }

    pub(crate) async fn cancel_batch(
        &self,
        name: &str,
    ) -> Result<Value, GeminiProviderClientError> {
        self.post_json(
            &format!("{}:cancel", normalize_resource_path(name, "batches")?),
            json!({}),
        )
        .await
    }

    pub(crate) async fn generate_image(
        &self,
        request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        self.generate_content(request).await
    }

    pub(crate) async fn generate_video(
        &self,
        mut request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let model = take_model(&mut request)?;
        self.post_json(&format!("/models/{model}:predictLongRunning"), request)
            .await
    }

    pub(crate) async fn query_video(&self, name: &str) -> Result<Value, GeminiProviderClientError> {
        self.get_json(&normalize_resource_path(name, "operations")?, &[])
            .await
    }

    pub(crate) async fn text_to_speech(
        &self,
        request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        self.generate_content(request).await
    }

    async fn generate_content(
        &self,
        mut request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let model = take_model(&mut request)?;
        self.post_json(&format!("/models/{model}:generateContent"), request)
            .await
    }

    async fn get_json(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, GeminiProviderClientError> {
        self.request_json(
            HttpMethod::Get,
            self.query_url(path, query)?,
            self.headers_with_json(),
            None,
        )
        .await
    }

    async fn post_json(
        &self,
        path: &str,
        request: Value,
    ) -> Result<Value, GeminiProviderClientError> {
        let body = serde_json::to_vec(&request)
            .map_err(|error| GeminiProviderClientError::InvalidRequest(error.to_string()))?;
        let resp = self
            .request_raw(
                HttpMethod::Post,
                self.query_url(path, &[])?,
                self.headers_with_json(),
                Some(body),
                DEFAULT_MAX_RESPONSE_BYTES,
            )
            .await?;
        self.response_json(resp.status, &resp.body).await
    }

    async fn delete_json(&self, path: &str) -> Result<Value, GeminiProviderClientError> {
        let resp = self
            .request_raw(
                HttpMethod::Delete,
                self.query_url(path, &[])?,
                self.headers_with_json(),
                None,
                DEFAULT_MAX_RESPONSE_BYTES,
            )
            .await?;
        self.response_json(resp.status, &resp.body).await
    }

    async fn request_json(
        &self,
        method: HttpMethod,
        url: String,
        headers: BTreeMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<Value, GeminiProviderClientError> {
        let resp = self
            .request_raw(method, url, headers, body, DEFAULT_MAX_RESPONSE_BYTES)
            .await?;
        self.response_json(resp.status, &resp.body).await
    }

    async fn request_raw(
        &self,
        method: HttpMethod,
        url: String,
        headers: BTreeMap<String, String>,
        body: Option<Vec<u8>>,
        max_response_bytes: u64,
    ) -> Result<ToolHttpResponse, GeminiProviderClientError> {
        let req = ToolHttpJsonRequest {
            method,
            url,
            headers,
            body,
            timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_response_bytes,
        };
        self.transport
            .execute_json(&self.permit, req)
            .await
            .map_err(|error| GeminiProviderClientError::ProviderUnavailable(error.to_string()))
    }

    async fn response_json(
        &self,
        status: u16,
        body: &[u8],
    ) -> Result<Value, GeminiProviderClientError> {
        let body_str = String::from_utf8_lossy(body).into_owned();
        if !(200..300).contains(&status) {
            return Err(GeminiProviderClientError::ProviderUnavailable(format!(
                "Gemini API request failed with status {status}: {}",
                redact_secret(&body_str, self.api_key.expose_secret())
            )));
        }
        if body_str.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&body_str)
            .map_err(|error| GeminiProviderClientError::UnexpectedResponse(error.to_string()))
    }

    fn headers(&self) -> BTreeMap<String, String> {
        let mut headers = BTreeMap::new();
        headers.insert(
            "x-goog-api-key".to_owned(),
            self.api_key.expose_secret().to_owned(),
        );
        headers
    }

    fn headers_with_json(&self) -> BTreeMap<String, String> {
        let mut headers = self.headers();
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        headers
    }

    fn query_url(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<String, GeminiProviderClientError> {
        let mut url = Url::parse(&self.api_url(path)?)
            .map_err(|error| GeminiProviderClientError::InvalidRequest(error.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url.to_string())
    }

    fn api_url(&self, path: &str) -> Result<String, GeminiProviderClientError> {
        Ok(format!(
            "{}/{API_VERSION}{}",
            normalized_gemini_api_base_url(&self.base_url)?,
            if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            }
        ))
    }

    fn upload_url(&self, path: &str) -> Result<String, GeminiProviderClientError> {
        Ok(format!(
            "{}/upload/{API_VERSION}{}",
            normalized_gemini_api_base_url(&self.base_url)?,
            if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            }
        ))
    }
}

fn take_model(request: &mut Value) -> Result<String, GeminiProviderClientError> {
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| {
            GeminiProviderClientError::InvalidRequest(
                "Gemini request must include model".to_owned(),
            )
        })?
        .trim_start_matches("models/")
        .to_owned();
    remove_key(request, "model");
    Ok(model)
}

fn remove_key(value: &mut Value, key: &str) {
    if let Some(object) = value.as_object_mut() {
        object.remove(key);
    }
}

fn normalize_resource_path(
    name: &str,
    collection: &str,
) -> Result<String, GeminiProviderClientError> {
    let name = name.trim().trim_start_matches('/');
    if name.is_empty()
        || name.contains("..")
        || name.contains("//")
        || name.split('/').any(str::is_empty)
    {
        return Err(GeminiProviderClientError::InvalidRequest(
            "Gemini resource name is invalid".to_owned(),
        ));
    }
    if name == collection {
        return Err(GeminiProviderClientError::InvalidRequest(
            "Gemini resource name must include an id".to_owned(),
        ));
    }
    if name.starts_with(&format!("{collection}/")) {
        Ok(format!("/{name}"))
    } else {
        Ok(format!("/{collection}/{name}"))
    }
}

fn upload_metadata_body(
    file_name: &str,
    metadata: Value,
) -> Result<Vec<u8>, GeminiProviderClientError> {
    let file_name = upload_file_name(file_name)?;
    let metadata = if metadata.is_null() {
        json!({ "file": { "displayName": file_name } })
    } else {
        metadata
    };
    serde_json::to_vec(&metadata)
        .map_err(|error| GeminiProviderClientError::InvalidRequest(error.to_string()))
}

fn upload_file_name(value: &str) -> Result<String, GeminiProviderClientError> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(|ch| ch.is_control()) {
        return Err(GeminiProviderClientError::InvalidRequest(
            "Gemini file_name is invalid".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn upload_header_value(field: &str, value: &str) -> Result<String, GeminiProviderClientError> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(|ch| ch.is_control()) {
        return Err(GeminiProviderClientError::InvalidRequest(format!(
            "Gemini {field} is invalid"
        )));
    }
    Ok(value.to_owned())
}

fn normalized_gemini_api_base_url(value: &str) -> Result<String, GeminiProviderClientError> {
    let value = value.trim().trim_end_matches('/');
    let url = Url::parse(value).map_err(|_| {
        GeminiProviderClientError::InvalidRequest("Gemini base URL is invalid".to_owned())
    })?;
    if !matches!(url.scheme(), "https" | "http")
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(GeminiProviderClientError::InvalidRequest(
            "Gemini base URL is invalid".to_owned(),
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        GeminiProviderClientError::InvalidRequest("Gemini base URL is invalid".to_owned())
    })?;
    let is_allowed_host = host.eq_ignore_ascii_case("generativelanguage.googleapis.com");
    #[cfg(debug_assertions)]
    let is_allowed_host = is_allowed_host
        || host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !is_allowed_host {
        return Err(GeminiProviderClientError::InvalidRequest(
            "Gemini base URL host is not allowed".to_owned(),
        ));
    }
    if url.scheme() == "http" {
        #[cfg(debug_assertions)]
        {
            let loopback = host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback());
            if loopback {
                return Ok(value.to_owned());
            }
        }
        return Err(GeminiProviderClientError::InvalidRequest(
            "Gemini base URL must use https".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_owned()
    } else {
        text.replace(secret, "[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_resource_path_rejects_collection_without_id() {
        let error = normalize_resource_path("models", "models").unwrap_err();

        assert!(matches!(
            error,
            GeminiProviderClientError::InvalidRequest(message)
                if message.contains("must include an id")
        ));
    }

    #[test]
    fn upload_metadata_body_rejects_control_characters() {
        let error = upload_metadata_body(
            "demo\r\nx.txt",
            json!({ "file": { "displayName": "demo" } }),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            GeminiProviderClientError::InvalidRequest(message)
                if message.contains("file_name")
        ));
    }
}
