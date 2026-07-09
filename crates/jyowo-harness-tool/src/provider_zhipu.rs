use std::collections::BTreeMap;
use std::sync::Arc;

use base64::{engine::general_purpose, Engine as _};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use url::Url;

use crate::{AuthorizedNetworkPermit, HttpMethod, ToolHttpJsonRequest, ToolNetworkBrokerCap};

const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ZhipuProviderClientError {
    #[error("invalid Zhipu API request: {0}")]
    InvalidRequest(String),
    #[error("Zhipu API request failed: {0}")]
    ProviderUnavailable(String),
    #[error("invalid Zhipu API response: {0}")]
    UnexpectedResponse(String),
}

pub(crate) struct ZhipuApiClient {
    transport: Arc<dyn ToolNetworkBrokerCap>,
    permit: AuthorizedNetworkPermit,
    api_key: SecretString,
    base_url: String,
}

pub(crate) struct ZhipuBinaryResponse {
    pub(crate) content_type: String,
    pub(crate) body: Vec<u8>,
}

impl ZhipuApiClient {
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

    #[must_use]
    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub(crate) async fn image_generation(
        &self,
        request: Value,
    ) -> Result<Value, ZhipuProviderClientError> {
        self.post_json("/images/generations", request).await
    }

    pub(crate) async fn image_generation_async(
        &self,
        request: Value,
    ) -> Result<Value, ZhipuProviderClientError> {
        self.post_json("/async/images/generations", request).await
    }

    pub(crate) async fn video_generation(
        &self,
        request: Value,
    ) -> Result<Value, ZhipuProviderClientError> {
        self.post_json("/videos/generations", request).await
    }

    pub(crate) async fn text_to_speech(
        &self,
        request: Value,
    ) -> Result<ZhipuBinaryResponse, ZhipuProviderClientError> {
        self.post_binary("/audio/speech", request).await
    }

    pub(crate) async fn speech_to_text(
        &self,
        request: Value,
    ) -> Result<Value, ZhipuProviderClientError> {
        self.post_multipart_json("/audio/transcriptions", request)
            .await
    }

    pub(crate) async fn async_result(
        &self,
        task_id: &str,
    ) -> Result<Value, ZhipuProviderClientError> {
        self.get_json(&format!("/async-result/{}", path_segment(task_id)))
            .await
    }

    async fn post_json(
        &self,
        path: &str,
        request: Value,
    ) -> Result<Value, ZhipuProviderClientError> {
        let body = serde_json::to_vec(&request)
            .map_err(|error| ZhipuProviderClientError::InvalidRequest(error.to_string()))?;
        let response = self
            .transport
            .execute_json(
                &self.permit,
                ToolHttpJsonRequest {
                    method: HttpMethod::Post,
                    url: self.url(path)?,
                    headers: self.headers(),
                    body: Some(body),
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                },
            )
            .await
            .map_err(|error| ZhipuProviderClientError::ProviderUnavailable(error.to_string()))?;
        self.response_json_or_text(response.status, &response.headers, &response.body)
    }

    async fn get_json(&self, path: &str) -> Result<Value, ZhipuProviderClientError> {
        let response = self
            .transport
            .execute_json(
                &self.permit,
                ToolHttpJsonRequest {
                    method: HttpMethod::Get,
                    url: self.url(path)?,
                    headers: self.headers(),
                    body: None,
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                },
            )
            .await
            .map_err(|error| ZhipuProviderClientError::ProviderUnavailable(error.to_string()))?;
        self.response_json_or_text(response.status, &response.headers, &response.body)
    }

    async fn post_binary(
        &self,
        path: &str,
        request: Value,
    ) -> Result<ZhipuBinaryResponse, ZhipuProviderClientError> {
        let body = serde_json::to_vec(&request)
            .map_err(|error| ZhipuProviderClientError::InvalidRequest(error.to_string()))?;
        let response = self
            .transport
            .execute_json(
                &self.permit,
                ToolHttpJsonRequest {
                    method: HttpMethod::Post,
                    url: self.url(path)?,
                    headers: self.headers(),
                    body: Some(body),
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                },
            )
            .await
            .map_err(|error| ZhipuProviderClientError::ProviderUnavailable(error.to_string()))?;
        self.response_binary(response.status, &response.headers, &response.body)
    }

    async fn post_multipart_json(
        &self,
        path: &str,
        request: Value,
    ) -> Result<Value, ZhipuProviderClientError> {
        let multipart = multipart_body(&request)?;
        let mut headers = self.headers_without_content_type();
        headers.insert(
            "content-type".to_owned(),
            format!("multipart/form-data; boundary={}", multipart.boundary),
        );
        let response = self
            .transport
            .execute_json(
                &self.permit,
                ToolHttpJsonRequest {
                    method: HttpMethod::Post,
                    url: self.url(path)?,
                    headers,
                    body: Some(multipart.body),
                    timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                },
            )
            .await
            .map_err(|error| ZhipuProviderClientError::ProviderUnavailable(error.to_string()))?;
        self.response_json_or_text(response.status, &response.headers, &response.body)
    }

    fn response_json(&self, status: u16, body: &[u8]) -> Result<Value, ZhipuProviderClientError> {
        let body_text = String::from_utf8_lossy(body).into_owned();
        if !(200..300).contains(&status) {
            return Err(ZhipuProviderClientError::ProviderUnavailable(format!(
                "Zhipu API request failed with status {status}: {}",
                redact_secret(&body_text, self.api_key.expose_secret())
            )));
        }
        serde_json::from_slice(body).map_err(|error| {
            ZhipuProviderClientError::UnexpectedResponse(format!(
                "{}: {}",
                error,
                redact_secret(&body_text, self.api_key.expose_secret())
            ))
        })
    }

    fn response_json_or_text(
        &self,
        status: u16,
        headers: &BTreeMap<String, String>,
        body: &[u8],
    ) -> Result<Value, ZhipuProviderClientError> {
        if !(200..300).contains(&status) {
            return self.response_json(status, body);
        }
        let content_type = headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value)
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if content_type.contains("application/json") || content_type.is_empty() {
            return self.response_json(status, body);
        }
        Ok(serde_json::json!({
            "content_type": content_type_header(headers).unwrap_or_default(),
            "body": String::from_utf8_lossy(body).into_owned(),
        }))
    }

    fn headers(&self) -> BTreeMap<String, String> {
        let mut headers = self.headers_without_content_type();
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        headers
    }

    fn headers_without_content_type(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(
            "authorization".to_owned(),
            format!("Bearer {}", self.api_key.expose_secret()),
        )])
    }

    fn response_binary(
        &self,
        status: u16,
        headers: &BTreeMap<String, String>,
        body: &[u8],
    ) -> Result<ZhipuBinaryResponse, ZhipuProviderClientError> {
        if !(200..300).contains(&status) {
            let body_text = String::from_utf8_lossy(body).into_owned();
            return Err(ZhipuProviderClientError::ProviderUnavailable(format!(
                "Zhipu API request failed with status {status}: {}",
                redact_secret(&body_text, self.api_key.expose_secret())
            )));
        }
        let content_type = headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value)
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("application/octet-stream")
            .to_owned();
        Ok(ZhipuBinaryResponse {
            content_type,
            body: body.to_vec(),
        })
    }

    fn url(&self, path: &str) -> Result<String, ZhipuProviderClientError> {
        Self::join_url(&self.base_url, path)
    }

    fn join_url(base_url: &str, path: &str) -> Result<String, ZhipuProviderClientError> {
        let base = base_url.trim_end_matches('/');
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Url::parse(&format!("{base}{path}"))
            .map(|url| url.to_string())
            .map_err(|error| ZhipuProviderClientError::InvalidRequest(error.to_string()))
    }
}

fn content_type_header(headers: &BTreeMap<String, String>) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.clone())
}

fn redact_secret(body: &str, secret: &str) -> String {
    if secret.is_empty() {
        body.to_owned()
    } else {
        body.replace(secret, "[REDACTED]")
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

struct MultipartBody {
    boundary: String,
    body: Vec<u8>,
}

fn multipart_body(request: &Value) -> Result<MultipartBody, ZhipuProviderClientError> {
    let object = request.as_object().ok_or_else(|| {
        ZhipuProviderClientError::InvalidRequest("multipart request must be an object".to_owned())
    })?;
    let boundary = multipart_boundary();
    let mut body = Vec::new();
    for (key, value) in object {
        if object.contains_key("file") && key == "file_base64" {
            continue;
        }
        let part = multipart_value(key, value)?;
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(part.headers.as_bytes());
        body.extend_from_slice(&part.body);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(MultipartBody { boundary, body })
}

struct MultipartPart {
    headers: String,
    body: Vec<u8>,
}

fn multipart_value(key: &str, value: &Value) -> Result<MultipartPart, ZhipuProviderClientError> {
    if key == "file" {
        return multipart_file_part(value);
    }
    let value = match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value)
            .map_err(|error| ZhipuProviderClientError::InvalidRequest(error.to_string()))?,
    };
    Ok(MultipartPart {
        headers: format!(
            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
            multipart_header_value(key)
        ),
        body: value.into_bytes(),
    })
}

fn multipart_file_part(value: &Value) -> Result<MultipartPart, ZhipuProviderClientError> {
    let object = value.as_object().ok_or_else(|| {
        ZhipuProviderClientError::InvalidRequest(
            "file must be an object with base64 audio content".to_owned(),
        )
    })?;
    let filename = object
        .get("filename")
        .or_else(|| object.get("file_name"))
        .and_then(Value::as_str)
        .unwrap_or("audio.wav");
    let content_type = object
        .get("content_type")
        .or_else(|| object.get("contentType"))
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream");
    let encoded = object
        .get("base64")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ZhipuProviderClientError::InvalidRequest("file.base64 string is required".to_owned())
        })?;
    let body = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| ZhipuProviderClientError::InvalidRequest(error.to_string()))?;
    Ok(MultipartPart {
        headers: format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
            multipart_header_value(filename),
            multipart_header_value(content_type)
        ),
        body,
    })
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

fn multipart_boundary() -> String {
    let mut bytes = [0_u8; 16];
    if SystemRandom::new().fill(&mut bytes).is_ok() {
        return format!("jyowo-zhipu-{}", hex_bytes(&bytes));
    }
    let fallback = blake3::hash(
        format!(
            "{}-{:?}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            std::thread::current().id()
        )
        .as_bytes(),
    );
    format!("jyowo-zhipu-{}", hex_bytes(&fallback.as_bytes()[..16]))
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_rejects_invalid_base_url_without_panicking() {
        let error = ZhipuApiClient::join_url("not a url", "/images/generations").unwrap_err();
        assert!(matches!(error, ZhipuProviderClientError::InvalidRequest(_)));
    }

    #[test]
    fn url_joins_base_url_and_path() {
        assert_eq!(
            ZhipuApiClient::join_url("https://example.test/api/paas/v4/", "/images/generations")
                .unwrap(),
            "https://example.test/api/paas/v4/images/generations"
        );
    }

    #[test]
    fn path_segment_escapes_task_id() {
        assert_eq!(path_segment("task/id ?"), "task%2Fid%20%3F");
    }

    #[test]
    fn multipart_body_serializes_official_stt_fields() {
        let body = multipart_body(&serde_json::json!({
            "model": "glm-asr-2512",
            "file_base64": "abc",
            "stream": false,
            "hotwords": ["智谱"],
        }))
        .unwrap();
        assert!(body.boundary.starts_with("jyowo-zhipu-"));
        assert_ne!(body.boundary, "jyowo-zhipu-boundary");
        let body = String::from_utf8(body.body).unwrap();
        assert!(body.contains("name=\"model\"\r\n\r\nglm-asr-2512"));
        assert!(body.contains("name=\"file_base64\"\r\n\r\nabc"));
        assert!(body.contains("name=\"stream\"\r\n\r\nfalse"));
        assert!(body.contains("name=\"hotwords\"\r\n\r\n[\"智谱\"]"));
        assert!(body.contains("--jyowo-zhipu-"));
    }

    #[test]
    fn multipart_body_serializes_official_binary_file_field() {
        let body = multipart_body(&serde_json::json!({
            "model": "glm-asr-2512",
            "file": {
                "filename": "audio.wav",
                "content_type": "audio/wav",
                "base64": "UklGRg=="
            },
            "file_base64": "ignored-when-file-is-present",
        }))
        .unwrap();
        let body = String::from_utf8(body.body).unwrap();
        assert!(
            body.contains("Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"")
        );
        assert!(body.contains("Content-Type: audio/wav\r\n\r\nRIFF"));
        assert!(!body.contains("name=\"file_base64\""));
    }
}
