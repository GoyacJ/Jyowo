//! Shared provider media download policy and validation helpers.
//!
//! Pure validation compiles unconditionally. Provider downloads are routed
//! through the tool network broker.

#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
use harness_contracts::BudgetMetric;
use harness_contracts::{ModelModality, ToolError};
use url::Url;

pub const MAX_MINIMAX_MEDIA_BYTES: u64 = 10 * 1024 * 1024;
pub const SAFE_IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
];
pub const SAFE_VIDEO_MIME_TYPES: &[&str] = &["video/mp4", "video/webm", "video/quicktime"];
pub const SAFE_AUDIO_MIME_TYPES: &[&str] = &[
    "audio/mpeg",
    "audio/mp4",
    "audio/ogg",
    "audio/flac",
    "audio/pcm",
    "audio/wav",
    "audio/webm",
];
pub const SEEDANCE_VIDEO_MIME_TYPES: &[&str] = &["video/mp4"];

#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
#[derive(Debug, Clone, Copy)]
pub struct ProviderMediaDownloadRequest<'a> {
    pub provider_id: &'a str,
    pub operation_id: &'a str,
    pub url: &'a str,
    pub artifact_kind: ModelModality,
    pub expected_mime_types: &'a [&'a str],
    pub max_bytes: u64,
}

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
        "http" => url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "[::1]")),
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
        "gemini" => is_allowed_gemini_media_host(url),
        "doubao" => is_allowed_doubao_media_host(url),
        _ => false,
    }
}

pub fn is_allowed_gemini_media_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    #[cfg(debug_assertions)]
    if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return true;
    }
    matches!(
        host,
        "generativelanguage.googleapis.com"
            | "ai.google.dev"
            | "aistudio.google.com"
            | "storage.googleapis.com"
            | "googleusercontent.com"
    ) || host.ends_with(".googleapis.com")
        || host.ends_with(".googleusercontent.com")
        || host.ends_with(".gstatic.com")
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
        "audio/flac" => Some("audio/flac"),
        "audio/pcm" => Some("audio/pcm"),
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

pub fn safe_mime_types_for_modality(modality: ModelModality) -> &'static [&'static str] {
    match modality {
        ModelModality::Image => SAFE_IMAGE_MIME_TYPES,
        ModelModality::Video => SAFE_VIDEO_MIME_TYPES,
        ModelModality::Audio => SAFE_AUDIO_MIME_TYPES,
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => &[],
    }
}

pub fn provider_media_mime_policy(
    provider_id: &str,
    operation_id: &str,
    artifact_kind: ModelModality,
) -> Option<&'static [&'static str]> {
    match (provider_id, operation_id, artifact_kind) {
        ("minimax", "minimax.image_generation", ModelModality::Image) => {
            Some(SAFE_IMAGE_MIME_TYPES)
        }
        ("gemini", "gemini.image_generation", ModelModality::Image) => Some(SAFE_IMAGE_MIME_TYPES),
        ("gemini", "gemini.video_generation.query", ModelModality::Video) => {
            Some(SAFE_VIDEO_MIME_TYPES)
        }
        ("gemini", "gemini.text_to_speech", ModelModality::Audio) => Some(SAFE_AUDIO_MIME_TYPES),
        (
            "minimax",
            "minimax.video_generation.query" | "minimax.video_template.query",
            ModelModality::Video,
        ) => Some(SAFE_VIDEO_MIME_TYPES),
        (
            "minimax",
            "minimax.text_to_speech.sync"
            | "minimax.text_to_speech.async.query"
            | "minimax.music_generation",
            ModelModality::Audio,
        ) => Some(SAFE_AUDIO_MIME_TYPES),
        ("doubao", "seedance.video_generation.query", ModelModality::Video) => {
            Some(SEEDANCE_VIDEO_MIME_TYPES)
        }
        _ => None,
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
    if bytes.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
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
    let declared_mime = declared_mime
        .map(|declared_mime| {
            safe_mime_for_modality(declared_mime, modality).ok_or_else(|| {
                ToolError::Message(
                    "provider media payload is not a supported media type".to_owned(),
                )
            })
        })
        .transpose()?;
    let detected_mime = detect_mime_for_modality(bytes, modality);
    if let Some(detected_mime) = detected_mime {
        if let Some(declared_mime) = declared_mime {
            if declared_mime != detected_mime {
                return Err(ToolError::Message(
                    "provider media payload MIME type does not match bytes".to_owned(),
                ));
            }
        }
        return Ok(detected_mime.to_owned());
    }
    if modality == ModelModality::Audio && declared_mime == Some("audio/pcm") && !bytes.is_empty() {
        return Ok("audio/pcm".to_owned());
    }
    Err(ToolError::Message(
        "provider media payload is not a supported media type".to_owned(),
    ))
}

fn normalized_mime(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
fn mime_policy_matches(expected: &[&str], policy: &[&str]) -> bool {
    expected.len() == policy.len()
        && expected
            .iter()
            .zip(policy)
            .all(|(expected, policy)| normalized_mime(expected) == *policy)
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

/// Broker-backed provider media downloader.
///
/// Uses `ToolNetworkBrokerCap` and `AuthorizedNetworkPermit` instead of raw
/// reqwest. The broker validates every download URL against the approved host
/// rules before issuing the request.
#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
pub struct BrokerProviderMediaDownloader<'a> {
    broker: std::sync::Arc<dyn crate::ToolNetworkBrokerCap>,
    permit: &'a crate::AuthorizedNetworkPermit,
}

#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
impl<'a> BrokerProviderMediaDownloader<'a> {
    pub fn new(
        broker: std::sync::Arc<dyn crate::ToolNetworkBrokerCap>,
        permit: &'a crate::AuthorizedNetworkPermit,
    ) -> Self {
        Self { broker, permit }
    }
}

#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
#[async_trait::async_trait]
impl ProviderMediaDownloader for BrokerProviderMediaDownloader<'_> {
    async fn download(
        &self,
        url: &Url,
        max_bytes: u64,
        modality: ModelModality,
    ) -> Result<ProviderMediaBytes, ToolError> {
        use std::collections::BTreeMap;

        use crate::{HttpMethod, ToolHttpJsonRequest};

        let url_str = url.to_string();
        let req = ToolHttpJsonRequest {
            method: HttpMethod::Get,
            url: url_str,
            headers: BTreeMap::new(),
            body: None,
            timeout: std::time::Duration::from_secs(30),
            max_response_bytes: max_bytes,
        };
        let resp = self.broker.execute_json(&self.permit, req).await?;

        if resp.status != 200 {
            return Err(ToolError::Message(
                "provider media download failed".to_owned(),
            ));
        }

        let mime_type = resp
            .headers
            .get("content-type")
            .and_then(|value| safe_mime_for_modality(value, modality))
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                ToolError::Message(
                    "provider media download returned unsupported content type".to_owned(),
                )
            })?;

        let body_len = resp.body.len() as u64;
        if body_len == 0 {
            return Err(ToolError::Message(
                "provider media download returned empty body".to_owned(),
            ));
        }
        if body_len > max_bytes {
            return Err(ToolError::ResultTooLarge {
                original: body_len,
                limit: max_bytes,
                metric: BudgetMetric::Bytes,
            });
        }

        let bytes = resp.body.to_vec();
        let mime_type = validate_media_bytes(&bytes, modality, Some(&mime_type))?;
        Ok(ProviderMediaBytes { bytes, mime_type })
    }
}

#[cfg(any(
    feature = "minimax-tools",
    feature = "gemini-tools",
    feature = "seedance-tools"
))]
pub async fn download_provider_https_media(
    request: ProviderMediaDownloadRequest<'_>,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ProviderMediaBytes, ToolError> {
    let mime_policy = provider_media_mime_policy(
        request.provider_id,
        request.operation_id,
        request.artifact_kind,
    )
    .ok_or_else(|| {
        ToolError::PermissionDenied("provider media operation is not allowed".to_owned())
    })?;
    if request.operation_id.trim().is_empty() {
        return Err(ToolError::PermissionDenied(
            "provider media operation is not allowed".to_owned(),
        ));
    }
    if !mime_policy_matches(request.expected_mime_types, mime_policy) {
        return Err(ToolError::PermissionDenied(
            "provider media operation is not allowed".to_owned(),
        ));
    }

    let url = validate_https_media_url(request.url)?;
    if !is_allowed_provider_media_host(request.provider_id, &url) {
        return Err(ToolError::PermissionDenied(
            "provider media asset host is not allowed".to_owned(),
        ));
    }
    let media = downloader
        .download(&url, request.max_bytes, request.artifact_kind)
        .await?;
    let mime_type =
        validate_media_bytes(&media.bytes, request.artifact_kind, Some(&media.mime_type))?;
    if !mime_policy
        .iter()
        .any(|expected| normalized_mime(expected) == mime_type)
    {
        return Err(ToolError::Message(
            "provider media payload MIME type is not allowed for operation".to_owned(),
        ));
    }
    Ok(ProviderMediaBytes {
        bytes: media.bytes,
        mime_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(
        feature = "minimax-tools",
        feature = "gemini-tools",
        feature = "seedance-tools"
    ))]
    struct StaticProviderMediaDownloader {
        bytes: Vec<u8>,
        mime_type: &'static str,
    }

    #[cfg(any(
        feature = "minimax-tools",
        feature = "gemini-tools",
        feature = "seedance-tools"
    ))]
    #[async_trait::async_trait]
    impl ProviderMediaDownloader for StaticProviderMediaDownloader {
        async fn download(
            &self,
            _url: &Url,
            _max_bytes: u64,
            _modality: ModelModality,
        ) -> Result<ProviderMediaBytes, ToolError> {
            Ok(ProviderMediaBytes {
                bytes: self.bytes.clone(),
                mime_type: self.mime_type.to_owned(),
            })
        }
    }

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
    fn gemini_media_host_allowlist_accepts_google_media_hosts() {
        let url = Url::parse("https://generativelanguage.googleapis.com/v1beta/files/abc").unwrap();
        assert!(is_allowed_gemini_media_host(&url));
        let url = Url::parse("https://storage.googleapis.com/gemini/video.mp4").unwrap();
        assert!(is_allowed_gemini_media_host(&url));
    }

    #[test]
    fn gemini_media_host_allowlist_rejects_untrusted_host() {
        let url = Url::parse("https://example.invalid/video.mp4").unwrap();
        assert!(!is_allowed_gemini_media_host(&url));
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

    #[cfg(any(
        feature = "minimax-tools",
        feature = "gemini-tools",
        feature = "seedance-tools"
    ))]
    #[tokio::test]
    async fn provider_media_download_rejects_mime_outside_expected_set() {
        let downloader = StaticProviderMediaDownloader {
            bytes: vec![0x1A, 0x45, 0xDF, 0xA3, 0x01],
            mime_type: "video/webm",
        };

        let error = download_provider_https_media(
            ProviderMediaDownloadRequest {
                provider_id: "doubao",
                operation_id: "seedance.video_generation.query",
                url: "https://ark.cn-beijing.volces.com/video.webm",
                artifact_kind: ModelModality::Video,
                expected_mime_types: SEEDANCE_VIDEO_MIME_TYPES,
                max_bytes: 1024,
            },
            &downloader,
        )
        .await
        .expect_err("media MIME outside operation policy should fail");

        assert!(matches!(
            error,
            ToolError::Message(message)
                if message == "provider media payload MIME type is not allowed for operation"
        ));
    }

    #[cfg(any(
        feature = "minimax-tools",
        feature = "gemini-tools",
        feature = "seedance-tools"
    ))]
    #[tokio::test]
    async fn provider_media_download_rejects_unregistered_policy_tuple() {
        let downloader = StaticProviderMediaDownloader {
            bytes: b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00".to_vec(),
            mime_type: "video/mp4",
        };

        let error = download_provider_https_media(
            ProviderMediaDownloadRequest {
                provider_id: "minimax",
                operation_id: "minimax.unknown.query",
                url: "https://assets.minimaxi.com/video.mp4",
                artifact_kind: ModelModality::Video,
                expected_mime_types: SAFE_VIDEO_MIME_TYPES,
                max_bytes: 1024,
            },
            &downloader,
        )
        .await
        .expect_err("unregistered provider media policy tuple should fail");

        assert!(matches!(
            error,
            ToolError::PermissionDenied(message)
                if message == "provider media operation is not allowed"
        ));
    }

    #[cfg(any(
        feature = "minimax-tools",
        feature = "gemini-tools",
        feature = "seedance-tools"
    ))]
    #[tokio::test]
    async fn provider_media_download_rejects_operation_artifact_mismatch() {
        let downloader = StaticProviderMediaDownloader {
            bytes: b"\x89PNG\r\n\x1A\n".to_vec(),
            mime_type: "image/png",
        };

        let error = download_provider_https_media(
            ProviderMediaDownloadRequest {
                provider_id: "minimax",
                operation_id: "minimax.video_generation.query",
                url: "https://assets.minimaxi.com/image.png",
                artifact_kind: ModelModality::Image,
                expected_mime_types: SAFE_IMAGE_MIME_TYPES,
                max_bytes: 1024,
            },
            &downloader,
        )
        .await
        .expect_err("operation and artifact kind mismatch should fail");

        assert!(matches!(
            error,
            ToolError::PermissionDenied(message)
                if message == "provider media operation is not allowed"
        ));
    }
}
