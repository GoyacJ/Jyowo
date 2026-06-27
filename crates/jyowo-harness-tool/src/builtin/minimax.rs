use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use futures::{stream, StreamExt as _};
use harness_contracts::{
    BlobMeta, BlobRetention, BlobWriterCap, BudgetMetric, DecisionScope, PermissionSubject,
    ProviderCredential, ProviderCredentialResolveContext, ProviderCredentialResolverCap,
    ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolResult, ToolResultPart,
};
use harness_model::MinimaxApiClient;
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use url::Url;

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const MINIMAX_PROVIDER_ID: &str = "minimax";
const MAX_MINIMAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

macro_rules! minimax_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: descriptor($name, $display_name, $description),
                }
            }
        }

        #[async_trait]
        impl Tool for $type_name {
            fn descriptor(&self) -> &ToolDescriptor {
                &self.descriptor
            }

            async fn validate(
                &self,
                input: &Value,
                _ctx: &ToolContext,
            ) -> Result<(), ValidationError> {
                request(input)?;
                Ok(())
            }

            async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
                minimax_network_permission(ctx).await
            }

            async fn execute(
                &self,
                input: Value,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                Ok(execute_request(input, ctx, |client, request| async move {
                    client.$operation(request).await.map_err(model_error)
                }))
            }
        }
    };
}

macro_rules! minimax_image_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: image_descriptor($name, $display_name, $description),
                }
            }
        }

        #[async_trait]
        impl Tool for $type_name {
            fn descriptor(&self) -> &ToolDescriptor {
                &self.descriptor
            }

            async fn validate(
                &self,
                input: &Value,
                _ctx: &ToolContext,
            ) -> Result<(), ValidationError> {
                request(input)?;
                Ok(())
            }

            async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
                minimax_network_permission(ctx).await
            }

            async fn execute(
                &self,
                input: Value,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                Ok(execute_image_request(input, ctx))
            }
        }
    };
}

macro_rules! minimax_task_query_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: descriptor($name, $display_name, $description),
                }
            }
        }

        #[async_trait]
        impl Tool for $type_name {
            fn descriptor(&self) -> &ToolDescriptor {
                &self.descriptor
            }

            async fn validate(
                &self,
                input: &Value,
                _ctx: &ToolContext,
            ) -> Result<(), ValidationError> {
                let request = request(input)?;
                required_string(&request, "task_id")?;
                Ok(())
            }

            async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
                minimax_network_permission(ctx).await
            }

            async fn execute(
                &self,
                input: Value,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                Ok(execute_request(input, ctx, |client, request| async move {
                    let task_id = required_string(&request, "task_id").map_err(validation_error)?;
                    client.$operation(&task_id).await.map_err(model_error)
                }))
            }
        }
    };
}

macro_rules! minimax_string_arg_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident, $field:literal) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: descriptor($name, $display_name, $description),
                }
            }
        }

        #[async_trait]
        impl Tool for $type_name {
            fn descriptor(&self) -> &ToolDescriptor {
                &self.descriptor
            }

            async fn validate(
                &self,
                input: &Value,
                _ctx: &ToolContext,
            ) -> Result<(), ValidationError> {
                let request = request(input)?;
                required_string(&request, $field)?;
                Ok(())
            }

            async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
                minimax_network_permission(ctx).await
            }

            async fn execute(
                &self,
                input: Value,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                Ok(execute_request(input, ctx, |client, request| async move {
                    let value = required_string(&request, $field).map_err(validation_error)?;
                    client.$operation(&value).await.map_err(model_error)
                }))
            }
        }
    };
}

minimax_image_tool!(
    MiniMaxTextToImageTool,
    "MiniMaxTextToImage",
    "MiniMax text to image",
    "Generate an image with MiniMax image generation."
);
minimax_image_tool!(
    MiniMaxImageToImageTool,
    "MiniMaxImageToImage",
    "MiniMax image to image",
    "Generate or transform an image with MiniMax image generation."
);
minimax_tool!(
    MiniMaxTextToVideoTool,
    "MiniMaxTextToVideo",
    "MiniMax text to video",
    "Create a MiniMax video generation task from text.",
    video_generation
);
minimax_tool!(
    MiniMaxImageToVideoTool,
    "MiniMaxImageToVideo",
    "MiniMax image to video",
    "Create a MiniMax video generation task from an image reference.",
    video_generation
);
minimax_tool!(
    MiniMaxFirstLastFrameToVideoTool,
    "MiniMaxFirstLastFrameToVideo",
    "MiniMax first last frame video",
    "Create a MiniMax video task from first and last frame references.",
    video_generation
);
minimax_tool!(
    MiniMaxSubjectReferenceVideoTool,
    "MiniMaxSubjectReferenceVideo",
    "MiniMax subject reference video",
    "Create a MiniMax video task with subject reference inputs.",
    video_generation
);
minimax_task_query_tool!(
    MiniMaxVideoGenerationQueryTool,
    "MiniMaxVideoGenerationQuery",
    "MiniMax video generation query",
    "Query a MiniMax video generation task.",
    query_video_generation
);
minimax_tool!(
    MiniMaxVideoTemplateTool,
    "MiniMaxVideoTemplate",
    "MiniMax video template",
    "Create a MiniMax video template generation task.",
    video_template_generation
);
minimax_task_query_tool!(
    MiniMaxVideoTemplateQueryTool,
    "MiniMaxVideoTemplateQuery",
    "MiniMax video template query",
    "Query a MiniMax video template generation task.",
    query_video_template_generation
);
minimax_tool!(
    MiniMaxTextToSpeechTool,
    "MiniMaxTextToSpeech",
    "MiniMax text to speech",
    "Generate speech with MiniMax synchronous TTS.",
    text_to_speech
);
minimax_tool!(
    MiniMaxTextToSpeechAsyncTool,
    "MiniMaxTextToSpeechAsync",
    "MiniMax async text to speech",
    "Create a MiniMax async long-form TTS task.",
    text_to_speech_async
);
minimax_task_query_tool!(
    MiniMaxTextToSpeechAsyncQueryTool,
    "MiniMaxTextToSpeechAsyncQuery",
    "MiniMax async text to speech query",
    "Query a MiniMax async TTS task.",
    query_text_to_speech_async
);
minimax_tool!(
    MiniMaxVoiceCloneTool,
    "MiniMaxVoiceClone",
    "MiniMax voice clone",
    "Clone a voice with MiniMax voice cloning.",
    voice_clone
);
minimax_tool!(
    MiniMaxVoiceDesignTool,
    "MiniMaxVoiceDesign",
    "MiniMax voice design",
    "Design a voice with MiniMax voice design.",
    voice_design
);
minimax_tool!(
    MiniMaxListVoicesTool,
    "MiniMaxListVoices",
    "MiniMax list voices",
    "List MiniMax voices.",
    get_voice
);
minimax_tool!(
    MiniMaxDeleteVoiceTool,
    "MiniMaxDeleteVoice",
    "MiniMax delete voice",
    "Delete a MiniMax voice.",
    delete_voice
);
minimax_tool!(
    MiniMaxLyricsGenerationTool,
    "MiniMaxLyricsGeneration",
    "MiniMax lyrics generation",
    "Generate lyrics with MiniMax music APIs.",
    lyrics_generation
);
minimax_tool!(
    MiniMaxMusicGenerationTool,
    "MiniMaxMusicGeneration",
    "MiniMax music generation",
    "Generate music with MiniMax music APIs.",
    music_generation
);
minimax_tool!(
    MiniMaxMusicCoverPreprocessTool,
    "MiniMaxMusicCoverPreprocess",
    "MiniMax music cover preprocess",
    "Preprocess source audio for MiniMax music cover generation.",
    music_cover_preprocess
);
minimax_tool!(
    MiniMaxResponsesTool,
    "MiniMaxResponses",
    "MiniMax responses",
    "Call MiniMax responses endpoint.",
    responses
);
minimax_tool!(
    MiniMaxResponsesInputTokensTool,
    "MiniMaxResponsesInputTokens",
    "MiniMax responses input tokens",
    "Count MiniMax responses input tokens.",
    responses_input_tokens
);
minimax_tool!(
    MiniMaxAnthropicMessagesTool,
    "MiniMaxAnthropicMessages",
    "MiniMax Anthropic messages",
    "Call MiniMax Anthropic-compatible messages endpoint.",
    anthropic_messages
);
minimax_tool!(
    MiniMaxAnthropicCountTokensTool,
    "MiniMaxAnthropicCountTokens",
    "MiniMax Anthropic count tokens",
    "Call MiniMax Anthropic-compatible count tokens endpoint.",
    anthropic_count_tokens
);

#[derive(Clone)]
pub struct MiniMaxFileUploadTool {
    descriptor: ToolDescriptor,
}

impl Default for MiniMaxFileUploadTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "MiniMaxFileUpload",
                "MiniMax file upload",
                "Upload a file to MiniMax files API.",
            ),
        }
    }
}

#[async_trait]
impl Tool for MiniMaxFileUploadTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let request = request(input)?;
        file_upload_request(&request)?;
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
        minimax_network_permission(ctx).await
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(execute_request(input, ctx, |client, request| async move {
            let upload = file_upload_request(&request).map_err(validation_error)?;
            client
                .file_upload_with_group_id(
                    &upload.purpose,
                    &upload.file_name,
                    upload.bytes,
                    upload.group_id.as_deref(),
                )
                .await
                .map_err(model_error)
        }))
    }
}

minimax_string_arg_tool!(
    MiniMaxFileRetrieveTool,
    "MiniMaxFileRetrieve",
    "MiniMax file retrieve",
    "Retrieve MiniMax file metadata.",
    file_retrieve,
    "file_id"
);
minimax_string_arg_tool!(
    MiniMaxFileDeleteTool,
    "MiniMaxFileDelete",
    "MiniMax file delete",
    "Delete a MiniMax file.",
    file_delete,
    "file_id"
);
minimax_string_arg_tool!(
    MiniMaxModelRetrieveTool,
    "MiniMaxModelRetrieve",
    "MiniMax model retrieve",
    "Retrieve MiniMax model metadata.",
    retrieve_model,
    "model_id"
);
minimax_string_arg_tool!(
    MiniMaxAnthropicModelRetrieveTool,
    "MiniMaxAnthropicModelRetrieve",
    "MiniMax Anthropic model retrieve",
    "Retrieve MiniMax Anthropic-compatible model metadata.",
    retrieve_anthropic_model,
    "model_id"
);

#[derive(Clone)]
pub struct MiniMaxFileListTool {
    descriptor: ToolDescriptor,
}

impl Default for MiniMaxFileListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "MiniMaxFileList",
                "MiniMax file list",
                "List MiniMax files.",
            ),
        }
    }
}

#[async_trait]
impl Tool for MiniMaxFileListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
        minimax_network_permission(ctx).await
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(execute_request(input, ctx, |client, request| async move {
            let purpose = optional_string(&request, "purpose").map_err(validation_error)?;
            client
                .file_list(purpose.as_deref())
                .await
                .map_err(model_error)
        }))
    }
}

#[derive(Clone)]
pub struct MiniMaxModelsListTool {
    descriptor: ToolDescriptor,
}

impl Default for MiniMaxModelsListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "MiniMaxModelsList",
                "MiniMax models list",
                "List MiniMax models.",
            ),
        }
    }
}

#[async_trait]
impl Tool for MiniMaxModelsListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
        minimax_network_permission(ctx).await
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(execute_request(input, ctx, |client, _request| async move {
            client.list_models().await.map_err(model_error)
        }))
    }
}

#[derive(Clone)]
pub struct MiniMaxAnthropicModelsListTool {
    descriptor: ToolDescriptor,
}

impl Default for MiniMaxAnthropicModelsListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "MiniMaxAnthropicModelsList",
                "MiniMax Anthropic models list",
                "List MiniMax Anthropic-compatible models.",
            ),
        }
    }
}

#[async_trait]
impl Tool for MiniMaxAnthropicModelsListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
        minimax_network_permission(ctx).await
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(execute_request(input, ctx, |client, request| async move {
            let limit = optional_u32(&request, "limit").map_err(validation_error)?;
            let after_id = optional_string(&request, "after_id").map_err(validation_error)?;
            let before_id = optional_string(&request, "before_id").map_err(validation_error)?;
            client
                .list_anthropic_models(limit, after_id.as_deref(), before_id.as_deref())
                .await
                .map_err(model_error)
        }))
    }
}

fn execute_request<F, Fut>(input: Value, ctx: ToolContext, call: F) -> ToolStream
where
    F: FnOnce(MinimaxApiClient, Value) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<Value, ToolError>> + Send + 'static,
{
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client = MinimaxApiClient::from_api_key(credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            call(client, request).await
        }
        .await;
        match result {
            Ok(value) => ToolEvent::Final(ToolResult::Structured(value)),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

fn execute_image_request(input: Value, ctx: ToolContext) -> ToolStream {
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx).await?;
            let base_url = credential.base_url.clone();
            let request = request(&input).map_err(validation_error)?;
            let mut client = MinimaxApiClient::from_api_key(credential.api_key);
            if let Some(base_url) = &base_url {
                client = client.with_base_url(base_url.clone());
            }
            let response = client
                .image_generation(request)
                .await
                .map_err(model_error)?;
            image_tool_result_from_response(response, &ctx, base_url.as_deref()).await
        }
        .await;
        match result {
            Ok(result) => ToolEvent::Final(result),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

async fn image_tool_result_from_response(
    response: Value,
    ctx: &ToolContext,
    provider_base_url: Option<&str>,
) -> Result<ToolResult, ToolError> {
    image_tool_result_from_response_with_downloader(
        response,
        ctx,
        provider_base_url,
        &ReqwestMinimaxImageDownloader,
    )
    .await
}

async fn image_tool_result_from_response_with_downloader(
    response: Value,
    ctx: &ToolContext,
    _provider_base_url: Option<&str>,
    downloader: &dyn MinimaxImageDownloader,
) -> Result<ToolResult, ToolError> {
    let candidate = select_image_candidate(&response).ok_or_else(|| {
        ToolError::Message("MiniMax image response did not include a supported image".to_owned())
    })?;
    let image = match candidate {
        ImageCandidate::DataUrl(value) => decode_data_url_image(&value)?,
        ImageCandidate::Base64(value) => decode_base64_image(&value)?,
        ImageCandidate::HttpsUrl(value) => download_https_image(&value, downloader).await?,
    };
    let blob_ref = write_image_blob(ctx, image.bytes, &image.mime_type).await?;
    Ok(ToolResult::Mixed(vec![
        ToolResultPart::Structured {
            value: json!({
                "kind": "image",
                "status": "ready",
                "summary": "生成的图片",
                "mimeType": image.mime_type,
                "sizeBytes": blob_ref.size,
            }),
            schema_ref: None,
        },
        ToolResultPart::Blob {
            content_type: image.mime_type,
            blob_ref,
            summary: Some("生成的图片".to_owned()),
        },
    ]))
}

async fn write_image_blob(
    ctx: &ToolContext,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Result<harness_contracts::BlobRef, ToolError> {
    if bytes.is_empty() {
        return Err(ToolError::Message(
            "MiniMax image response was empty".to_owned(),
        ));
    }
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > MAX_MINIMAX_IMAGE_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: size,
            limit: MAX_MINIMAX_IMAGE_BYTES,
            metric: BudgetMetric::Bytes,
        });
    }
    let content_hash = *blake3::hash(&bytes).as_bytes();
    let writer = ctx.capability::<dyn BlobWriterCap>(ToolCapability::BlobWriter)?;
    writer
        .write_blob(
            ctx.tenant_id,
            Bytes::from(bytes),
            BlobMeta {
                content_type: Some(mime_type.to_owned()),
                size,
                content_hash,
                created_at: Utc::now(),
                retention: BlobRetention::SessionScoped(ctx.session_id),
            },
        )
        .await
}

#[derive(Debug, Clone)]
struct ImageBytes {
    bytes: Vec<u8>,
    mime_type: String,
}

#[async_trait::async_trait]
trait MinimaxImageDownloader: Send + Sync {
    async fn download(&self, url: &Url) -> Result<ImageBytes, ToolError>;
}

struct ReqwestMinimaxImageDownloader;

#[derive(Debug, Clone)]
enum ImageCandidate {
    DataUrl(String),
    Base64(String),
    HttpsUrl(String),
}

fn select_image_candidate(value: &Value) -> Option<ImageCandidate> {
    let mut candidates = Vec::new();
    collect_image_candidates(value, None, &mut candidates);
    candidates.sort_by_key(image_candidate_priority);
    candidates.into_iter().next()
}

fn image_candidate_priority(candidate: &ImageCandidate) -> u8 {
    match candidate {
        ImageCandidate::DataUrl(_) => 0,
        ImageCandidate::Base64(_) => 1,
        ImageCandidate::HttpsUrl(_) => 2,
    }
}

fn collect_image_candidates(
    value: &Value,
    key_hint: Option<&str>,
    candidates: &mut Vec<ImageCandidate>,
) {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.starts_with("data:image/") {
                candidates.push(ImageCandidate::DataUrl(trimmed.to_owned()));
                return;
            }
            if key_hint.is_some_and(is_likely_base64_image_key) {
                candidates.push(ImageCandidate::Base64(trimmed.to_owned()));
                return;
            }
            if key_hint.is_some_and(is_likely_image_url_key) && trimmed.starts_with("https://") {
                candidates.push(ImageCandidate::HttpsUrl(trimmed.to_owned()));
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_image_candidates(item, key_hint, candidates);
            }
        }
        Value::Object(object) => {
            for (key, nested) in object {
                collect_image_candidates(nested, Some(key), candidates);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_likely_base64_image_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("base64") || key == "image" || key == "image_data"
}

fn is_likely_image_url_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("url") && (key.contains("image") || key.contains("file") || key == "url")
}

fn decode_data_url_image(value: &str) -> Result<ImageBytes, ToolError> {
    let comma = value
        .find(',')
        .ok_or_else(|| ToolError::Message("MiniMax image data URL is malformed".to_owned()))?;
    let metadata = &value["data:".len()..comma];
    let mut parts = metadata.split(';');
    let mime_type = parts.next().unwrap_or_default().trim();
    let is_base64 = parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let mime_type = safe_minimax_image_mime(mime_type)
        .ok_or_else(|| ToolError::Message("MiniMax image data URL is unsupported".to_owned()))?;
    if !is_base64 {
        return Err(ToolError::Message(
            "MiniMax image data URL is unsupported".to_owned(),
        ));
    }
    let bytes = decode_base64_bytes(&value[comma + 1..])?;
    let mime_type = validate_image_bytes(&bytes, Some(mime_type))?;
    Ok(ImageBytes { bytes, mime_type })
}

fn decode_base64_image(value: &str) -> Result<ImageBytes, ToolError> {
    let bytes = decode_base64_bytes(value)?;
    let mime_type = validate_image_bytes(&bytes, None)?;
    Ok(ImageBytes { bytes, mime_type })
}

fn decode_base64_bytes(value: &str) -> Result<Vec<u8>, ToolError> {
    let value = value.trim();
    let decoded_upper_bound = base64_decoded_upper_bound(value);
    if decoded_upper_bound > MAX_MINIMAX_IMAGE_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: decoded_upper_bound,
            limit: MAX_MINIMAX_IMAGE_BYTES,
            metric: BudgetMetric::Bytes,
        });
    }
    general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ToolError::Message("MiniMax image payload is not valid base64".to_owned()))
}

fn base64_decoded_upper_bound(value: &str) -> u64 {
    let len = u64::try_from(value.len()).unwrap_or(u64::MAX);
    let padding = value
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    len.div_ceil(4)
        .saturating_mul(3)
        .saturating_sub(u64::try_from(padding).unwrap_or(0))
}

async fn download_https_image(
    value: &str,
    downloader: &dyn MinimaxImageDownloader,
) -> Result<ImageBytes, ToolError> {
    let url = Url::parse(value)
        .map_err(|_| ToolError::Message("MiniMax image asset URL is malformed".to_owned()))?;
    if url.scheme() != "https" || !is_allowed_minimax_image_host(&url) {
        return Err(ToolError::PermissionDenied(
            "MiniMax image asset host is not allowed".to_owned(),
        ));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(ToolError::PermissionDenied(
            "MiniMax image asset URL is not allowed".to_owned(),
        ));
    }
    let image = downloader.download(&url).await?;
    let mime_type = validate_image_bytes(&image.bytes, Some(&image.mime_type))?;
    Ok(ImageBytes {
        bytes: image.bytes,
        mime_type,
    })
}

#[async_trait::async_trait]
impl MinimaxImageDownloader for ReqwestMinimaxImageDownloader {
    async fn download(&self, url: &Url) -> Result<ImageBytes, ToolError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| {
                ToolError::Message(format!("MiniMax image download setup failed: {error}"))
            })?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| ToolError::Message("MiniMax image download failed".to_owned()))?;
        if !response.status().is_success() {
            return Err(ToolError::Message(
                "MiniMax image download failed".to_owned(),
            ));
        }
        let mime_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .and_then(safe_minimax_image_mime)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                ToolError::Message("MiniMax image download returned non-image content".to_owned())
            })?;
        let content_length = response.content_length().unwrap_or(0);
        if content_length > MAX_MINIMAX_IMAGE_BYTES {
            return Err(ToolError::ResultTooLarge {
                original: content_length,
                limit: MAX_MINIMAX_IMAGE_BYTES,
                metric: BudgetMetric::Bytes,
            });
        }
        let mut bytes = Vec::with_capacity(content_length.min(MAX_MINIMAX_IMAGE_BYTES) as usize);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|_| ToolError::Message("MiniMax image download failed".to_owned()))?;
            let next_len = u64::try_from(bytes.len())
                .unwrap_or(u64::MAX)
                .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
            if next_len > MAX_MINIMAX_IMAGE_BYTES {
                return Err(ToolError::ResultTooLarge {
                    original: next_len,
                    limit: MAX_MINIMAX_IMAGE_BYTES,
                    metric: BudgetMetric::Bytes,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        let mime_type = validate_image_bytes(&bytes, Some(&mime_type))?;
        Ok(ImageBytes { bytes, mime_type })
    }
}

fn is_allowed_minimax_image_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(
        host,
        "api.minimaxi.com" | "api.minimax.io" | "api.minimax.chat"
    ) || host.ends_with(".minimaxi.com")
        || host.ends_with(".minimax.io")
        || host.ends_with(".minimax.chat")
}

fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
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

fn safe_minimax_image_mime(value: &str) -> Option<&'static str> {
    let mime = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

fn validate_image_bytes(bytes: &[u8], declared_mime: Option<&str>) -> Result<String, ToolError> {
    let detected_mime = detect_image_mime(bytes).ok_or_else(|| {
        ToolError::Message("MiniMax image payload is not a supported image".to_owned())
    })?;
    if let Some(declared_mime) = declared_mime {
        let declared_mime = safe_minimax_image_mime(declared_mime).ok_or_else(|| {
            ToolError::Message("MiniMax image payload is not a supported image".to_owned())
        })?;
        if declared_mime != detected_mime {
            return Err(ToolError::Message(
                "MiniMax image payload MIME type does not match image bytes".to_owned(),
            ));
        }
    }
    Ok(detected_mime.to_owned())
}

async fn minimax_network_permission(ctx: &ToolContext) -> PermissionCheck {
    let credential = match minimax_credential(ctx).await {
        Ok(credential) => credential,
        Err(error) => return minimax_permission_denied(error),
    };

    match minimax_base_url_host(credential.base_url.as_deref()) {
        Ok((host, port)) => PermissionCheck::AskUser {
            subject: PermissionSubject::NetworkAccess { host, port },
            scope: DecisionScope::Category("network".to_owned()),
        },
        Err(reason) => PermissionCheck::Denied { reason },
    }
}

fn minimax_permission_denied(error: ToolError) -> PermissionCheck {
    let reason = match error {
        ToolError::CapabilityMissing(ToolCapability::ProviderCredentialResolver) => {
            "MiniMax provider credential resolver is missing".to_owned()
        }
        ToolError::PermissionDenied(message) => message,
        other => format!("MiniMax provider credential is unavailable: {other}"),
    };
    PermissionCheck::Denied { reason }
}

fn minimax_base_url_host(base_url: Option<&str>) -> Result<(String, Option<u16>), String> {
    let raw_base_url = base_url
        .map(str::trim)
        .filter(|base_url| !base_url.is_empty())
        .unwrap_or(DEFAULT_BASE_URL);
    let url =
        Url::parse(raw_base_url).map_err(|_| "MiniMax provider base URL is invalid".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("MiniMax provider base URL is invalid".to_owned());
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| "MiniMax provider base URL is invalid".to_owned())?;
    Ok((host.to_owned(), url.port()))
}

fn descriptor(name: &str, display_name: &str, description: &str) -> ToolDescriptor {
    super::descriptor(
        name,
        display_name,
        description,
        ToolGroup::Network,
        true,
        false,
        false,
        128_000,
        vec![ToolCapability::ProviderCredentialResolver],
        super::object_schema(
            &["request"],
            json!({
                "request": {
                    "type": "object",
                    "description": "MiniMax official API request body for this operation."
                }
            }),
        ),
    )
}

fn image_descriptor(name: &str, display_name: &str, description: &str) -> ToolDescriptor {
    super::descriptor(
        name,
        display_name,
        description,
        ToolGroup::Network,
        true,
        false,
        false,
        128_000,
        vec![
            ToolCapability::ProviderCredentialResolver,
            ToolCapability::BlobWriter,
        ],
        super::object_schema(
            &["request"],
            json!({
                "request": {
                    "type": "object",
                    "description": "MiniMax official API request body for image generation."
                }
            }),
        ),
    )
}

fn request(input: &Value) -> Result<Value, ValidationError> {
    input
        .get("request")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| ValidationError::from("request object is required"))
}

fn required_string(input: &Value, field: &str) -> Result<String, ValidationError> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ValidationError::from(format!("request.{field} string is required")))
}

fn optional_string(input: &Value, field: &str) -> Result<Option<String>, ValidationError> {
    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| ValidationError::from(format!("request.{field} must be a string"))),
    }
}

fn optional_u32(input: &Value, field: &str) -> Result<Option<u32>, ValidationError> {
    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| {
                ValidationError::from(format!("request.{field} must be an unsigned integer"))
            }),
    }
}

struct FileUploadRequest {
    bytes: Vec<u8>,
    file_name: String,
    group_id: Option<String>,
    purpose: String,
}

fn file_upload_request(input: &Value) -> Result<FileUploadRequest, ValidationError> {
    let purpose = required_string(input, "purpose")?;
    let file_name = required_string(input, "fileName")?;
    let bytes_base64 = required_string(input, "bytesBase64")?;
    let group_id = optional_string(input, "groupId")?;
    let bytes = general_purpose::STANDARD
        .decode(bytes_base64)
        .map_err(|_| ValidationError::from("request.bytesBase64 must be valid base64"))?;
    if bytes.is_empty() {
        return Err(ValidationError::from(
            "request.bytesBase64 must not be empty",
        ));
    }
    Ok(FileUploadRequest {
        bytes,
        file_name,
        group_id,
        purpose,
    })
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

async fn minimax_credential(ctx: &ToolContext) -> Result<ProviderCredential, ToolError> {
    let resolver = ctx.capability::<dyn ProviderCredentialResolverCap>(
        ToolCapability::ProviderCredentialResolver,
    )?;
    let credential = resolver
        .resolve_provider_credential(ProviderCredentialResolveContext {
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            provider_id: MINIMAX_PROVIDER_ID.to_owned(),
        })
        .await?;
    if credential.provider_id != MINIMAX_PROVIDER_ID {
        return Err(ToolError::PermissionDenied(
            "resolved provider credential does not match MiniMax".to_owned(),
        ));
    }
    if credential.api_key.trim().is_empty() {
        return Err(ToolError::PermissionDenied(
            "MiniMax provider config has no api key".to_owned(),
        ));
    }
    Ok(credential)
}

fn model_error(error: harness_contracts::ModelError) -> ToolError {
    ToolError::Message(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::Arc;

    use bytes::Bytes;
    use futures::future::BoxFuture;
    use harness_contracts::{
        AgentId, BlobMeta, BlobRef, BlobWriterCap, CapabilityRegistry, CorrelationId, Decision,
        PermissionError, RunId, SessionId, TenantId, ToolResultPart, ToolUseId,
    };
    use harness_permission::{
        PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
    };

    const PNG_1X1_BASE64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

    #[tokio::test]
    async fn image_response_base64_is_stored_as_blob_result() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let result = image_tool_result_from_response(
            json!({
                "data": {
                    "image_base64": PNG_1X1_BASE64,
                    "url": "https://asset.example.invalid/private.png"
                }
            }),
            &ctx,
            None,
        )
        .await
        .expect("image result is extracted");

        let ToolResult::Mixed(parts) = result else {
            panic!("expected mixed result, got {result:?}");
        };
        assert!(parts.iter().any(|part| matches!(
            part,
            ToolResultPart::Structured { value, .. }
                if value["kind"] == "image" && value.get("url").is_none()
        )));
        assert!(parts.iter().any(|part| matches!(
            part,
            ToolResultPart::Blob {
                content_type,
                blob_ref,
                summary
            } if content_type == "image/png"
                && blob_ref.content_type.as_deref() == Some("image/png")
                && summary.as_deref() == Some("生成的图片")
        )));
    }

    #[tokio::test]
    async fn image_response_rejects_disallowed_https_asset_host() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let error = image_tool_result_from_response(
            json!({
                "data": {
                    "image_url": "https://example.invalid/private.png"
                }
            }),
            &ctx,
            None,
        )
        .await
        .expect_err("disallowed host is rejected");

        assert!(error
            .to_string()
            .contains("MiniMax image asset host is not allowed"));
        assert!(!error.to_string().contains("private.png"));
    }

    #[tokio::test]
    async fn image_response_rejects_provider_base_url_asset_host() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let downloader = FakeImageDownloader;
        let error = image_tool_result_from_response_with_downloader(
            json!({
                "data": {
                    "image_url": "https://proxy.example.invalid/private-token.png"
                }
            }),
            &ctx,
            Some("https://proxy.example.invalid"),
            &downloader,
        )
        .await
        .expect_err("provider base URL host is not an allowed image asset host");

        assert!(error
            .to_string()
            .contains("MiniMax image asset host is not allowed"));
        assert!(!error.to_string().contains("proxy.example.invalid"));
        assert!(!error.to_string().contains("private-token.png"));
    }

    #[tokio::test]
    async fn image_response_rejects_svg_data_url() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let svg = general_purpose::STANDARD.encode(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        let error = image_tool_result_from_response(
            json!({
                "data": {
                    "image": format!("data:image/svg+xml;base64,{svg}")
                }
            }),
            &ctx,
            None,
        )
        .await
        .expect_err("svg data URL should be rejected");

        assert!(error.to_string().contains("unsupported"));
    }

    #[tokio::test]
    async fn image_response_rejects_oversized_inline_base64_before_decoding() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let oversized_base64 = "A".repeat(((MAX_MINIMAX_IMAGE_BYTES + 3) / 3 * 4) as usize);
        let error = image_tool_result_from_response(
            json!({
                "data": {
                    "image_base64": oversized_base64
                }
            }),
            &ctx,
            None,
        )
        .await
        .expect_err("oversized inline image should be rejected");

        assert!(matches!(error, ToolError::ResultTooLarge { .. }));
    }

    #[tokio::test]
    async fn image_response_rejects_svg_https_asset_from_downloader() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let downloader = FakeSvgImageDownloader;
        let error = image_tool_result_from_response_with_downloader(
            json!({
                "data": {
                    "image_url": "https://assets.minimaxi.com/generated/vector.svg"
                }
            }),
            &ctx,
            None,
            &downloader,
        )
        .await
        .expect_err("svg asset should be rejected");

        assert!(error.to_string().contains("supported image"));
        assert!(!error.to_string().contains("vector.svg"));
    }

    #[tokio::test]
    async fn image_response_allowed_https_asset_is_downloaded_as_blob_result() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let downloader = FakeImageDownloader;
        let result = image_tool_result_from_response_with_downloader(
            json!({
                "data": {
                    "image_url": "https://assets.minimaxi.com/generated/private-token.png"
                }
            }),
            &ctx,
            None,
            &downloader,
        )
        .await
        .expect("allowed MiniMax image asset is downloaded");

        let ToolResult::Mixed(parts) = result else {
            panic!("expected mixed result, got {result:?}");
        };
        let serialized = serde_json::to_string(&parts).unwrap();
        assert!(parts.iter().any(|part| matches!(
            part,
            ToolResultPart::Blob {
                content_type,
                blob_ref,
                ..
            } if content_type == "image/png"
                && blob_ref.content_type.as_deref() == Some("image/png")
        )));
        assert!(!serialized.contains("private-token.png"));
        assert!(!serialized.contains("assets.minimaxi.com"));
    }

    fn test_context(writer: Arc<dyn BlobWriterCap>) -> ToolContext {
        let mut caps = CapabilityRegistry::default();
        caps.install(ToolCapability::BlobWriter, writer);
        ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::new(),
            subagent_depth: 0,
            workspace_root: PathBuf::from("/tmp"),
            sandbox: None,
            permission_broker: Arc::new(AllowBroker),
            cap_registry: Arc::new(caps),
            redactor: Arc::new(harness_contracts::NoopRedactor),
            interrupt: crate::InterruptToken::new(),
            parent_run: None,
        }
    }

    struct FakeImageDownloader;

    #[async_trait::async_trait]
    impl MinimaxImageDownloader for FakeImageDownloader {
        async fn download(&self, _url: &Url) -> Result<ImageBytes, ToolError> {
            Ok(ImageBytes {
                bytes: general_purpose::STANDARD.decode(PNG_1X1_BASE64).unwrap(),
                mime_type: "image/png".to_owned(),
            })
        }
    }

    struct FakeSvgImageDownloader;

    #[async_trait::async_trait]
    impl MinimaxImageDownloader for FakeSvgImageDownloader {
        async fn download(&self, _url: &Url) -> Result<ImageBytes, ToolError> {
            Ok(ImageBytes {
                bytes: br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec(),
                mime_type: "image/svg+xml".to_owned(),
            })
        }
    }

    struct CapturingBlobWriter;

    impl BlobWriterCap for CapturingBlobWriter {
        fn write_blob(
            &self,
            _tenant_id: TenantId,
            bytes: Bytes,
            meta: BlobMeta,
        ) -> BoxFuture<'_, Result<BlobRef, ToolError>> {
            Box::pin(async move {
                assert!(!bytes.is_empty());
                assert_eq!(meta.content_type.as_deref(), Some("image/png"));
                Ok(BlobRef {
                    id: harness_contracts::BlobId::new(),
                    size: meta.size,
                    content_hash: meta.content_hash,
                    content_type: meta.content_type,
                })
            })
        }
    }

    struct AllowBroker;

    #[async_trait::async_trait]
    impl PermissionBroker for AllowBroker {
        async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
            Decision::AllowOnce
        }

        async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
            Ok(())
        }
    }
}
