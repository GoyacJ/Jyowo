use crate::provider_gemini::{GeminiApiClient, GeminiProviderClientError};
use crate::provider_media::{
    download_provider_https_media, safe_mime_types_for_modality, validate_media_bytes,
    BrokerProviderMediaDownloader, ProviderMediaBytes, ProviderMediaDownloadRequest,
    ProviderMediaDownloader, MAX_MINIMAX_MEDIA_BYTES,
};
use crate::{
    action_plan_from_permission_check, AuthorizedNetworkPermit, AuthorizedToolInput, Tool,
    ToolContext, ToolEvent, ToolNetworkBrokerCap, ToolStream, ValidationError,
};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use futures::{future::BoxFuture, stream};
use harness_contracts::{
    ActionResource, BlobMeta, BlobRetention, BlobWriterCap, BudgetMetric, CapabilityRouteKind,
    DecisionScope, HostRule, ModelModality, NetworkAccess, PermissionSubject, ProviderCredential,
    ProviderCredentialResolveContext, ProviderCredentialResolverCap, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
    ToolResultPart, ToolServiceBinding, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use url::Url;

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const GEMINI_PROVIDER_ID: &str = "gemini";
const MAX_GEMINI_MEDIA_BYTES: u64 = MAX_MINIMAX_MEDIA_BYTES;

macro_rules! gemini_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident) => {
        gemini_tool!(
            $type_name,
            $name,
            $display_name,
            $description,
            $operation,
            None
        );
    };
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident, $binding:expr) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: descriptor($name, $display_name, $description, $binding),
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

            async fn plan(
                &self,
                input: &Value,
                ctx: &ToolContext,
            ) -> Result<ToolActionPlan, ToolError> {
                gemini_network_action_plan(input, ctx, &self.descriptor).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                let input = authorized.raw_input().clone();
                let permit = authorized.network_permit()?;
                let broker =
                    ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
                Ok(execute_request(
                    input,
                    ctx,
                    &self.descriptor,
                    permit,
                    broker,
                    |client, request| {
                        Box::pin(async move {
                            client
                                .$operation(request)
                                .await
                                .map_err(provider_client_error)
                        })
                    },
                ))
            }
        }
    };
}

macro_rules! gemini_string_arg_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident, $field:literal) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: descriptor($name, $display_name, $description, None),
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

            async fn plan(
                &self,
                input: &Value,
                ctx: &ToolContext,
            ) -> Result<ToolActionPlan, ToolError> {
                gemini_network_action_plan(input, ctx, &self.descriptor).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                let input = authorized.raw_input().clone();
                let permit = authorized.network_permit()?;
                let broker =
                    ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
                Ok(execute_request(
                    input,
                    ctx,
                    &self.descriptor,
                    permit,
                    broker,
                    |client, request| {
                        Box::pin(async move {
                            let value =
                                required_string(&request, $field).map_err(validation_error)?;
                            client
                                .$operation(&value)
                                .await
                                .map_err(provider_client_error)
                        })
                    },
                ))
            }
        }
    };
}

gemini_tool!(
    GeminiTokensCountTool,
    "GeminiTokensCount",
    "Gemini count tokens",
    "Count input tokens with Gemini countTokens.",
    count_tokens
);
gemini_tool!(
    GeminiCachedContentCreateTool,
    "GeminiCachedContentCreate",
    "Gemini cached content create",
    "Create Gemini cached content.",
    create_cached_content
);
gemini_tool!(
    GeminiEmbeddingTool,
    "GeminiEmbedding",
    "Gemini embedding",
    "Embed content with Gemini.",
    embed_content
);
gemini_tool!(
    GeminiEmbeddingBatchTool,
    "GeminiEmbeddingBatch",
    "Gemini batch embedding",
    "Batch embed content with Gemini.",
    batch_embed_contents
);
gemini_tool!(
    GeminiBatchCreateTool,
    "GeminiBatchCreate",
    "Gemini batch create",
    "Create a Gemini batch job.",
    create_batch
);
gemini_tool!(
    GeminiImageGenerationTool,
    "GeminiImageGeneration",
    "Gemini image generation",
    "Generate an image with Gemini generateContent image models.",
    generate_image,
    Some(service_binding(
        "gemini.image_generation",
        CapabilityRouteKind::ImageGeneration,
        ModelModality::Image,
    ))
);
gemini_tool!(
    GeminiVideoGenerationTool,
    "GeminiVideoGeneration",
    "Gemini video generation",
    "Create a Gemini Veo video generation operation.",
    generate_video,
    Some(service_binding(
        "gemini.video_generation",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
gemini_tool!(
    GeminiTextToSpeechTool,
    "GeminiTextToSpeech",
    "Gemini text to speech",
    "Generate speech with Gemini TTS models.",
    text_to_speech,
    Some(service_binding(
        "gemini.text_to_speech",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Audio,
    ))
);

gemini_string_arg_tool!(
    GeminiModelGetTool,
    "GeminiModelGet",
    "Gemini model get",
    "Get Gemini model metadata.",
    get_model,
    "name"
);
gemini_string_arg_tool!(
    GeminiFileGetTool,
    "GeminiFileGet",
    "Gemini file get",
    "Get Gemini file metadata.",
    get_file,
    "name"
);
gemini_string_arg_tool!(
    GeminiFileDeleteTool,
    "GeminiFileDelete",
    "Gemini file delete",
    "Delete a Gemini file.",
    delete_file,
    "name"
);
gemini_string_arg_tool!(
    GeminiCachedContentGetTool,
    "GeminiCachedContentGet",
    "Gemini cached content get",
    "Get Gemini cached content metadata.",
    get_cached_content,
    "name"
);
gemini_string_arg_tool!(
    GeminiCachedContentDeleteTool,
    "GeminiCachedContentDelete",
    "Gemini cached content delete",
    "Delete Gemini cached content.",
    delete_cached_content,
    "name"
);
gemini_string_arg_tool!(
    GeminiBatchGetTool,
    "GeminiBatchGet",
    "Gemini batch get",
    "Get a Gemini batch job.",
    get_batch,
    "name"
);
gemini_string_arg_tool!(
    GeminiBatchCancelTool,
    "GeminiBatchCancel",
    "Gemini batch cancel",
    "Cancel a Gemini batch job.",
    cancel_batch,
    "name"
);

#[derive(Clone)]
pub struct GeminiModelsListTool {
    descriptor: ToolDescriptor,
}

impl Default for GeminiModelsListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "GeminiModelsList",
                "Gemini models list",
                "List Gemini models.",
                None,
            ),
        }
    }
}

#[async_trait]
impl Tool for GeminiModelsListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        gemini_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        Ok(execute_request(
            input,
            ctx,
            &self.descriptor,
            permit,
            broker,
            |client, _request| {
                Box::pin(async move { client.list_models().await.map_err(provider_client_error) })
            },
        ))
    }
}

#[derive(Clone)]
pub struct GeminiFileUploadTool {
    descriptor: ToolDescriptor,
}

impl Default for GeminiFileUploadTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "GeminiFileUpload",
                "Gemini file upload",
                "Upload a file to Gemini Files API.",
                None,
            ),
        }
    }
}

#[async_trait]
impl Tool for GeminiFileUploadTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let request = request(input)?;
        file_upload_request(&request)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        gemini_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        Ok(execute_request(
            input,
            ctx,
            &self.descriptor,
            permit,
            broker,
            |client, request| {
                Box::pin(async move {
                    let upload = file_upload_request(&request).map_err(validation_error)?;
                    client
                        .upload_file(
                            &upload.file_name,
                            &upload.mime_type,
                            upload.bytes,
                            upload.metadata,
                        )
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
    }
}

#[derive(Clone)]
pub struct GeminiFileListTool {
    descriptor: ToolDescriptor,
}

impl Default for GeminiFileListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "GeminiFileList",
                "Gemini file list",
                "List Gemini files.",
                None,
            ),
        }
    }
}

#[async_trait]
impl Tool for GeminiFileListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        gemini_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        Ok(execute_request(
            input,
            ctx,
            &self.descriptor,
            permit,
            broker,
            |client, request| {
                Box::pin(async move {
                    client
                        .list_files(
                            optional_u32(&request, "page_size").map_err(validation_error)?,
                            optional_string(&request, "page_token")
                                .map_err(validation_error)?
                                .as_deref(),
                        )
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
    }
}

#[derive(Clone)]
pub struct GeminiCachedContentListTool {
    descriptor: ToolDescriptor,
}

impl Default for GeminiCachedContentListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "GeminiCachedContentList",
                "Gemini cached content list",
                "List Gemini cached contents.",
                None,
            ),
        }
    }
}

#[async_trait]
impl Tool for GeminiCachedContentListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        gemini_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        Ok(execute_request(
            input,
            ctx,
            &self.descriptor,
            permit,
            broker,
            |client, request| {
                Box::pin(async move {
                    client
                        .list_cached_contents(
                            optional_u32(&request, "page_size").map_err(validation_error)?,
                            optional_string(&request, "page_token")
                                .map_err(validation_error)?
                                .as_deref(),
                        )
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
    }
}

#[derive(Clone)]
pub struct GeminiBatchListTool {
    descriptor: ToolDescriptor,
}

impl Default for GeminiBatchListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "GeminiBatchList",
                "Gemini batch list",
                "List Gemini batch jobs.",
                None,
            ),
        }
    }
}

#[async_trait]
impl Tool for GeminiBatchListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        gemini_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        Ok(execute_request(
            input,
            ctx,
            &self.descriptor,
            permit,
            broker,
            |client, request| {
                Box::pin(async move {
                    client
                        .list_batches(
                            optional_u32(&request, "page_size").map_err(validation_error)?,
                            optional_string(&request, "page_token")
                                .map_err(validation_error)?
                                .as_deref(),
                        )
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
    }
}

#[derive(Clone)]
pub struct GeminiVideoGenerationQueryTool {
    descriptor: ToolDescriptor,
}

impl Default for GeminiVideoGenerationQueryTool {
    fn default() -> Self {
        Self {
            descriptor: long_running_descriptor(descriptor(
                "GeminiVideoGenerationQuery",
                "Gemini video generation query",
                "Query a Gemini Veo operation.",
                Some(service_binding(
                    "gemini.video_generation.query",
                    CapabilityRouteKind::VideoGeneration,
                    ModelModality::Video,
                )),
            )),
        }
    }
}

#[async_trait]
impl Tool for GeminiVideoGenerationQueryTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let request = request(input)?;
        required_string(&request, "name")?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        gemini_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        Ok(execute_media_request(
            input,
            ctx,
            &self.descriptor,
            permit,
            broker,
            ModelModality::Video,
            "Generated video",
            |client, request| {
                Box::pin(async move {
                    let name = required_string(&request, "name").map_err(validation_error)?;
                    client
                        .query_video(&name)
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
    }
}

fn execute_request<F>(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn ToolNetworkBrokerCap>,
    call: F,
) -> ToolStream
where
    F: for<'a> FnOnce(&'a GeminiApiClient, Value) -> BoxFuture<'a, Result<Value, ToolError>>
        + Send
        + 'static,
{
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let service_binding = descriptor.service_binding.clone();
    Box::pin(stream::once(async move {
        let result = async {
            let credential = gemini_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                GeminiApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = call(&client, request).await?;
            if let Some(binding) = service_binding {
                match binding.operation_id.as_str() {
                    "gemini.image_generation" => {
                        return media_tool_result(
                            response,
                            &ctx,
                            &client,
                            &binding.operation_id,
                            ModelModality::Image,
                            "Generated image",
                            &broker,
                        )
                        .await;
                    }
                    "gemini.text_to_speech" => {
                        return media_tool_result(
                            response,
                            &ctx,
                            &client,
                            &binding.operation_id,
                            ModelModality::Audio,
                            "Generated speech",
                            &broker,
                        )
                        .await;
                    }
                    "gemini.video_generation" => {
                        return async_job_tool_result(
                            &response,
                            "gemini.video_generation.query",
                            ModelModality::Video,
                            "Generated video",
                        );
                    }
                    _ => {}
                }
            }
            Ok(ToolResult::Structured(response))
        }
        .await;
        match result {
            Ok(result) => ToolEvent::Final(result),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

fn execute_media_request<F>(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn ToolNetworkBrokerCap>,
    modality: ModelModality,
    title: &'static str,
    call: F,
) -> ToolStream
where
    F: for<'a> FnOnce(&'a GeminiApiClient, Value) -> BoxFuture<'a, Result<Value, ToolError>>
        + Send
        + 'static,
{
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let media_operation_id = operation_id.clone();
    Box::pin(stream::once(async move {
        let result = async {
            let credential = gemini_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                GeminiApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = call(&client, request).await?;
            if gemini_operation_done_failed(&response) {
                return Err(ToolError::Message(
                    "Gemini operation ended with failure status".to_owned(),
                ));
            }
            if gemini_operation_pending(&response) {
                return Ok(ToolResult::Structured(response));
            }
            let operation_id = media_operation_id.as_deref().ok_or_else(|| {
                ToolError::PermissionDenied(
                    "Gemini media operation credential context is incomplete".to_owned(),
                )
            })?;
            media_tool_result(
                response,
                &ctx,
                &client,
                operation_id,
                modality,
                title,
                &broker,
            )
            .await
        }
        .await;
        match result {
            Ok(result) => ToolEvent::Final(result),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

async fn media_tool_result(
    response: Value,
    ctx: &ToolContext,
    client: &GeminiApiClient,
    operation_id: &str,
    modality: ModelModality,
    title: &str,
    broker: &Arc<dyn ToolNetworkBrokerCap>,
) -> Result<ToolResult, ToolError> {
    let candidate = select_media_candidate(&response, modality).ok_or_else(|| {
        ToolError::Message(format!(
            "Gemini {} response did not include supported media",
            modality_label(modality)
        ))
    })?;
    let downloader = BrokerProviderMediaDownloader::new(Arc::clone(broker), client.broker_permit());
    let media = resolve_media_candidate(candidate, operation_id, modality, &downloader).await?;
    let mime_type = validate_media_bytes(&media.bytes, modality, Some(&media.mime_type))?;
    let blob_ref = write_media_blob(ctx, media.bytes, &mime_type).await?;
    Ok(artifact_tool_result(modality, mime_type, blob_ref, title))
}

fn async_job_tool_result(
    response: &Value,
    poll_operation_id: &str,
    artifact_kind: ModelModality,
    title: &str,
) -> Result<ToolResult, ToolError> {
    let job_id = response
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ToolError::Message("Gemini async operation response did not include name".to_owned())
        })?;
    Ok(ToolResult::Mixed(vec![ToolResultPart::Structured {
        value: json!({
            "kind": "async_job",
            "jobId": job_id,
            "pollOperationId": poll_operation_id,
            "artifactKind": modality_label(artifact_kind),
            "title": title,
        }),
        schema_ref: Some("provider_service_async_job.v1".to_owned()),
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

async fn write_media_blob(
    ctx: &ToolContext,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Result<harness_contracts::BlobRef, ToolError> {
    if bytes.is_empty() {
        return Err(ToolError::Message(
            "Gemini media response was empty".to_owned(),
        ));
    }
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > MAX_GEMINI_MEDIA_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: size,
            limit: MAX_GEMINI_MEDIA_BYTES,
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
enum MediaCandidate {
    Inline {
        data: String,
        mime_type: Option<String>,
    },
    HttpsUrl(String),
}

fn select_media_candidate(value: &Value, modality: ModelModality) -> Option<MediaCandidate> {
    let mut candidates = Vec::new();
    collect_media_candidates(value, modality, &mut candidates);
    candidates.into_iter().next()
}

fn collect_media_candidates(
    value: &Value,
    modality: ModelModality,
    candidates: &mut Vec<MediaCandidate>,
) {
    match value {
        Value::Object(object) => {
            for key in ["inlineData", "inline_data"] {
                if let Some(inline) = object.get(key).and_then(Value::as_object) {
                    if let Some(data) = inline.get("data").and_then(Value::as_str) {
                        let mime_type = inline
                            .get("mimeType")
                            .or_else(|| inline.get("mime_type"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned);
                        candidates.push(MediaCandidate::Inline {
                            data: data.to_owned(),
                            mime_type,
                        });
                    }
                }
            }
            for key in ["uri", "fileUri", "videoUri", "url"] {
                if let Some(uri) = object.get(key).and_then(Value::as_str) {
                    if is_collectible_media_url(uri) && key_matches_modality(key, modality) {
                        candidates.push(MediaCandidate::HttpsUrl(uri.to_owned()));
                    }
                }
            }
            for nested in object.values() {
                collect_media_candidates(nested, modality, candidates);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_media_candidates(item, modality, candidates);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn key_matches_modality(key: &str, modality: ModelModality) -> bool {
    let key = key.to_ascii_lowercase();
    match modality {
        ModelModality::Image => key.contains("image") || key == "uri" || key == "url",
        ModelModality::Video => key.contains("video") || key == "uri" || key == "url",
        ModelModality::Audio => key.contains("audio") || key == "uri" || key == "url",
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => false,
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

async fn resolve_media_candidate(
    candidate: MediaCandidate,
    operation_id: &str,
    modality: ModelModality,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ProviderMediaBytes, ToolError> {
    match candidate {
        MediaCandidate::Inline { data, mime_type } => {
            let bytes = decode_base64_bytes(&data)?;
            let mime_type = validate_media_bytes(&bytes, modality, mime_type.as_deref())?;
            Ok(ProviderMediaBytes { bytes, mime_type })
        }
        MediaCandidate::HttpsUrl(value) => {
            download_provider_https_media(
                ProviderMediaDownloadRequest {
                    provider_id: GEMINI_PROVIDER_ID,
                    operation_id,
                    url: &value,
                    artifact_kind: modality,
                    expected_mime_types: safe_mime_types_for_modality(modality),
                    max_bytes: MAX_GEMINI_MEDIA_BYTES,
                },
                downloader,
            )
            .await
        }
    }
}

fn decode_base64_bytes(value: &str) -> Result<Vec<u8>, ToolError> {
    let value = value.trim();
    let decoded_upper_bound = base64_decoded_upper_bound(value);
    if decoded_upper_bound > MAX_GEMINI_MEDIA_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: decoded_upper_bound,
            limit: MAX_GEMINI_MEDIA_BYTES,
            metric: BudgetMetric::Bytes,
        });
    }
    general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ToolError::Message("Gemini media payload is not valid base64".to_owned()))
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

fn gemini_operation_pending(response: &Value) -> bool {
    response.get("done").and_then(Value::as_bool) == Some(false)
}

fn gemini_operation_done_failed(response: &Value) -> bool {
    response.get("error").is_some()
}

async fn gemini_network_action_plan(
    input: &Value,
    ctx: &ToolContext,
    descriptor: &ToolDescriptor,
) -> Result<ToolActionPlan, ToolError> {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let downloads_media = gemini_operation_downloads_media(operation_id.as_deref());
    let credential = match gemini_credential(ctx, operation_id, route_kind).await {
        Ok(credential) => credential,
        Err(error) => {
            return action_plan_from_permission_check(
                descriptor,
                input,
                ctx,
                gemini_permission_denied(error),
                Vec::new(),
                WorkspaceAccess::None,
                NetworkAccess::None,
                ToolExecutionChannel::HttpBroker,
            );
        }
    };

    match gemini_base_url_host(credential.base_url.as_deref()) {
        Ok((host, port)) => {
            let host_rules = gemini_host_rules(&host, port, downloads_media);
            action_plan_from_permission_check(
                descriptor,
                input,
                ctx,
                PermissionCheck::AskUser {
                    subject: PermissionSubject::NetworkAccess {
                        host: host.clone(),
                        port: Some(port),
                    },
                    scope: DecisionScope::Category("network".to_owned()),
                },
                network_resources(&host_rules),
                WorkspaceAccess::None,
                NetworkAccess::AllowList(host_rules),
                ToolExecutionChannel::HttpBroker,
            )
        }
        Err(reason) => action_plan_from_permission_check(
            descriptor,
            input,
            ctx,
            PermissionCheck::Denied { reason },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::HttpBroker,
        ),
    }
}

fn gemini_permission_denied(error: ToolError) -> PermissionCheck {
    let reason = match error {
        ToolError::CapabilityMissing(ToolCapability::ProviderCredentialResolver) => {
            "Gemini provider credential resolver is missing".to_owned()
        }
        ToolError::PermissionDenied(message) => message,
        other => format!("Gemini provider credential is unavailable: {other}"),
    };
    PermissionCheck::Denied { reason }
}

fn gemini_operation_downloads_media(operation_id: Option<&str>) -> bool {
    matches!(
        operation_id,
        Some("gemini.image_generation" | "gemini.video_generation.query" | "gemini.text_to_speech")
    )
}

fn gemini_host_rules(base_host: &str, base_port: u16, include_media_hosts: bool) -> Vec<HostRule> {
    let mut rules = Vec::new();
    push_host_rule(&mut rules, base_host, base_port);
    if include_media_hosts {
        for host in [
            "*.googleapis.com",
            "*.googleusercontent.com",
            "*.gstatic.com",
            "storage.googleapis.com",
        ] {
            push_host_rule(&mut rules, host, 443);
        }
    }
    rules
}

fn push_host_rule(rules: &mut Vec<HostRule>, pattern: &str, port: u16) {
    if !rules.iter().any(|rule| {
        rule.pattern == pattern
            && rule
                .ports
                .as_ref()
                .is_some_and(|ports| ports.contains(&port))
    }) {
        rules.push(HostRule {
            pattern: pattern.to_owned(),
            ports: Some(vec![port]),
        });
    }
}

fn network_resources(rules: &[HostRule]) -> Vec<ActionResource> {
    rules
        .iter()
        .flat_map(|rule| {
            rule.ports
                .as_ref()
                .into_iter()
                .flatten()
                .map(|port| ActionResource::Network {
                    host: rule.pattern.clone(),
                    port: Some(*port),
                })
        })
        .collect()
}

fn gemini_base_url_host(base_url: Option<&str>) -> Result<(String, u16), String> {
    let raw_base_url = base_url
        .map(str::trim)
        .filter(|base_url| !base_url.is_empty())
        .unwrap_or(DEFAULT_BASE_URL);
    let url =
        Url::parse(raw_base_url).map_err(|_| "Gemini provider base URL is invalid".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Gemini provider base URL is invalid".to_owned());
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| "Gemini provider base URL is invalid".to_owned())?;
    let is_allowed_host = host.eq_ignore_ascii_case("generativelanguage.googleapis.com");
    #[cfg(debug_assertions)]
    let is_allowed_host = is_allowed_host
        || host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !is_allowed_host {
        return Err("Gemini provider base URL host is not allowed".to_owned());
    }
    if url.scheme() == "http" {
        #[cfg(debug_assertions)]
        {
            let loopback = host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback());
            if loopback {
                let port = url
                    .port_or_known_default()
                    .ok_or_else(|| "Gemini provider base URL is invalid".to_owned())?;
                return Ok((host.to_owned(), port));
            }
        }
        return Err("Gemini provider base URL must use https".to_owned());
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Gemini provider base URL is invalid".to_owned())?;
    Ok((host.to_owned(), port))
}

async fn gemini_credential(
    ctx: &ToolContext,
    operation_id: Option<String>,
    route_kind: Option<CapabilityRouteKind>,
) -> Result<ProviderCredential, ToolError> {
    let resolver = ctx.capability::<dyn ProviderCredentialResolverCap>(
        ToolCapability::ProviderCredentialResolver,
    )?;
    let credential = resolver
        .resolve_provider_credential(ProviderCredentialResolveContext {
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            provider_id: GEMINI_PROVIDER_ID.to_owned(),
            model_config_id: ctx.model_config_id.clone(),
            operation_id,
            route_kind,
        })
        .await?;
    if credential.provider_id != GEMINI_PROVIDER_ID {
        return Err(ToolError::PermissionDenied(format!(
            "provider credential does not match Gemini: {}",
            credential.provider_id
        )));
    }
    if credential.api_key.trim().is_empty() {
        return Err(ToolError::PermissionDenied(
            "Gemini provider credential is empty".to_owned(),
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
        provider_id: GEMINI_PROVIDER_ID.to_owned(),
        operation_id: operation_id.to_owned(),
        route_kind,
        output_artifact,
    }
}

fn descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    service_binding: Option<ToolServiceBinding>,
) -> ToolDescriptor {
    let writes_blob = service_binding.as_ref().is_some_and(|binding| {
        matches!(
            binding.operation_id.as_str(),
            "gemini.image_generation" | "gemini.video_generation.query" | "gemini.text_to_speech"
        )
    });
    let mut required_capabilities = vec![ToolCapability::ProviderCredentialResolver];
    if writes_blob {
        required_capabilities.push(ToolCapability::BlobWriter);
    }
    super::with_output_schema(
        super::descriptor_with_binding(
            name,
            display_name,
            description,
            ToolGroup::Network,
            true,
            false,
            matches!(
                name,
                "GeminiFileDelete" | "GeminiCachedContentDelete" | "GeminiBatchCancel"
            ),
            128_000,
            required_capabilities,
            super::object_schema(
                &["request"],
                json!({
                    "request": {
                        "type": "object",
                        "description": "Gemini API request payload. Pass official REST fields plus model/name helpers where required."
                    }
                }),
            ),
            service_binding,
        ),
        json!({ "type": "object" }),
    )
}

fn long_running_descriptor(descriptor: ToolDescriptor) -> ToolDescriptor {
    super::with_long_running(
        descriptor,
        super::long_running_policy(Duration::from_secs(10), Duration::from_secs(900)),
    )
}

fn request(input: &Value) -> Result<Value, ValidationError> {
    input
        .get("request")
        .cloned()
        .filter(Value::is_object)
        .ok_or_else(|| ValidationError::from("request must be an object"))
}

fn required_string(value: &Value, field: &str) -> Result<String, ValidationError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ValidationError::from(format!("{field} must be a non-empty string")))
}

fn optional_string(value: &Value, field: &str) -> Result<Option<String>, ValidationError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_owned()))
            }
        }
        _ => Err(ValidationError::from(format!("{field} must be a string"))),
    }
}

fn optional_u32(value: &Value, field: &str) -> Result<Option<u32>, ValidationError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| ValidationError::from(format!("{field} must be a u32"))),
        _ => Err(ValidationError::from(format!("{field} must be a u32"))),
    }
}

struct FileUploadRequest {
    file_name: String,
    mime_type: String,
    bytes: Vec<u8>,
    metadata: Value,
}

fn file_upload_request(value: &Value) -> Result<FileUploadRequest, ValidationError> {
    let file_name = required_string(value, "file_name")?;
    let mime_type = required_string(value, "mime_type")?;
    validate_upload_header_input("file_name", &file_name)?;
    validate_upload_header_input("mime_type", &mime_type)?;
    let bytes_base64 = required_string(value, "bytes_base64")?;
    if base64_decoded_upper_bound(&bytes_base64) > MAX_GEMINI_MEDIA_BYTES {
        return Err(ValidationError::from("bytes_base64 is too large"));
    }
    let bytes = general_purpose::STANDARD
        .decode(bytes_base64)
        .map_err(|_| ValidationError::from("bytes_base64 must be valid base64"))?;
    if bytes.is_empty() {
        return Err(ValidationError::from("bytes_base64 must not be empty"));
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_GEMINI_MEDIA_BYTES {
        return Err(ValidationError::from("bytes_base64 is too large"));
    }
    let metadata = value
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({ "file": { "displayName": file_name } }));
    Ok(FileUploadRequest {
        file_name,
        mime_type,
        bytes,
        metadata,
    })
}

fn validate_upload_header_input(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.chars().any(|ch| ch.is_control()) {
        return Err(ValidationError::from(format!("{field} is invalid")));
    }
    Ok(())
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn provider_client_error(error: GeminiProviderClientError) -> ToolError {
    match error {
        GeminiProviderClientError::InvalidRequest(message) => ToolError::Validation(message),
        GeminiProviderClientError::ProviderUnavailable(message)
        | GeminiProviderClientError::UnexpectedResponse(message) => ToolError::Message(message),
    }
}

fn modality_label(modality: ModelModality) -> &'static str {
    match modality {
        ModelModality::Image => "image",
        ModelModality::Video => "video",
        ModelModality::Audio => "audio",
        ModelModality::File => "file",
        ModelModality::Text | ModelModality::Embedding => "text",
    }
}
