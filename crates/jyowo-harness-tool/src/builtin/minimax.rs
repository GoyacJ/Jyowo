use crate::provider_media::{
    download_provider_https_media, safe_mime_types_for_modality, validate_media_bytes,
    BrokerProviderMediaDownloader, ProviderMediaBytes, ProviderMediaDownloadRequest,
    ProviderMediaDownloader, MAX_MINIMAX_MEDIA_BYTES,
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
use url::Url;

use std::sync::Arc;

use crate::provider_minimax::{MinimaxApiClient, MinimaxProviderClientError};
use crate::{
    action_plan_from_permission_check, AuthorizedNetworkPermit, AuthorizedToolInput, Tool,
    ToolContext, ToolEvent, ToolNetworkBrokerCap, ToolStream, ValidationError,
};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const MINIMAX_PROVIDER_ID: &str = "minimax";

macro_rules! minimax_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident) => {
        minimax_tool!(
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
                minimax_network_action_plan(input, ctx, &self.descriptor).await
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

macro_rules! minimax_image_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal) => {
        minimax_image_tool!($type_name, $name, $display_name, $description, None);
    };
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $binding:expr) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: image_descriptor($name, $display_name, $description, $binding),
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
                minimax_network_action_plan(input, ctx, &self.descriptor).await
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
                Ok(execute_image_request(
                    input,
                    ctx,
                    &self.descriptor,
                    permit,
                    broker,
                ))
            }
        }
    };
}

macro_rules! minimax_async_create_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident, $poll_operation_id:literal, $binding:expr) => {
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
                minimax_network_action_plan(input, ctx, &self.descriptor).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                let input = authorized.raw_input().clone();
                let poll_operation_id = $poll_operation_id;
                let artifact_kind = self
                    .descriptor
                    .service_binding
                    .as_ref()
                    .map(|binding| binding.output_artifact)
                    .unwrap_or(ModelModality::Video);
                let permit = authorized.network_permit()?;
                let broker =
                    ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
                Ok(execute_async_create_request(
                    input,
                    ctx,
                    &self.descriptor,
                    poll_operation_id,
                    artifact_kind,
                    $display_name,
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

macro_rules! minimax_sync_media_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident, $artifact_title:literal, $binding:expr) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: media_descriptor($name, $display_name, $description, $binding),
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
                minimax_network_action_plan(input, ctx, &self.descriptor).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                let input = authorized.raw_input().clone();
                let artifact_kind = self
                    .descriptor
                    .service_binding
                    .as_ref()
                    .map(|binding| binding.output_artifact)
                    .unwrap_or(ModelModality::Audio);
                let permit = authorized.network_permit()?;
                let broker =
                    ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
                Ok(execute_sync_media_request(
                    input,
                    ctx,
                    &self.descriptor,
                    artifact_kind,
                    $artifact_title,
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

macro_rules! minimax_media_query_tool {
    ($type_name:ident, $name:literal, $display_name:literal, $description:literal, $operation:ident, $artifact_title:literal, $binding:expr) => {
        #[derive(Clone)]
        pub struct $type_name {
            descriptor: ToolDescriptor,
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self {
                    descriptor: media_descriptor($name, $display_name, $description, $binding),
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

            async fn plan(
                &self,
                input: &Value,
                ctx: &ToolContext,
            ) -> Result<ToolActionPlan, ToolError> {
                minimax_network_action_plan(input, ctx, &self.descriptor).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                let input = authorized.raw_input().clone();
                let artifact_kind = self
                    .descriptor
                    .service_binding
                    .as_ref()
                    .map(|binding| binding.output_artifact)
                    .unwrap_or(ModelModality::Video);
                let permit = authorized.network_permit()?;
                let broker =
                    ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
                Ok(execute_media_query_request(
                    input,
                    ctx,
                    &self.descriptor,
                    artifact_kind,
                    $artifact_title,
                    permit,
                    broker,
                    |client, request| {
                        Box::pin(async move {
                            let task_id =
                                required_string(&request, "task_id").map_err(validation_error)?;
                            client
                                .$operation(&task_id)
                                .await
                                .map_err(provider_client_error)
                        })
                    },
                ))
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
                minimax_network_action_plan(input, ctx, &self.descriptor).await
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

minimax_image_tool!(
    MiniMaxTextToImageTool,
    "MiniMaxTextToImage",
    "MiniMax text to image",
    "Generate an image with MiniMax image generation.",
    Some(service_binding(
        "minimax.image_generation",
        CapabilityRouteKind::ImageGeneration,
        ModelModality::Image,
    ))
);
minimax_image_tool!(
    MiniMaxImageToImageTool,
    "MiniMaxImageToImage",
    "MiniMax image to image",
    "Generate or transform an image with MiniMax image generation.",
    Some(service_binding(
        "minimax.image_generation",
        CapabilityRouteKind::ImageGeneration,
        ModelModality::Image,
    ))
);
minimax_async_create_tool!(
    MiniMaxTextToVideoTool,
    "MiniMaxTextToVideo",
    "MiniMax text to video",
    "Create a MiniMax video generation task from text.",
    video_generation,
    "minimax.video_generation.query",
    Some(service_binding(
        "minimax.video_generation",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_async_create_tool!(
    MiniMaxImageToVideoTool,
    "MiniMaxImageToVideo",
    "MiniMax image to video",
    "Create a MiniMax video generation task from an image reference.",
    video_generation,
    "minimax.video_generation.query",
    Some(service_binding(
        "minimax.video_generation",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_async_create_tool!(
    MiniMaxFirstLastFrameToVideoTool,
    "MiniMaxFirstLastFrameToVideo",
    "MiniMax first last frame video",
    "Create a MiniMax video task from first and last frame references.",
    video_generation,
    "minimax.video_generation.query",
    Some(service_binding(
        "minimax.video_generation",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_async_create_tool!(
    MiniMaxSubjectReferenceVideoTool,
    "MiniMaxSubjectReferenceVideo",
    "MiniMax subject reference video",
    "Create a MiniMax video task with subject reference inputs.",
    video_generation,
    "minimax.video_generation.query",
    Some(service_binding(
        "minimax.video_generation",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_media_query_tool!(
    MiniMaxVideoGenerationQueryTool,
    "MiniMaxVideoGenerationQuery",
    "MiniMax video generation query",
    "Query a MiniMax video generation task.",
    query_video_generation,
    "Generated video",
    Some(service_binding(
        "minimax.video_generation.query",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_async_create_tool!(
    MiniMaxVideoTemplateTool,
    "MiniMaxVideoTemplate",
    "MiniMax video template",
    "Create a MiniMax video template generation task.",
    video_template_generation,
    "minimax.video_template.query",
    Some(service_binding(
        "minimax.video_template",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_media_query_tool!(
    MiniMaxVideoTemplateQueryTool,
    "MiniMaxVideoTemplateQuery",
    "MiniMax video template query",
    "Query a MiniMax video template generation task.",
    query_video_template_generation,
    "Generated video",
    Some(service_binding(
        "minimax.video_template.query",
        CapabilityRouteKind::VideoGeneration,
        ModelModality::Video,
    ))
);
minimax_sync_media_tool!(
    MiniMaxTextToSpeechTool,
    "MiniMaxTextToSpeech",
    "MiniMax text to speech",
    "Generate speech with MiniMax synchronous TTS.",
    text_to_speech,
    "Generated speech",
    Some(service_binding(
        "minimax.text_to_speech.sync",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Audio,
    ))
);
minimax_async_create_tool!(
    MiniMaxTextToSpeechAsyncTool,
    "MiniMaxTextToSpeechAsync",
    "MiniMax async text to speech",
    "Create a MiniMax async long-form TTS task.",
    text_to_speech_async,
    "minimax.text_to_speech.async.query",
    Some(service_binding(
        "minimax.text_to_speech.async",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Audio,
    ))
);
minimax_media_query_tool!(
    MiniMaxTextToSpeechAsyncQueryTool,
    "MiniMaxTextToSpeechAsyncQuery",
    "MiniMax async text to speech query",
    "Query a MiniMax async TTS task.",
    query_text_to_speech_async,
    "Generated speech",
    Some(service_binding(
        "minimax.text_to_speech.async.query",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Audio,
    ))
);
minimax_tool!(
    MiniMaxVoiceCloneTool,
    "MiniMaxVoiceClone",
    "MiniMax voice clone",
    "Clone a voice with MiniMax voice cloning.",
    voice_clone,
    Some(service_binding(
        "minimax.voice_clone",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Audio,
    ))
);
minimax_tool!(
    MiniMaxVoiceDesignTool,
    "MiniMaxVoiceDesign",
    "MiniMax voice design",
    "Design a voice with MiniMax voice design.",
    voice_design,
    Some(service_binding(
        "minimax.voice_design",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Audio,
    ))
);
minimax_tool!(
    MiniMaxListVoicesTool,
    "MiniMaxListVoices",
    "MiniMax list voices",
    "List MiniMax voices.",
    get_voice,
    Some(service_binding(
        "minimax.voice_list",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Text,
    ))
);
minimax_tool!(
    MiniMaxDeleteVoiceTool,
    "MiniMaxDeleteVoice",
    "MiniMax delete voice",
    "Delete a MiniMax voice.",
    delete_voice,
    Some(service_binding(
        "minimax.voice_delete",
        CapabilityRouteKind::TextToSpeech,
        ModelModality::Text,
    ))
);
minimax_tool!(
    MiniMaxLyricsGenerationTool,
    "MiniMaxLyricsGeneration",
    "MiniMax lyrics generation",
    "Generate lyrics with MiniMax music APIs.",
    lyrics_generation,
    Some(service_binding(
        "minimax.lyrics_generation",
        CapabilityRouteKind::MusicGeneration,
        ModelModality::Text,
    ))
);
minimax_sync_media_tool!(
    MiniMaxMusicGenerationTool,
    "MiniMaxMusicGeneration",
    "MiniMax music generation",
    "Generate music with MiniMax music APIs.",
    music_generation,
    "Generated music",
    Some(service_binding(
        "minimax.music_generation",
        CapabilityRouteKind::MusicGeneration,
        ModelModality::Audio,
    ))
);
minimax_tool!(
    MiniMaxMusicCoverPreprocessTool,
    "MiniMaxMusicCoverPreprocess",
    "MiniMax music cover preprocess",
    "Preprocess source audio for MiniMax music cover generation.",
    music_cover_preprocess,
    Some(service_binding(
        "minimax.music_cover_preprocess",
        CapabilityRouteKind::MusicGeneration,
        ModelModality::Audio,
    ))
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
    "Call MiniMax Anthropic protocol messages endpoint.",
    anthropic_messages
);
minimax_tool!(
    MiniMaxAnthropicCountTokensTool,
    "MiniMaxAnthropicCountTokens",
    "MiniMax Anthropic count tokens",
    "Call MiniMax Anthropic protocol count tokens endpoint.",
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
                None,
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

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        minimax_network_action_plan(input, ctx, &self.descriptor).await
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
                        .file_upload_with_group_id(
                            &upload.purpose,
                            &upload.file_name,
                            upload.bytes,
                            upload.group_id.as_deref(),
                        )
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
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
    "Retrieve MiniMax Anthropic protocol model metadata.",
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
                None,
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

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        minimax_network_action_plan(input, ctx, &self.descriptor).await
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
                    let purpose = optional_string(&request, "purpose").map_err(validation_error)?;
                    client
                        .file_list(purpose.as_deref())
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
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
                None,
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

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        minimax_network_action_plan(input, ctx, &self.descriptor).await
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
pub struct MiniMaxAnthropicModelsListTool {
    descriptor: ToolDescriptor,
}

impl Default for MiniMaxAnthropicModelsListTool {
    fn default() -> Self {
        Self {
            descriptor: descriptor(
                "MiniMaxAnthropicModelsList",
                "MiniMax Anthropic models list",
                "List MiniMax Anthropic protocol models.",
                None,
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

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        minimax_network_action_plan(input, ctx, &self.descriptor).await
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
                    let limit = optional_u32(&request, "limit").map_err(validation_error)?;
                    let after_id =
                        optional_string(&request, "after_id").map_err(validation_error)?;
                    let before_id =
                        optional_string(&request, "before_id").map_err(validation_error)?;
                    client
                        .list_anthropic_models(limit, after_id.as_deref(), before_id.as_deref())
                        .await
                        .map_err(provider_client_error)
                })
            },
        ))
    }
}

fn execute_async_create_request<F>(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    poll_operation_id: &'static str,
    artifact_kind: ModelModality,
    title: &'static str,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn crate::ToolNetworkBrokerCap>,
    call: F,
) -> ToolStream
where
    F: for<'a> FnOnce(&'a MinimaxApiClient, Value) -> BoxFuture<'a, Result<Value, ToolError>>
        + Send
        + 'static,
{
    let (operation_id, route_kind) = service_credential_context(descriptor);
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                MinimaxApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = call(&client, request).await?;
            async_job_tool_result(&response, poll_operation_id, artifact_kind, title)
        }
        .await;
        match result {
            Ok(result) => ToolEvent::Final(result),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

fn execute_sync_media_request<F>(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    artifact_kind: ModelModality,
    title: &'static str,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn crate::ToolNetworkBrokerCap>,
    call: F,
) -> ToolStream
where
    F: for<'a> FnOnce(&'a MinimaxApiClient, Value) -> BoxFuture<'a, Result<Value, ToolError>>
        + Send
        + 'static,
{
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let media_operation_id = operation_id.clone();
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                MinimaxApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = call(&client, request).await?;
            let media_operation_id = media_operation_id.as_deref().ok_or_else(|| {
                ToolError::PermissionDenied(
                    "MiniMax media operation credential context is incomplete".to_owned(),
                )
            })?;
            media_tool_result_from_response(
                response,
                &ctx,
                media_operation_id,
                artifact_kind,
                title,
                &BrokerProviderMediaDownloader::new(
                    Arc::clone(&broker),
                    client.broker_permit().map_err(provider_client_error)?,
                ),
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

fn execute_media_query_request<F>(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    artifact_kind: ModelModality,
    title: &'static str,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn crate::ToolNetworkBrokerCap>,
    call: F,
) -> ToolStream
where
    F: for<'a> FnOnce(&'a MinimaxApiClient, Value) -> BoxFuture<'a, Result<Value, ToolError>>
        + Send
        + 'static,
{
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let media_operation_id = operation_id.clone();
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                MinimaxApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            let response = call(&client, request).await?;
            let media_operation_id = media_operation_id.as_deref().ok_or_else(|| {
                ToolError::PermissionDenied(
                    "MiniMax media operation credential context is incomplete".to_owned(),
                )
            })?;
            query_tool_result_from_response(
                response,
                &ctx,
                media_operation_id,
                artifact_kind,
                title,
                &BrokerProviderMediaDownloader::new(
                    Arc::clone(&broker),
                    client.broker_permit().map_err(provider_client_error)?,
                ),
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

fn execute_request<F>(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn crate::ToolNetworkBrokerCap>,
    call: F,
) -> ToolStream
where
    F: for<'a> FnOnce(&'a MinimaxApiClient, Value) -> BoxFuture<'a, Result<Value, ToolError>>
        + Send
        + 'static,
{
    let (operation_id, route_kind) = service_credential_context(descriptor);
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx, operation_id, route_kind).await?;
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                MinimaxApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = credential.base_url {
                client = client.with_base_url(base_url);
            }
            call(&client, request).await
        }
        .await;
        match result {
            Ok(value) => ToolEvent::Final(ToolResult::Structured(value)),
            Err(error) => ToolEvent::Error(error),
        }
    }))
}

fn execute_image_request(
    input: Value,
    ctx: ToolContext,
    descriptor: &ToolDescriptor,
    permit: AuthorizedNetworkPermit,
    broker: Arc<dyn crate::ToolNetworkBrokerCap>,
) -> ToolStream {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let media_operation_id = operation_id.clone();
    Box::pin(stream::once(async move {
        let result = async {
            let credential = minimax_credential(&ctx, operation_id, route_kind).await?;
            let base_url = credential.base_url.clone();
            let request = request(&input).map_err(validation_error)?;
            let mut client =
                MinimaxApiClient::from_broker(Arc::clone(&broker), permit, credential.api_key);
            if let Some(base_url) = &base_url {
                client = client.with_base_url(base_url.clone());
            }
            let response = client
                .image_generation(request)
                .await
                .map_err(provider_client_error)?;
            let media_operation_id = media_operation_id.as_deref().ok_or_else(|| {
                ToolError::PermissionDenied(
                    "MiniMax media operation credential context is incomplete".to_owned(),
                )
            })?;
            let downloader = BrokerProviderMediaDownloader::new(
                Arc::clone(&broker),
                client.broker_permit().map_err(provider_client_error)?,
            );
            image_tool_result_from_response(
                response,
                &ctx,
                base_url.as_deref(),
                media_operation_id,
                &downloader,
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

async fn image_tool_result_from_response(
    response: Value,
    ctx: &ToolContext,
    provider_base_url: Option<&str>,
    operation_id: &str,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ToolResult, ToolError> {
    image_tool_result_from_response_with_downloader(
        response,
        ctx,
        provider_base_url,
        operation_id,
        downloader,
    )
    .await
}

async fn image_tool_result_from_response_with_downloader(
    response: Value,
    ctx: &ToolContext,
    _provider_base_url: Option<&str>,
    operation_id: &str,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ToolResult, ToolError> {
    media_tool_result_from_response(
        response,
        ctx,
        operation_id,
        ModelModality::Image,
        "Generated image",
        downloader,
    )
    .await
}

async fn media_tool_result_from_response(
    response: Value,
    ctx: &ToolContext,
    operation_id: &str,
    modality: ModelModality,
    title: &str,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ToolResult, ToolError> {
    let candidate = select_media_candidate(&response, modality).ok_or_else(|| {
        ToolError::Message(format!(
            "MiniMax {} response did not include supported media",
            modality_label(modality)
        ))
    })?;
    let media = resolve_media_candidate(candidate, operation_id, modality, downloader).await?;
    let mime_type = validate_media_bytes(&media.bytes, modality, Some(&media.mime_type))?;
    let blob_ref = write_media_blob(ctx, media.bytes, &mime_type).await?;
    Ok(artifact_tool_result(modality, mime_type, blob_ref, title))
}

async fn query_tool_result_from_response(
    response: Value,
    ctx: &ToolContext,
    operation_id: &str,
    modality: ModelModality,
    title: &str,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ToolResult, ToolError> {
    if let Some(candidate) = select_media_candidate(&response, modality) {
        let media = resolve_media_candidate(candidate, operation_id, modality, downloader).await?;
        let mime_type = validate_media_bytes(&media.bytes, modality, Some(&media.mime_type))?;
        let blob_ref = write_media_blob(ctx, media.bytes, &mime_type).await?;
        return Ok(artifact_tool_result(modality, mime_type, blob_ref, title));
    }
    if is_pending_task_status(&response) {
        return Ok(ToolResult::Structured(response));
    }
    Ok(ToolResult::Structured(response))
}

fn async_job_tool_result(
    response: &Value,
    poll_operation_id: &str,
    artifact_kind: ModelModality,
    title: &str,
) -> Result<ToolResult, ToolError> {
    let job_id = extract_task_id(response).ok_or_else(|| {
        ToolError::Message("MiniMax async task response did not include task id".to_owned())
    })?;
    Ok(ToolResult::Mixed(vec![ToolResultPart::Structured {
        value: json!({
            "kind": "async_job",
            "jobId": job_id,
            "pollOperationId": poll_operation_id,
            "artifactKind": modality_label(artifact_kind),
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

fn modality_label(modality: ModelModality) -> &'static str {
    match modality {
        ModelModality::Image => "image",
        ModelModality::Video => "video",
        ModelModality::Audio => "audio",
        ModelModality::File => "file",
        ModelModality::Text | ModelModality::Embedding => "text",
    }
}

fn extract_task_id(value: &Value) -> Option<String> {
    value
        .get("task_id")
        .or_else(|| value.pointer("/data/task_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
        .map(ToOwned::to_owned)
}

fn is_pending_task_status(value: &Value) -> bool {
    if let Some(status) = value.get("status").and_then(Value::as_str) {
        return matches!(
            status.to_ascii_lowercase().as_str(),
            "processing" | "pending" | "running" | "queueing" | "preparing"
        );
    }
    false
}

async fn write_media_blob(
    ctx: &ToolContext,
    bytes: Vec<u8>,
    mime_type: &str,
) -> Result<harness_contracts::BlobRef, ToolError> {
    if bytes.is_empty() {
        return Err(ToolError::Message(
            "MiniMax media response was empty".to_owned(),
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
                ModelModality::Image => "data:image/",
                ModelModality::Video => "data:video/",
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
        ModelModality::Image => key.contains("base64") || key == "image" || key == "image_data",
        ModelModality::Video => key.contains("base64") || key == "video_data",
        ModelModality::Audio => key.contains("base64") || key == "audio" || key == "audio_data",
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => false,
    }
}

fn is_likely_media_url_key(key: &str, modality: ModelModality) -> bool {
    let key = key.to_ascii_lowercase();
    match modality {
        ModelModality::Image => {
            key.contains("url") && (key.contains("image") || key.contains("file") || key == "url")
        }
        ModelModality::Video => {
            key.contains("url") && (key.contains("video") || key.contains("file") || key == "url")
        }
        ModelModality::Audio => {
            key.contains("url") && (key.contains("audio") || key.contains("music") || key == "url")
        }
        ModelModality::Text | ModelModality::Embedding | ModelModality::File => false,
    }
}

async fn resolve_media_candidate(
    candidate: MediaCandidate,
    operation_id: &str,
    modality: ModelModality,
    downloader: &dyn ProviderMediaDownloader,
) -> Result<ProviderMediaBytes, ToolError> {
    match candidate {
        MediaCandidate::DataUrl(value) => decode_data_url_media(&value, modality),
        MediaCandidate::Base64(value) => decode_base64_media(&value, modality),
        MediaCandidate::HttpsUrl(value) => {
            download_provider_https_media(
                ProviderMediaDownloadRequest {
                    provider_id: MINIMAX_PROVIDER_ID,
                    operation_id,
                    url: &value,
                    artifact_kind: modality,
                    expected_mime_types: safe_mime_types_for_modality(modality),
                    max_bytes: MAX_MINIMAX_MEDIA_BYTES,
                },
                downloader,
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
        .ok_or_else(|| ToolError::Message("MiniMax media data URL is malformed".to_owned()))?;
    let metadata = &value["data:".len()..comma];
    let mut parts = metadata.split(';');
    let mime_type = parts.next().unwrap_or_default().trim();
    let is_base64 = parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let mime_type = crate::provider_media::safe_mime_for_modality(mime_type, modality)
        .ok_or_else(|| ToolError::Message("MiniMax media data URL is unsupported".to_owned()))?;
    if !is_base64 {
        return Err(ToolError::Message(
            "MiniMax media data URL is unsupported".to_owned(),
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
        .map_err(|_| ToolError::Message("MiniMax media payload is not valid base64".to_owned()))
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

async fn minimax_network_action_plan(
    input: &Value,
    ctx: &ToolContext,
    descriptor: &ToolDescriptor,
) -> Result<ToolActionPlan, ToolError> {
    let (operation_id, route_kind) = service_credential_context(descriptor);
    let downloads_media = minimax_operation_downloads_media(operation_id.as_deref());
    let credential = match minimax_credential(ctx, operation_id, route_kind).await {
        Ok(credential) => credential,
        Err(error) => {
            return action_plan_from_permission_check(
                descriptor,
                input,
                ctx,
                minimax_permission_denied(error),
                Vec::new(),
                WorkspaceAccess::None,
                NetworkAccess::None,
                ToolExecutionChannel::HttpBroker,
            );
        }
    };

    match minimax_base_url_host(credential.base_url.as_deref()) {
        Ok((host, port)) => {
            let host_rules = minimax_host_rules(&host, port, downloads_media);
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

fn minimax_operation_downloads_media(operation_id: Option<&str>) -> bool {
    matches!(
        operation_id,
        Some(
            "minimax.image_generation"
                | "minimax.video_generation.query"
                | "minimax.video_template.query"
                | "minimax.text_to_speech.sync"
                | "minimax.text_to_speech.async.query"
                | "minimax.music_generation"
        )
    )
}

fn minimax_host_rules(base_host: &str, base_port: u16, include_media_hosts: bool) -> Vec<HostRule> {
    let mut rules = Vec::new();
    push_host_rule(&mut rules, base_host, base_port);
    if include_media_hosts {
        for host in ["*.minimaxi.com", "*.minimax.io", "*.minimax.chat"] {
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

fn minimax_base_url_host(base_url: Option<&str>) -> Result<(String, u16), String> {
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
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "MiniMax provider base URL is invalid".to_owned())?;
    Ok((host.to_owned(), port))
}

fn service_binding(
    operation_id: &str,
    route_kind: CapabilityRouteKind,
    output_artifact: ModelModality,
) -> ToolServiceBinding {
    ToolServiceBinding {
        provider_id: MINIMAX_PROVIDER_ID.to_owned(),
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
                    "description": "MiniMax official API request body for this operation."
                }
            }),
        ),
        service_binding,
    )
}

fn media_descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    service_binding: Option<ToolServiceBinding>,
) -> ToolDescriptor {
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
                    "description": "MiniMax official API request body for this operation."
                }
            }),
        ),
        service_binding,
    )
}

fn image_descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    service_binding: Option<ToolServiceBinding>,
) -> ToolDescriptor {
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
                    "description": "MiniMax official API request body for image generation."
                }
            }),
        ),
        service_binding,
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

fn service_credential_context(
    descriptor: &ToolDescriptor,
) -> (Option<String>, Option<CapabilityRouteKind>) {
    descriptor
        .service_binding
        .as_ref()
        .map(|binding| (Some(binding.operation_id.clone()), Some(binding.route_kind)))
        .unwrap_or((None, None))
}

async fn minimax_credential(
    ctx: &ToolContext,
    operation_id: Option<String>,
    route_kind: Option<CapabilityRouteKind>,
) -> Result<ProviderCredential, ToolError> {
    if operation_id.is_some() != route_kind.is_some() {
        return Err(ToolError::PermissionDenied(
            "MiniMax service operation credential context is incomplete".to_owned(),
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
            provider_id: MINIMAX_PROVIDER_ID.to_owned(),
            model_config_id: operation_id
                .is_none()
                .then(|| ctx.model_config_id.clone())
                .flatten(),
            operation_id: operation_id.clone(),
            route_kind,
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

fn provider_client_error(error: MinimaxProviderClientError) -> ToolError {
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
        AgentId, BlobMeta, BlobRef, BlobWriterCap, CapabilityRegistry, CorrelationId,
        ModelModality, RunId, SessionId, TenantId, ToolResultPart, ToolUseId,
    };

    const PNG_1X1_BASE64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

    #[tokio::test]
    async fn minimax_credential_rejects_incomplete_service_context_before_resolver() {
        let resolver = Arc::new(PanickingResolver);
        let mut caps = CapabilityRegistry::default();
        caps.install(ToolCapability::ProviderCredentialResolver, resolver);
        let ctx = test_context(Arc::new(CapturingBlobWriter));

        let error = minimax_credential(&ctx, Some("minimax.image_generation".to_owned()), None)
            .await
            .expect_err("partial service context should fail closed");

        assert!(matches!(error, ToolError::PermissionDenied(_)));
        assert!(error.to_string().contains("incomplete"));
    }

    #[tokio::test]
    async fn minimax_image_typed_artifact_from_base64_response() {
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
            "minimax.image_generation",
            &FakeImageDownloader,
        )
        .await
        .expect("image result is extracted");

        let ToolResult::Mixed(parts) = result else {
            panic!("expected mixed result, got {result:?}");
        };
        assert!(parts.iter().any(|part| matches!(
            part,
            ToolResultPart::Artifact {
                artifact_kind: ModelModality::Image,
                content_type,
                blob_ref,
                title,
                preview,
            } if content_type == "image/png"
                && blob_ref.content_type.as_deref() == Some("image/png")
                && title == "Generated image"
                && preview.as_deref() == Some("Generated image")
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
            "minimax.image_generation",
            &FakeImageDownloader,
        )
        .await
        .expect_err("disallowed host is rejected");

        assert!(error
            .to_string()
            .contains("provider media asset host is not allowed"));
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
            "minimax.image_generation",
            &downloader,
        )
        .await
        .expect_err("provider base URL host is not an allowed image asset host");

        assert!(error
            .to_string()
            .contains("provider media asset host is not allowed"));
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
                    "image": format!("{}{}{}", "data:", "image/svg+xml;base64,", svg)
                }
            }),
            &ctx,
            None,
            "minimax.image_generation",
            &FakeImageDownloader,
        )
        .await
        .expect_err("svg data URL should be rejected");

        assert!(error.to_string().contains("unsupported"));
    }

    #[tokio::test]
    async fn image_response_rejects_oversized_inline_base64_before_decoding() {
        let ctx = test_context(Arc::new(CapturingBlobWriter));
        let oversized_base64 = "A".repeat(((MAX_MINIMAX_MEDIA_BYTES + 3) / 3 * 4) as usize);
        let error = image_tool_result_from_response(
            json!({
                "data": {
                    "image_base64": oversized_base64
                }
            }),
            &ctx,
            None,
            "minimax.image_generation",
            &FakeImageDownloader,
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
            "minimax.image_generation",
            &downloader,
        )
        .await
        .expect_err("svg asset should be rejected");

        assert!(error.to_string().contains("supported media"));
    }

    #[tokio::test]
    async fn image_response_allowed_https_asset_is_downloaded_as_artifact_result() {
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
            "minimax.image_generation",
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
            ToolResultPart::Artifact {
                artifact_kind: ModelModality::Image,
                content_type,
                blob_ref,
                title,
                ..
            } if content_type == "image/png"
                && blob_ref.content_type.as_deref() == Some("image/png")
                && title == "Generated image"
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
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(caps),
            redactor: Arc::new(harness_contracts::NoopRedactor),
            interrupt: crate::InterruptToken::new(),
            parent_run: None,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            actor_source: harness_contracts::PermissionActorSource::ParentRun,
        }
    }

    struct FakeImageDownloader;

    #[async_trait::async_trait]
    impl ProviderMediaDownloader for FakeImageDownloader {
        async fn download(
            &self,
            _url: &url::Url,
            _max_bytes: u64,
            modality: ModelModality,
        ) -> Result<ProviderMediaBytes, ToolError> {
            assert_eq!(modality, ModelModality::Image);
            Ok(ProviderMediaBytes {
                bytes: general_purpose::STANDARD.decode(PNG_1X1_BASE64).unwrap(),
                mime_type: "image/png".to_owned(),
            })
        }
    }

    struct FakeSvgImageDownloader;

    #[async_trait::async_trait]
    impl ProviderMediaDownloader for FakeSvgImageDownloader {
        async fn download(
            &self,
            _url: &url::Url,
            _max_bytes: u64,
            _modality: ModelModality,
        ) -> Result<ProviderMediaBytes, ToolError> {
            Ok(ProviderMediaBytes {
                bytes: br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec(),
                mime_type: "image/svg+xml".to_owned(),
            })
        }
    }

    struct CapturingBlobWriter;

    struct PanickingResolver;

    impl ProviderCredentialResolverCap for PanickingResolver {
        fn resolve_provider_credential(
            &self,
            _context: ProviderCredentialResolveContext,
        ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
            panic!("credential resolver must not be called for incomplete service context");
        }
    }

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
}
