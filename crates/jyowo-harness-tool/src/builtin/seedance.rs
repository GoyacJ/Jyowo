use crate::provider_media::{
    download_provider_https_media, validate_media_bytes, ProviderMediaBytes,
    ProviderMediaDownloader, ReqwestProviderMediaDownloader, MAX_MINIMAX_MEDIA_BYTES,
};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use futures::stream;
use harness_contracts::{
    BlobMeta, BlobRetention, BlobWriterCap, BudgetMetric, CapabilityRouteKind, DecisionScope,
    ModelModality, PermissionSubject, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ToolCapability, ToolDescriptor, ToolError, ToolGroup,
    ToolResult, ToolResultPart, ToolServiceBinding,
};
use harness_model::{SeedanceApiClient, SEEDANCE_DEFAULT_BASE_URL, SEEDANCE_PROVIDER_ID};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use url::Url;

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

const POLL_OPERATION_ID: &str = "seedance.video_generation.query";

macro_rules! seedance_create_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: create_descriptor($name, $display_name, $description),
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
                seedance_network_permission(ctx, &self.descriptor).await
            }

            async fn execute(
                &self,
                input: Value,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                Ok(execute_create_request(
                    input,
                    ctx,
                    &self.descriptor,
                    $display_name,
                ))
            }
        }
    };
}

seedance_create_tool!(
    SeedanceTextToVideo,
    "SeedanceTextToVideo",
    "Seedance text to video",
    "Create a Volcengine Ark Seedance video generation task from text."
);
seedance_create_tool!(
    SeedanceImageToVideo,
    "SeedanceImageToVideo",
    "Seedance image to video",
    "Create a Volcengine Ark Seedance video generation task from text and image references."
);

#[derive(Clone)]
pub struct SeedanceVideoGenerationQueryTool {
    descriptor: ToolDescriptor,
}

impl Default for SeedanceVideoGenerationQueryTool {
    fn default() -> Self {
        Self {
            descriptor: query_descriptor(
                "SeedanceVideoGenerationQuery",
                "Seedance video generation query",
                "Query a Volcengine Ark Seedance video generation task.",
            ),
        }
    }
}

#[async_trait]
impl Tool for SeedanceVideoGenerationQueryTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let request = request(input)?;
        required_string(&request, "task_id")?;
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionCheck {
        seedance_network_permission(ctx, &self.descriptor).await
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(execute_query_request(input, ctx, &self.descriptor))
    }
}

fn execute_create_request(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    title: &'static str,
) -> ToolStream {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    Box::pin(stream::once(async move {
        let result = async {
            let credential = seedance_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client = SeedanceApiClient::from_api_key(credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = client
                .create_video_generation_task(request)
                .await
                .map_err(model_error)?;
            async_job_tool_result(&response, POLL_OPERATION_ID, ModelModality::Video, title)
        }
        .await;
        match result {
            Ok(result) => ToolEvent::Final(result),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

fn execute_query_request(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
) -> ToolStream {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    Box::pin(stream::once(async move {
        let result = async {
            let credential = seedance_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let task_id = required_string(&request, "task_id").map_err(validation_error)?;
            let mut client = SeedanceApiClient::from_api_key(credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = client
                .query_video_generation_task(&task_id)
                .await
                .map_err(model_error)?;
            query_tool_result_from_response(response, &ctx, &ReqwestProviderMediaDownloader).await
        }
        .await;
        match result {
            Ok(result) => ToolEvent::Final(result),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

async fn query_tool_result_from_response(
    response: Value,
    ctx: &ToolContext,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ToolResult, ToolError> {
    if let Some(candidate) = select_media_candidate(&response, ModelModality::Video) {
        let media = resolve_media_candidate(candidate, ModelModality::Video, downloader).await?;
        let mime_type =
            validate_media_bytes(&media.bytes, ModelModality::Video, Some(&media.mime_type))?;
        let blob_ref = write_media_blob(ctx, media.bytes, &mime_type).await?;
        return Ok(artifact_tool_result(
            ModelModality::Video,
            mime_type,
            blob_ref,
            "Generated video",
        ));
    }
    if is_pending_task_status(&response) {
        return Ok(ToolResult::Structured(response));
    }
    Ok(ToolResult::Structured(response))
}

fn async_job_tool_result(
    response: &Value,
    poll_operation_id: &str,
    _artifact_kind: ModelModality,
    title: &str,
) -> Result<ToolResult, ToolError> {
    let job_id = extract_task_id(response).ok_or_else(|| {
        ToolError::Message("Seedance async task response did not include task id".to_owned())
    })?;
    Ok(ToolResult::Mixed(vec![ToolResultPart::Structured {
        value: json!({
            "kind": "async_job",
            "jobId": job_id,
            "pollOperationId": poll_operation_id,
            "artifactKind": "video",
            "title": title,
        }),
        schema_ref: Some("provider_service_async_job.v1".to_string()),
    }]))
}

fn artifact_tool_result(
    artifact_kind: ModelModality,
    content_type: String,
    blob_ref: harness_contracts::BlobRef,
    title: &str,
) -> ToolResult {
    ToolResult::Mixed(vec![ToolResultPart::Artifact {
        artifact_kind,
        content_type,
        blob_ref,
        title: title.to_owned(),
        preview: Some(title.to_owned()),
    }])
}

fn extract_task_id(value: &Value) -> Option<String> {
    value
        .get("id")
        .or_else(|| value.get("task_id"))
        .or_else(|| value.pointer("/data/id"))
        .or_else(|| value.pointer("/data/task_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
        .map(ToOwned::to_owned)
}

fn is_pending_task_status(value: &Value) -> bool {
    value
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| {
            matches!(
                status.to_ascii_lowercase().as_str(),
                "queued" | "running" | "processing" | "pending"
            )
        })
}

#[derive(Debug, Clone)]
enum MediaCandidate {
    DataUrl(String),
    Base64(String),
    HttpsUrl(String),
}

fn select_media_candidate(value: &Value, modality: ModelModality) -> Option<MediaCandidate> {
    let mut candidates = Vec::new();
    collect_media_candidates(value, None, modality, &mut candidates);
    candidates.sort_by_key(media_candidate_priority);
    candidates.into_iter().next()
}

fn media_candidate_priority(candidate: &MediaCandidate) -> u8 {
    match candidate {
        MediaCandidate::DataUrl(_) => 0,
        MediaCandidate::Base64(_) => 1,
        MediaCandidate::HttpsUrl(_) => 2,
    }
}

fn collect_media_candidates(
    value: &Value,
    key_hint: Option<&str>,
    modality: ModelModality,
    candidates: &mut Vec<MediaCandidate>,
) {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            let data_prefix = match modality {
                ModelModality::Video => "data:video/",
                ModelModality::Image => "data:image/",
                ModelModality::Audio => "data:audio/",
                ModelModality::Text | ModelModality::Embedding | ModelModality::File => return,
            };
            if trimmed.starts_with(data_prefix) {
                candidates.push(MediaCandidate::DataUrl(trimmed.to_owned()));
                return;
            }
            if key_hint.is_some_and(|key| is_likely_base64_media_key(key, modality)) {
                candidates.push(MediaCandidate::Base64(trimmed.to_owned()));
                return;
            }
            if key_hint.is_some_and(|key| is_likely_media_url_key(key, modality))
                && is_collectible_media_url(trimmed)
            {
                candidates.push(MediaCandidate::HttpsUrl(trimmed.to_owned()));
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_media_candidates(item, key_hint, modality, candidates);
            }
        }
        Value::Object(object) => {
            for (key, nested) in object {
                collect_media_candidates(nested, Some(key), modality, candidates);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_collectible_media_url(value: &str) -> bool {
    if value.starts_with("https://") {
        return true;
    }
    if value.starts_with("http://") {
        return Url::parse(value)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]"));
    }
    false
}

fn is_likely_base64_media_key(key: &str, modality: ModelModality) -> bool {
    let key = key.to_ascii_lowercase();
    match modality {
        ModelModality::Video => key.contains("base64") || key == "video_data",
        ModelModality::Image => key.contains("base64") || key == "image" || key == "image_data",
        ModelModality::Audio => key.contains("base64") || key == "audio" || key == "audio_data",
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => false,
    }
}

fn is_likely_media_url_key(key: &str, modality: ModelModality) -> bool {
    let key = key.to_ascii_lowercase();
    match modality {
        ModelModality::Video => {
            key.contains("url") && (key.contains("video") || key.contains("file") || key == "url")
        }
        ModelModality::Image => {
            key.contains("url") && (key.contains("image") || key.contains("file") || key == "url")
        }
        ModelModality::Audio => {
            key.contains("url") && (key.contains("audio") || key.contains("music") || key == "url")
        }
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => false,
    }
}

async fn resolve_media_candidate(
    candidate: MediaCandidate,
    modality: ModelModality,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ProviderMediaBytes, ToolError> {
    match candidate {
        MediaCandidate::DataUrl(value) => decode_data_url_media(&value, modality),
        MediaCandidate::Base64(value) => decode_base64_media(&value, modality),
        MediaCandidate::HttpsUrl(value) => {
            download_provider_https_media(
                SEEDANCE_PROVIDER_ID,
                &value,
                modality,
                downloader,
                MAX_MINIMAX_MEDIA_BYTES,
            )
            .await
        }
    }
}

fn decode_data_url_media(
    value: &str,
    modality: ModelModality,
) -> Result<ProviderMediaBytes, ToolError> {
    let comma = value
        .find(',')
        .ok_or_else(|| ToolError::Message("Seedance media data URL is malformed".to_owned()))?;
    let metadata = &value["data:".len()..comma];
    let mut parts = metadata.split(';');
    let mime_type = parts.next().unwrap_or_default().trim();
    let is_base64 = parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let mime_type = crate::provider_media::safe_mime_for_modality(mime_type, modality)
        .ok_or_else(|| ToolError::Message("Seedance media data URL is unsupported".to_owned()))?;
    if !is_base64 {
        return Err(ToolError::Message(
            "Seedance media data URL is unsupported".to_owned(),
        ));
    }
    let bytes = decode_base64_bytes(&value[comma + 1..])?;
    let mime_type = validate_media_bytes(&bytes, modality, Some(mime_type))?;
    Ok(ProviderMediaBytes { bytes, mime_type })
}

fn decode_base64_media(
    value: &str,
    modality: ModelModality,
) -> Result<ProviderMediaBytes, ToolError> {
    let bytes = decode_base64_bytes(value)?;
    let mime_type = validate_media_bytes(&bytes, modality, None)?;
    Ok(ProviderMediaBytes { bytes, mime_type })
}

fn decode_base64_bytes(value: &str) -> Result<Vec<u8>, ToolError> {
    let value = value.trim();
    let decoded_upper_bound = base64_decoded_upper_bound(value);
    if decoded_upper_bound > MAX_MINIMAX_MEDIA_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: decoded_upper_bound,
            limit: MAX_MINIMAX_MEDIA_BYTES,
            metric: BudgetMetric::Bytes,
        });
    }
    general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ToolError::Message("Seedance media payload is not valid base64".to_owned()))
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

async fn write_media_blob(
    ctx: &ToolContext,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Result<harness_contracts::BlobRef, ToolError> {
    if bytes.is_empty() {
        return Err(ToolError::Message(
            "Seedance media response was empty".to_owned(),
        ));
    }
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > MAX_MINIMAX_MEDIA_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: size,
            limit: MAX_MINIMAX_MEDIA_BYTES,
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

async fn seedance_network_permission(
    ctx: &ToolContext,
    descriptor: &ToolDescriptor,
) -> PermissionCheck {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let credential = match seedance_credential(ctx, operation_id, route_kind).await {
        Ok(credential) => credential,
        Err(error) => return seedance_permission_denied(error),
    };

    match seedance_base_url_host(credential.base_url.as_deref()) {
        Ok((host, port)) => PermissionCheck::AskUser {
            subject: PermissionSubject::NetworkAccess { host, port },
            scope: DecisionScope::Category("network".to_owned()),
        },
        Err(reason) => PermissionCheck::Denied { reason },
    }
}

fn seedance_permission_denied(error: ToolError) -> PermissionCheck {
    let reason = match error {
        ToolError::CapabilityMissing(ToolCapability::ProviderCredentialResolver) => {
            "Seedance provider credential resolver is missing".to_owned()
        }
        ToolError::PermissionDenied(message) => message,
        other => format!("Seedance provider credential is unavailable: {other}"),
    };
    PermissionCheck::Denied { reason }
}

fn seedance_base_url_host(base_url: Option<&str>) -> Result<(String, Option<u16>), String> {
    let raw_base_url = base_url
        .map(str::trim)
        .filter(|base_url| !base_url.is_empty())
        .unwrap_or(SEEDANCE_DEFAULT_BASE_URL);
    let url =
        Url::parse(raw_base_url).map_err(|_| "Seedance provider base URL is invalid".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Seedance provider base URL is invalid".to_owned());
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| "Seedance provider base URL is invalid".to_owned())?;
    Ok((host.to_owned(), url.port()))
}

async fn seedance_credential(
    ctx: &ToolContext,
    operation_id: Option<String>,
    route_kind: Option<CapabilityRouteKind>,
) -> Result<ProviderCredential, ToolError> {
    if operation_id.is_some() != route_kind.is_some() {
        return Err(ToolError::PermissionDenied(
            "Seedance service operation credential context is incomplete".to_owned(),
        ));
    }
    let resolver = ctx.capability::<dyn ProviderCredentialResolverCap>(
        ToolCapability::ProviderCredentialResolver,
    )?;
    let credential = resolver
        .resolve_provider_credential(ProviderCredentialResolveContext {
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            provider_id: SEEDANCE_PROVIDER_ID.to_owned(),
            operation_id: operation_id.clone(),
            route_kind,
        })
        .await?;
    if credential.provider_id != SEEDANCE_PROVIDER_ID {
        return Err(ToolError::PermissionDenied(
            "resolved provider credential does not match Seedance".to_owned(),
        ));
    }
    if credential.api_key.trim().is_empty() {
        return Err(ToolError::PermissionDenied(
            "Seedance provider config has no api key".to_owned(),
        ));
    }
    Ok(credential)
}

fn service_credential_context(
    descriptor: &ToolDescriptor,
) -> (Option<String>, Option<CapabilityRouteKind>) {
    descriptor
        .service_binding
        .as_ref()
        .map(|binding| (Some(binding.operation_id.clone()), Some(binding.route_kind)))
        .unwrap_or((None, None))
}

fn service_binding(
    operation_id: &str,
    route_kind: CapabilityRouteKind,
    output_artifact: ModelModality,
) -> ToolServiceBinding {
    ToolServiceBinding {
        provider_id: SEEDANCE_PROVIDER_ID.to_owned(),
        operation_id: operation_id.to_owned(),
        route_kind,
        output_artifact,
    }
}

fn create_descriptor(name: &str, display_name: &str, description: &str) -> ToolDescriptor {
    super::descriptor_with_binding(
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
                    "description": "Volcengine Ark Seedance official API request body for this operation."
                }
            }),
        ),
        Some(service_binding(
            "seedance.video_generation",
            CapabilityRouteKind::VideoGeneration,
            ModelModality::Video,
        )),
    )
}

fn query_descriptor(name: &str, display_name: &str, description: &str) -> ToolDescriptor {
    super::descriptor_with_binding(
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
                    "required": ["task_id"],
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Seedance video generation task id."
                        }
                    }
                }
            }),
        ),
        Some(service_binding(
            "seedance.video_generation.query",
            CapabilityRouteKind::VideoGeneration,
            ModelModality::Video,
        )),
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

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Message(error.to_string())
}

fn model_error(error: harness_contracts::ModelError) -> ToolError {
    ToolError::Message(error.to_string())
}
