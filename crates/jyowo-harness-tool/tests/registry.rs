use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DeferPolicy, ProviderRestriction, ToolActionPlan, ToolDescriptor, ToolError,
    ToolExecutionChannel, ToolGroup, ToolOrigin, ToolProperties, ToolResult, TrustLevel,
};
use harness_permission::PermissionCheck;
use harness_tool::{
    action_plan_from_permission_check, default_result_budget, AuthorizedToolInput, BuiltinToolset,
    RegistrationError, Tool, ToolContext, ToolEvent, ToolRegistry, ValidationError,
};
use serde_json::{json, Value};

#[test]
fn rejects_non_canonical_tool_names() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .unwrap();

    for name in ["", "bad name", "bad__reserved"] {
        let error = registry
            .register(Box::new(TestTool {
                descriptor: descriptor(name),
            }))
            .unwrap_err();
        assert!(
            matches!(error, RegistrationError::InvalidDescriptor(_)),
            "expected InvalidDescriptor for {name:?}, got {error:?}"
        );
    }

    registry
        .register(Box::new(TestTool {
            descriptor: descriptor("mcp__server__tool"),
        }))
        .expect("canonical MCP tool name should stay valid");
}

struct TestTool {
    descriptor: ToolDescriptor,
}

#[async_trait]
impl Tool for TestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(authorized.raw_input().clone()),
        )])))
    }
}

fn descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: "test".to_owned(),
        description: "test".to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::Custom("test".to_owned()),
        version: "0.1.0".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: Vec::new(),
        budget: default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
        service_binding: None,
        metadata: Default::default(),
    }
}

#[cfg(feature = "minimax-tools")]
mod minimax_service_binding {
    use harness_contracts::{CapabilityRouteKind, ModelModality};
    use harness_tool::{
        provider_service_adapter_availability_from_snapshot, BuiltinToolset,
        MiniMaxImageToImageTool, MiniMaxMusicGenerationTool, MiniMaxTextToImageTool,
        MiniMaxTextToSpeechTool, MiniMaxTextToVideoTool, MiniMaxVideoGenerationQueryTool, Tool,
        ToolRegistryBuilder,
    };

    #[test]
    fn minimax_image_tool_descriptor_has_image_generation_binding() {
        let binding = MiniMaxTextToImageTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("image tool should declare service binding");
        assert_eq!(binding.provider_id, "minimax");
        assert_eq!(binding.operation_id, "minimax.image_generation");
        assert_eq!(binding.route_kind, CapabilityRouteKind::ImageGeneration);
        assert_eq!(binding.output_artifact, ModelModality::Image);

        let image_to_image_tool = MiniMaxImageToImageTool::default();
        let image_to_image = image_to_image_tool
            .descriptor()
            .service_binding
            .as_ref()
            .expect("image-to-image tool should declare service binding");
        assert_eq!(image_to_image.operation_id, "minimax.image_generation");
    }

    #[test]
    fn minimax_video_tools_have_video_generation_bindings() {
        let generation = MiniMaxTextToVideoTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("video generation tool should declare service binding");
        assert_eq!(generation.operation_id, "minimax.video_generation");
        assert_eq!(generation.route_kind, CapabilityRouteKind::VideoGeneration);

        let query = MiniMaxVideoGenerationQueryTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("video query tool should declare service binding");
        assert_eq!(query.operation_id, "minimax.video_generation.query");
        assert_eq!(query.route_kind, CapabilityRouteKind::VideoGeneration);
    }

    #[test]
    fn minimax_tts_tools_have_text_to_speech_bindings() {
        let binding = MiniMaxTextToSpeechTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("tts tool should declare service binding");
        assert_eq!(binding.operation_id, "minimax.text_to_speech.sync");
        assert_eq!(binding.route_kind, CapabilityRouteKind::TextToSpeech);
        assert_eq!(binding.output_artifact, ModelModality::Audio);
    }

    #[test]
    fn adapter_availability_reports_runtime_support_from_descriptors() {
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("default tool registry should build");
        let availability =
            provider_service_adapter_availability_from_snapshot(&registry.snapshot());

        assert!(availability
            .bindings
            .iter()
            .any(|binding| binding.operation_id == "minimax.image_generation"));
        assert!(availability
            .bindings
            .iter()
            .any(|binding| binding.operation_id == "minimax.music_generation"));
    }

    #[test]
    fn adapter_availability_does_not_report_catalog_only_operations() {
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("default tool registry should build");
        let availability =
            provider_service_adapter_availability_from_snapshot(&registry.snapshot());

        assert!(!availability
            .bindings
            .iter()
            .any(|binding| binding.operation_id == "minimax.responses"));
        assert!(!availability
            .bindings
            .iter()
            .any(|binding| binding.operation_id == "minimax.files.upload"));
    }

    #[test]
    fn minimax_music_tool_has_music_generation_binding() {
        let binding = MiniMaxMusicGenerationTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("music tool should declare service binding");
        assert_eq!(binding.operation_id, "minimax.music_generation");
        assert_eq!(binding.route_kind, CapabilityRouteKind::MusicGeneration);
    }
}

#[cfg(feature = "zhipu-tools")]
mod zhipu_service_binding {
    use harness_contracts::{CapabilityRouteKind, ModelModality};
    use harness_tool::{
        provider_service_adapter_availability_from_snapshot, BuiltinToolset, Tool,
        ToolRegistryBuilder, ZhipuImageGenerationQueryTool, ZhipuImageGenerationTool,
        ZhipuSpeechToTextTool, ZhipuTextToSpeechTool, ZhipuVideoGenerationQueryTool,
        ZhipuVideoGenerationTool,
    };

    #[test]
    fn zhipu_media_tools_have_route_bindings() {
        let image = ZhipuImageGenerationTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("image tool should declare service binding");
        assert_eq!(image.provider_id, "zhipu");
        assert_eq!(image.operation_id, "zhipu.image_generation");
        assert_eq!(image.route_kind, CapabilityRouteKind::ImageGeneration);
        assert_eq!(image.output_artifact, ModelModality::Image);

        let image_async = harness_tool::ZhipuImageGenerationAsyncTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("async image tool should declare service binding");
        assert_eq!(image_async.operation_id, "zhipu.image_generation.async");
        assert_eq!(image_async.route_kind, CapabilityRouteKind::ImageGeneration);

        let image_query = ZhipuImageGenerationQueryTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("image query tool should declare service binding");
        assert_eq!(image_query.operation_id, "zhipu.image_generation.query");
        assert_eq!(image_query.route_kind, CapabilityRouteKind::ImageGeneration);

        let video = ZhipuVideoGenerationTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("video tool should declare service binding");
        assert_eq!(video.operation_id, "zhipu.video_generation");
        assert_eq!(video.route_kind, CapabilityRouteKind::VideoGeneration);
        assert_eq!(video.output_artifact, ModelModality::Video);

        let video_query = ZhipuVideoGenerationQueryTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("video query tool should declare service binding");
        assert_eq!(video_query.operation_id, "zhipu.video_generation.query");
        assert_eq!(video_query.route_kind, CapabilityRouteKind::VideoGeneration);

        let tts = ZhipuTextToSpeechTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("tts tool should declare service binding");
        assert_eq!(tts.operation_id, "zhipu.text_to_speech");
        assert_eq!(tts.route_kind, CapabilityRouteKind::TextToSpeech);
        assert_eq!(tts.output_artifact, ModelModality::Audio);

        let stt = ZhipuSpeechToTextTool::default()
            .descriptor()
            .service_binding
            .clone()
            .expect("stt tool should declare service binding");
        assert_eq!(stt.operation_id, "zhipu.speech_to_text");
        assert_eq!(stt.route_kind, CapabilityRouteKind::SpeechToText);
    }

    #[test]
    fn adapter_availability_reports_zhipu_runtime_support_from_descriptors() {
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("default tool registry should build");
        let availability =
            provider_service_adapter_availability_from_snapshot(&registry.snapshot());

        for operation_id in [
            "zhipu.image_generation",
            "zhipu.image_generation.async",
            "zhipu.image_generation.query",
            "zhipu.video_generation",
            "zhipu.video_generation.query",
            "zhipu.text_to_speech",
            "zhipu.speech_to_text",
        ] {
            assert!(
                availability
                    .bindings
                    .iter()
                    .any(|binding| binding.operation_id == operation_id),
                "{operation_id} should have a runtime adapter"
            );
        }
        assert!(!availability
            .bindings
            .iter()
            .any(|binding| binding.operation_id == "zhipu.embeddings"));
    }
}
