use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ToolCapability, ToolDescriptor, ToolError, ToolGroup,
    ToolResult,
};
use harness_model::MinimaxApiClient;
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use url::Url;

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const MINIMAX_PROVIDER_ID: &str = "minimax";

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

minimax_tool!(
    MiniMaxTextToImageTool,
    "MiniMaxTextToImage",
    "MiniMax text to image",
    "Generate an image with MiniMax image generation.",
    image_generation
);
minimax_tool!(
    MiniMaxImageToImageTool,
    "MiniMaxImageToImage",
    "MiniMax image to image",
    "Generate or transform an image with MiniMax image generation.",
    image_generation
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
