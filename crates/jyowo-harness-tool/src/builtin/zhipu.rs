use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::stream;
use harness_contracts::{
    ActionResource, BlobMeta, BlobRetention, BlobWriterCap, BudgetMetric, CapabilityRouteKind,
    DecisionScope, HostRule, ModelModality, NetworkAccess, PermissionSubject, ProviderCredential,
    ProviderCredentialResolveContext, ProviderCredentialResolverCap, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
    ToolResultPart, ToolServiceBinding, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use url::Url;

use crate::provider_zhipu::{ZhipuApiClient, ZhipuProviderClientError};
use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolNetworkBrokerCap, ToolStream, ValidationError,
};

const ZHIPU_PROVIDER_ID: &str = "zhipu";
const MAX_ZHIPU_MEDIA_BYTES: u64 = 25 * 1024 * 1024;

macro_rules! zhipu_post_tool {
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
                zhipu_network_action_plan(input, ctx, &self.descriptor).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                let request = request(authorized.raw_input()).map_err(validation_error)?;
                let client = zhipu_client(&ctx, &self.descriptor, authorized).await?;
                let response = client
                    .$operation(request)
                    .await
                    .map_err(provider_client_error)?;
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Structured(response),
                )])))
            }
        }
    };
}

zhipu_post_tool!(
    ZhipuImageGenerationTool,
    "ZhipuImageGeneration",
    "Zhipu image generation",
    "Generate an image with Zhipu image generation.",
    image_generation,
    Some(service_binding(
        "zhipu.image_generation",
        CapabilityRouteKind::ImageGeneration,
        ModelModality::Image,
    ))
);

zhipu_post_tool!(
    ZhipuImageGenerationAsyncTool,
    "ZhipuImageGenerationAsync",
    "Zhipu async image generation",
    "Create a Zhipu async image generation task.",
    image_generation_async,
    Some(service_binding(
        "zhipu.image_generation.async",
        CapabilityRouteKind::ImageGeneration,
        ModelModality::Image,
    ))
);

zhipu_post_tool!(
    ZhipuVideoGenerationTool,
    "ZhipuVideoGeneration",
    "Zhipu video generation",
    "Create a Zhipu video generation task.",
    video_generation,
    Some(service_binding(
        "zhipu.video_generation",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);

zhipu_post_tool!(
    ZhipuSpeechToTextTool,
    "ZhipuSpeechToText",
    "Zhipu speech to text",
    "Transcribe audio with Zhipu ASR.",
    speech_to_text,
    Some(service_binding(
        "zhipu.speech_to_text",
        CapabilityRouteKind::SpeechToText,
        ModelModality::Text,
    ))
);

#[derive(Clone)]
pub struct ZhipuTextToSpeechTool {
    descriptor: ToolDescriptor,
}

impl Default for ZhipuTextToSpeechTool {
    fn default() -> Self {
        Self {
            descriptor: media_descriptor(
                "ZhipuTextToSpeech",
                "Zhipu text to speech",
                "Generate speech with Zhipu TTS.",
                Some(service_binding(
                    "zhipu.text_to_speech",
                    CapabilityRouteKind::TextToSpeech,
                    ModelModality::Audio,
                )),
            ),
        }
    }
}

#[async_trait]
impl Tool for ZhipuTextToSpeechTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        zhipu_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let request = request(authorized.raw_input()).map_err(validation_error)?;
        let client = zhipu_client(&ctx, &self.descriptor, authorized).await?;
        let response = client
            .text_to_speech(request)
            .await
            .map_err(provider_client_error)?;
        let result = text_to_speech_result(response.content_type, response.body, &ctx).await?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(result)])))
    }
}

#[derive(Clone)]
pub struct ZhipuImageGenerationQueryTool {
    descriptor: ToolDescriptor,
}

impl Default for ZhipuImageGenerationQueryTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "ZhipuImageGenerationQuery",
                "Zhipu image generation query",
                "Query a Zhipu async image generation task.",
                Some(service_binding(
                    "zhipu.image_generation.query",
                    CapabilityRouteKind::ImageGeneration,
                    ModelModality::Image,
                )),
            ),
        }
    }
}

#[derive(Clone)]
pub struct ZhipuVideoGenerationQueryTool {
    descriptor: ToolDescriptor,
}

impl Default for ZhipuVideoGenerationQueryTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "ZhipuVideoGenerationQuery",
                "Zhipu video generation query",
                "Query a Zhipu video generation task.",
                Some(service_binding(
                    "zhipu.video_generation.query",
                    CapabilityRouteKind::VideoGeneration,
                    ModelModality::Video,
                )),
            ),
        }
    }
}

#[async_trait]
impl Tool for ZhipuImageGenerationQueryTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        task_id(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        zhipu_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        execute_query(authorized, ctx, &self.descriptor).await
    }
}

#[async_trait]
impl Tool for ZhipuVideoGenerationQueryTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        task_id(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        zhipu_network_action_plan(input, ctx, &self.descriptor).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        execute_query(authorized, ctx, &self.descriptor).await
    }
}

async fn execute_query(
    authorized: AuthorizedToolInput,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
) -> Result<ToolStream, ToolError> {
    let id = task_id(authorized.raw_input()).map_err(validation_error)?;
    let client = zhipu_client(&ctx, descriptor, authorized).await?;
    let response = client
        .async_result(&id)
        .await
        .map_err(provider_client_error)?;
    Ok(Box::pin(stream::iter([ToolEvent::Final(
        ToolResult::Structured(response),
    )])))
}

async fn zhipu_client(
    ctx: &ToolContext,
    descriptor: &ToolDescriptor,
    authorized: AuthorizedToolInput,
) -> Result<ZhipuApiClient, ToolError> {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let credential = zhipu_credential(ctx, operation_id, route_kind).await?;
    let permit = authorized.network_permit()?;
    let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
    let mut client = ZhipuApiClient::from_broker(broker, permit, credential.api_key);
    if let Some(base_url) = credential.base_url {
        client = client.with_base_url(base_url);
    }
    Ok(client)
}

async fn zhipu_network_action_plan(
    input: &Value,
    ctx: &ToolContext,
    descriptor: &ToolDescriptor,
) -> Result<ToolActionPlan, ToolError> {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let credential = match zhipu_credential(ctx, operation_id, route_kind).await {
        Ok(credential) => credential,
        Err(error) => {
            return action_plan_from_permission_check(
                descriptor,
                input,
                ctx,
                zhipu_permission_denied(error),
                Vec::new(),
                WorkspaceAccess::None,
                NetworkAccess::None,
                ToolExecutionChannel::HttpBroker,
            );
        }
    };

    match zhipu_base_url_host(credential.base_url.as_deref()) {
        Ok((host, port)) => {
            let host_rules = vec![HostRule {
                pattern: host.clone(),
                ports: Some(vec![port]),
            }];
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

async fn zhipu_credential(
    ctx: &ToolContext,
    operation_id: Option<String>,
    route_kind: Option<CapabilityRouteKind>,
) -> Result<ProviderCredential, ToolError> {
    if operation_id.is_some() != route_kind.is_some() {
        return Err(ToolError::PermissionDenied(
            "Zhipu service operation credential context is incomplete".to_owned(),
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
            provider_id: ZHIPU_PROVIDER_ID.to_owned(),
            model_config_id: operation_id
                .is_none()
                .then(|| ctx.model_config_id.clone())
                .flatten(),
            operation_id: operation_id.clone(),
            route_kind,
        })
        .await?;
    if credential.provider_id != ZHIPU_PROVIDER_ID {
        return Err(ToolError::PermissionDenied(
            "resolved provider credential does not match Zhipu".to_owned(),
        ));
    }
    if credential.api_key.trim().is_empty() {
        return Err(ToolError::PermissionDenied(
            "Zhipu provider config has no api key".to_owned(),
        ));
    }
    Ok(credential)
}

fn zhipu_permission_denied(error: ToolError) -> PermissionCheck {
    let reason = match error {
        ToolError::CapabilityMissing(ToolCapability::ProviderCredentialResolver) => {
            "Zhipu provider credential resolver is missing".to_owned()
        }
        ToolError::PermissionDenied(message) => message,
        other => format!("Zhipu provider credential is unavailable: {other}"),
    };
    PermissionCheck::Denied { reason }
}

fn zhipu_base_url_host(base_url: Option<&str>) -> Result<(String, u16), String> {
    let base_url = base_url.unwrap_or("https://open.bigmodel.cn/api/paas/v4");
    let url = Url::parse(base_url).map_err(|_| "Zhipu provider base URL is invalid".to_owned())?;
    let host = url
        .host_str()
        .ok_or_else(|| "Zhipu provider base URL is invalid".to_owned())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Zhipu provider base URL is invalid".to_owned())?;
    Ok((host.to_owned(), port))
}

fn network_resources(host_rules: &[HostRule]) -> Vec<ActionResource> {
    host_rules
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

fn service_credential_context(
    descriptor: &ToolDescriptor,
) -> (Option<String>, Option<CapabilityRouteKind>) {
    descriptor
        .service_binding
        .as_ref()
        .map(|binding| (Some(binding.operation_id.clone()), Some(binding.route_kind)))
        .unwrap_or((None, None))
}

fn descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    service_binding: Option<ToolServiceBinding>,
) -> ToolDescriptor {
    super::with_output_schema(
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
                ToolCapability::NetworkBroker,
            ],
            super::object_schema(
                &["request"],
                json!({
                    "request": {
                        "type": "object",
                        "description": "Zhipu official API request body for this operation."
                    }
                }),
            ),
            service_binding,
        ),
        provider_output_schema(),
    )
}

fn media_descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    service_binding: Option<ToolServiceBinding>,
) -> ToolDescriptor {
    super::with_output_schema(
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
                ToolCapability::NetworkBroker,
                ToolCapability::BlobWriter,
            ],
            super::object_schema(
                &["request"],
                json!({
                    "request": {
                        "type": "object",
                        "description": "Zhipu official API request body for this operation."
                    }
                }),
            ),
            service_binding,
        ),
        provider_output_schema(),
    )
}

fn provider_output_schema() -> Value {
    json!({
        "type": "object",
        "description": "Zhipu provider API response."
    })
}

fn service_binding(
    operation_id: &str,
    route_kind: CapabilityRouteKind,
    output_artifact: ModelModality,
) -> ToolServiceBinding {
    ToolServiceBinding {
        provider_id: ZHIPU_PROVIDER_ID.to_owned(),
        operation_id: operation_id.to_owned(),
        route_kind,
        output_artifact,
    }
}

fn request(input: &Value) -> Result<Value, ValidationError> {
    input
        .get("request")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| ValidationError::from("request object is required"))
}

fn task_id(input: &Value) -> Result<String, ValidationError> {
    input
        .get("taskId")
        .or_else(|| input.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ValidationError::from("taskId string is required"))
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn provider_client_error(error: ZhipuProviderClientError) -> ToolError {
    ToolError::Message(error.to_string())
}

async fn text_to_speech_result(
    content_type: String,
    body: Vec<u8>,
    ctx: &ToolContext,
) -> Result<ToolResult, ToolError> {
    if content_type
        .to_ascii_lowercase()
        .starts_with("text/event-stream")
    {
        return Ok(ToolResult::Structured(json!({
            "content_type": content_type,
            "body": String::from_utf8_lossy(&body).into_owned(),
        })));
    }
    let mime_type = zhipu_audio_mime_type(&content_type)?;
    let blob_ref = write_audio_blob(ctx, body, mime_type).await?;
    Ok(ToolResult::Mixed(vec![ToolResultPart::Artifact {
        artifact_kind: ModelModality::Audio,
        content_type: mime_type.to_owned(),
        blob_ref,
        title: "Generated speech".to_owned(),
        preview: Some("Generated speech".to_owned()),
    }]))
}

fn zhipu_audio_mime_type(content_type: &str) -> Result<&str, ToolError> {
    let normalized = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "audio/wav" | "audio/x-wav" | "audio/wave" => Ok("audio/wav"),
        "audio/pcm" | "audio/l16" | "application/octet-stream" => Ok("audio/pcm"),
        _ => Err(ToolError::Message(format!(
            "Zhipu TTS returned unsupported content type: {content_type}"
        ))),
    }
}

async fn write_audio_blob(
    ctx: &ToolContext,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Result<harness_contracts::BlobRef, ToolError> {
    if bytes.is_empty() {
        return Err(ToolError::Message(
            "Zhipu TTS response was empty".to_owned(),
        ));
    }
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > MAX_ZHIPU_MEDIA_BYTES {
        return Err(ToolError::ResultTooLarge {
            original: size,
            limit: MAX_ZHIPU_MEDIA_BYTES,
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
