//! Shared provider media download policy and validation helpers.
//!
//! Pure validation compiles unconditionally. HTTP download uses `reqwest` behind
//! `minimax-tools`.

use harness_contracts::{BudgetMetric, ModelModality, ToolError};
use url::Url;

pub const MAX_MINIMAX_MEDIA_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ProviderMediaBytes {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

pub fn validate_https_media_url(url_str: &str) -> Result<Url, ToolError> {
    let url = Url::parse(url_str.trim())
        .map_err(|_| ToolError::Message("provider media asset URL is malformed".to_owned()))?;
    let allowed_scheme = match url.scheme() {
        "https" => true,
        "http" => url.host_str().is_some_and(|host| {
            matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        }),
        _ => false,
    };
    if !allowed_scheme {
        return Err(ToolError::PermissionDenied(
            "provider media asset URL is not allowed".to_owned(),
        ));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(ToolError::PermissionDenied(
            "provider media asset URL is not allowed".to_owned(),
        ));
    }
    Ok(url)
}

pub fn is_allowed_minimax_media_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    #[cfg(debug_assertions)]
    if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return true;
    }
    matches!(
        host,
        "api.minimaxi.com" | "api.minimax.io" | "api.minimax.chat"
    ) || host.ends_with(".minimaxi.com")
        || host.ends_with(".minimax.io")
        || host.ends_with(".minimax.chat")
}

pub fn is_allowed_provider_media_host(provider_id: &str, url: &Url) -> bool {
    match provider_id {
        "minimax" => is_allowed_minimax_media_host(url),
        "doubao" => is_allowed_doubao_media_host(url),
        _ => false,
    }
}

pub fn is_allowed_doubao_media_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    #[cfg(debug_assertions)]
    if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return true;
    }
    matches!(host, "ark.cn-beijing.volces.com")
        || host.ends_with(".volces.com")
        || host.ends_with(".volccdn.com")
}

pub fn safe_image_mime(value: &str) -> Option<&'static str> {
    let mime = normalized_mime(value);
    match mime.as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

pub fn safe_video_mime(value: &str) -> Option<&'static str> {
    let mime = normalized_mime(value);
    match mime.as_str() {
        "video/mp4" => Some("video/mp4"),
        "video/webm" => Some("video/webm"),
        "video/quicktime" => Some("video/quicktime"),
        _ => None,
    }
}

pub fn safe_audio_mime(value: &str) -> Option<&'static str> {
    let mime = normalized_mime(value);
    match mime.as_str() {
        "audio/mpeg" => Some("audio/mpeg"),
        "audio/mp4" => Some("audio/mp4"),
        "audio/ogg" => Some("audio/ogg"),
        "audio/wav" => Some("audio/wav"),
        "audio/webm" => Some("audio/webm"),
        _ => None,
    }
}

pub fn safe_mime_for_modality(value: &str, modality: ModelModality) -> Option<&'static str> {
    match modality {
        ModelModality::Image => safe_image_mime(value),
        ModelModality::Video => safe_video_mime(value),
        ModelModality::Audio => safe_audio_mime(value),
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => None,
    }
}

pub fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major_brand = &bytes[8..12];
        if major_brand == b"avif" || major_brand == b"avis" {
            return Some("image/avif");
        }
        if bytes
            .get(16..)
            .unwrap_or_default()
            .chunks_exact(4)
            .any(|brand| brand == b"avif" || brand == b"avis")
        {
            return Some("image/avif");
        }
    }
    None
}

pub fn detect_video_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major_brand = &bytes[8..12];
        if major_brand == b"qt  " {
            return Some("video/quicktime");
        }
        return Some("video/mp4");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("video/webm");
    }
    None
}

pub fn detect_audio_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3")
        || bytes.starts_with(&[0xFF, 0xFB])
        || bytes.starts_with(&[0xFF, 0xF3])
        || bytes.starts_with(&[0xFF, 0xF2])
    {
        return Some("audio/mpeg");
    }
    if bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        return Some("audio/wav");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("audio/mp4");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("audio/webm");
    }
    None
}

pub fn detect_mime_for_modality(bytes: &[u8], modality: ModelModality) -> Option<&'static str> {
    match modality {
        ModelModality::Image => detect_image_mime(bytes),
        ModelModality::Video => detect_video_mime(bytes),
        ModelModality::Audio => detect_audio_mime(bytes),
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => None,
    }
}

pub fn validate_media_bytes(
    bytes: &[u8],
    modality: ModelModality,
    declared_mime: Option<&str>,
) -> Result<String, ToolError> {
    let detected_mime = detect_mime_for_modality(bytes, modality).ok_or_else(|| {
        ToolError::Message("provider media payload is not a supported media type".to_owned())
    })?;
    if let Some(declared_mime) = declared_mime {
        let declared_mime = safe_mime_for_modality(declared_mime, modality).ok_or_else(|| {
            ToolError::Message("provider media payload is not a supported media type".to_owned())
        })?;
        if declared_mime != detected_mime {
            return Err(ToolError::Message(
                "provider media payload MIME type does not match bytes".to_owned(),
            ));
        }
    }
    Ok(detected_mime.to_owned())
}

fn normalized_mime(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

#[async_trait::async_trait]
pub trait ProviderMediaDownloader: Send + Sync {
    async fn download(
        &self,
        url: &Url,
        max_bytes: u64,
        modality: ModelModality,
    ) -> Result<ProviderMediaBytes, ToolError>;
}

#[cfg(any(feature = "minimax-tools", feature = "seedance-tools"))]
pub struct ReqwestProviderMediaDownloader;

#[cfg(any(feature = "minimax-tools", feature = "seedance-tools"))]
#[async_trait::async_trait]
impl ProviderMediaDownloader for ReqwestProviderMediaDownloader {
    async fn download(
        &self,
        url: &Url,
        max_bytes: u64,
        modality: ModelModality,
    ) -> Result<ProviderMediaBytes, ToolError> {
        use futures::StreamExt as _;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| {
                ToolError::Message(format!("provider media download setup failed: {error}"))
            })?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| ToolError::Message("provider media download failed".to_owned()))?;
        if !response.status().is_success() {
            return Err(ToolError::Message(
                "provider media download failed".to_owned(),
            ));
        }
        if !response
            .headers()
            .contains_key(reqwest::header::CONTENT_LENGTH)
        {
            return Err(ToolError::Message(
                "provider media download returned unknown content length".to_owned(),
            ));
        }
        let mime_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| safe_mime_for_modality(value, modality))
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                ToolError::Message(
                    "provider media download returned unsupported content type".to_owned(),
                )
            })?;
        let content_length = response.content_length().unwrap_or(0);
        if content_length == 0 {
            return Err(ToolError::Message(
                "provider media download returned empty content length".to_owned(),
            ));
        }
        if content_length > max_bytes {
            return Err(ToolError::ResultTooLarge {
                original: content_length,
                limit: max_bytes,
                metric: BudgetMetric::Bytes,
            });
        }
        let mut bytes = Vec::with_capacity(content_length.min(max_bytes) as usize);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|_| ToolError::Message("provider media download failed".to_owned()))?;
            let next_len = u64::try_from(bytes.len())
                .unwrap_or(u64::MAX)
                .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
            if next_len > max_bytes {
                return Err(ToolError::ResultTooLarge {
                    original: next_len,
                    limit: max_bytes,
                    metric: BudgetMetric::Bytes,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        let mime_type = validate_media_bytes(&bytes, modality, Some(&mime_type))?;
        Ok(ProviderMediaBytes { bytes, mime_type })
    }
}

#[cfg(any(feature = "minimax-tools", feature = "seedance-tools"))]
pub async fn download_provider_https_media(
    provider_id: &str,
    url_str: &str,
    modality: ModelModality,
    downloader: &dyn ProviderMediaDownloader,
    max_bytes: u64,
) -> Result<ProviderMediaBytes, ToolError> {
    let url = validate_https_media_url(url_str)?;
    if !is_allowed_provider_media_host(provider_id, &url) {
        return Err(ToolError::PermissionDenied(
            "provider media asset host is not allowed".to_owned(),
        ));
    }
    downloader.download(&url, max_bytes, modality).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimax_media_host_allowlist_accepts_provider_cdn() {
        let url = Url::parse("https://assets.minimaxi.com/generated/video.mp4").unwrap();
        assert!(is_allowed_minimax_media_host(&url));
    }

    #[test]
    fn minimax_media_host_allowlist_rejects_untrusted_host() {
        let url = Url::parse("https://example.invalid/video.mp4").unwrap();
        assert!(!is_allowed_minimax_media_host(&url));
    }

    #[test]
    fn video_mime_detection_matches_mp4_bytes() {
        let bytes = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00";
        assert_eq!(detect_video_mime(bytes), Some("video/mp4"));
    }

    #[test]
    fn audio_mime_detection_matches_mp3_id3_header() {
        let bytes = b"ID3\x04\x00\x00\x00\x00\x00\x00";
        assert_eq!(detect_audio_mime(bytes), Some("audio/mpeg"));
    }
}
