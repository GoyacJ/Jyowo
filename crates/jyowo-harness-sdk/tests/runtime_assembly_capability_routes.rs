#![cfg(feature = "testing")]

mod runtime_assembly_support;
#[allow(unused_imports)]
use runtime_assembly_support::*;

mod capability_route_filter {
    use async_trait::async_trait;
    use futures::stream;
    use harness_contracts::{
        CapabilityRouteKind, ConversationModelCapability, DeferPolicy, ModelModality,
        NetworkAccess, ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
        ProviderRestriction, ToolActionPlan, ToolDescriptor, ToolError, ToolExecutionChannel,
        ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolServiceBinding, TrustLevel,
        WorkspaceAccess,
    };
    use harness_tool::{
        action_plan_from_permission_check, default_result_budget, AuthorizedToolInput,
        BuiltinToolset, PermissionCheck, Tool, ToolContext, ToolEvent, ToolPoolFilter,
        ToolRegistry, ToolRegistryBuilder, ToolStream, ValidationError,
    };
    use jyowo_harness_sdk::filter_unrouted_service_tools;
    use serde_json::{json, Value};

    struct RouteFilterTestTool {
        descriptor: ToolDescriptor,
    }

    fn descriptor(name: &str, service_binding: Option<ToolServiceBinding>) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_owned(),
            display_name: name.to_owned(),
            description: name.to_owned(),
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
            service_binding,
        }
    }

    fn service_binding() -> ToolServiceBinding {
        ToolServiceBinding {
            provider_id: "minimax".to_owned(),
            operation_id: "minimax.image_generation".to_owned(),
            route_kind: CapabilityRouteKind::ImageGeneration,
            output_artifact: ModelModality::Image,
        }
    }

    fn empty_routes() -> ProviderCapabilityRouteSettings {
        ProviderCapabilityRouteSettings {
            version: 1,
            routes: Vec::new(),
        }
    }

    fn enabled_image_route() -> ProviderCapabilityRouteSettings {
        ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![ProviderCapabilityRoute {
                kind: CapabilityRouteKind::ImageGeneration,
                config_id: "minimax-image".to_owned(),
                provider_id: "minimax".to_owned(),
                operation_ids: vec!["minimax.image_generation".to_owned()],
                enabled: true,
            }],
        }
    }

    fn registry_with_tools() -> ToolRegistry {
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .build()
            .expect("empty tool registry should build");
        registry
            .register(Box::new(RouteFilterTestTool {
                descriptor: descriptor("plain_tool", None),
            }))
            .expect("plain tool registers");
        registry
            .register(Box::new(RouteFilterTestTool {
                descriptor: descriptor("service_tool", Some(service_binding())),
            }))
            .expect("service tool registers");
        registry
    }

    #[async_trait]
    impl Tool for RouteFilterTestTool {
        fn descriptor(&self) -> &ToolDescriptor {
            &self.descriptor
        }

        async fn validate(
            &self,
            _input: &Value,
            _ctx: &ToolContext,
        ) -> Result<(), ValidationError> {
            Ok(())
        }

        async fn plan(
            &self,
            input: &Value,
            ctx: &ToolContext,
        ) -> Result<ToolActionPlan, ToolError> {
            action_plan_from_permission_check(
                self.descriptor(),
                input,
                ctx,
                PermissionCheck::Allowed,
                Vec::new(),
                WorkspaceAccess::None,
                NetworkAccess::None,
                ToolExecutionChannel::DirectAuthorizedRust,
            )
        }

        async fn execute_authorized(
            &self,
            authorized: AuthorizedToolInput,
            _ctx: ToolContext,
        ) -> Result<ToolStream, ToolError> {
            Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Structured(authorized.raw_input().clone()),
            )])))
        }
    }

    #[test]
    fn capability_route_filter_denies_service_bound_tools_without_enabled_route() {
        let registry = registry_with_tools();
        let snapshot = registry.snapshot();
        let mut filter = ToolPoolFilter::default();
        filter_unrouted_service_tools(&mut filter, &snapshot, &empty_routes());

        assert!(filter.denylist.contains("service_tool"));
        assert!(!filter.denylist.contains("plain_tool"));
    }

    #[test]
    fn capability_route_filter_allows_service_bound_tools_for_matching_route() {
        let registry = registry_with_tools();
        let snapshot = registry.snapshot();
        let mut filter = ToolPoolFilter::default();
        filter_unrouted_service_tools(&mut filter, &snapshot, &enabled_image_route());

        assert!(!filter.denylist.contains("service_tool"));
        assert!(!filter.denylist.contains("plain_tool"));
    }

    #[test]
    fn capability_route_filter_leaves_non_service_tools_unaffected() {
        let registry = registry_with_tools();
        let snapshot = registry.snapshot();
        let mut filter = ToolPoolFilter::default();
        filter_unrouted_service_tools(&mut filter, &snapshot, &empty_routes());

        assert!(!filter.denylist.contains("plain_tool"));
    }

    #[test]
    fn capability_route_filter_does_not_replace_tool_calling_model_gate() {
        let mut capability = ConversationModelCapability::default();
        capability.tool_calling = false;

        assert!(!capability.tool_calling);
    }
}

#[cfg(feature = "minimax-tools")]
mod capability_route {
    use harness_contracts::{
        CapabilityRouteKind, ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
        ProviderCredential, ProviderCredentialResolveContext, ProviderCredentialResolverCap,
        ToolCapability, ToolError,
    };
    use harness_model::{ConversationModelCapability, ModelStreamEvent};
    use jyowo_harness_sdk::builtin::FileBlobStore;

    use super::*;

    struct StubCredentialResolver;

    #[async_trait]
    impl ProviderCredentialResolverCap for StubCredentialResolver {
        fn resolve_provider_credential(
            &self,
            context: ProviderCredentialResolveContext,
        ) -> futures::future::BoxFuture<'_, Result<ProviderCredential, ToolError>> {
            Box::pin(async move {
                Ok(ProviderCredential {
                    provider_id: context.provider_id,
                    config_id: "test-config".to_owned(),
                    api_key: "test-key".to_owned(),
                    base_url: None,
                })
            })
        }
    }

    fn enabled_image_route() -> ProviderCapabilityRouteSettings {
        ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![ProviderCapabilityRoute {
                kind: CapabilityRouteKind::ImageGeneration,
                config_id: "minimax-image".to_owned(),
                provider_id: "minimax".to_owned(),
                operation_ids: vec!["minimax.image_generation".to_owned()],
                enabled: true,
            }],
        }
    }

    fn enabled_video_route() -> ProviderCapabilityRouteSettings {
        ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![ProviderCapabilityRoute {
                kind: CapabilityRouteKind::VideoGeneration,
                config_id: "minimax-video".to_owned(),
                provider_id: "minimax".to_owned(),
                operation_ids: vec![
                    "minimax.video_generation".to_owned(),
                    "minimax.video_generation.query".to_owned(),
                ],
                enabled: true,
            }],
        }
    }

    fn enabled_tts_route() -> ProviderCapabilityRouteSettings {
        ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![ProviderCapabilityRoute {
                kind: CapabilityRouteKind::TextToSpeech,
                config_id: "minimax-tts".to_owned(),
                provider_id: "minimax".to_owned(),
                operation_ids: vec!["minimax.text_to_speech.sync".to_owned()],
                enabled: true,
            }],
        }
    }

    async fn session_tool_names(
        routes: ProviderCapabilityRouteSettings,
        capabilities: ConversationModelCapability,
    ) -> Vec<String> {
        let workspace = unique_workspace("sdk-capability-route");
        std::fs::create_dir_all(&workspace).unwrap();
        let provider = Arc::new(CapabilityScriptedProvider::new(
            capabilities,
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("registry should build");
        std::fs::create_dir_all(workspace.join(".jyowo").join("runtime").join("blobs")).unwrap();
        let blob_store =
            FileBlobStore::open(workspace.join(".jyowo").join("runtime").join("blobs"))
                .expect("blob store should open");
        let harness = Harness::builder()
            .with_model_arc(Arc::clone(&provider) as Arc<dyn ModelProvider>)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_blob_store(blob_store)
            .with_tool_registry(registry)
            .with_provider_capability_routes(routes)
            .with_capability(
                ToolCapability::ProviderCredentialResolver,
                Arc::new(StubCredentialResolver),
            )
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should be created");
        session
            .run_turn("use service tools")
            .await
            .expect("turn should run");

        provider
            .requests()
            .await
            .into_iter()
            .next()
            .and_then(|request| request.tools)
            .map(|tools| tools.into_iter().map(|tool| tool.name).collect())
            .unwrap_or_default()
    }

    #[test]
    fn capability_route_exposes_image_tool_when_route_enabled() {
        block_on(async {
            let tool_names = session_tool_names(
                enabled_image_route(),
                ConversationModelCapability::default(),
            )
            .await;
            assert!(tool_names.contains(&"MiniMaxTextToImage".to_owned()));
        });
    }

    #[test]
    fn capability_route_hides_image_tool_without_route() {
        block_on(async {
            let tool_names = session_tool_names(
                ProviderCapabilityRouteSettings {
                    version: 1,
                    routes: Vec::new(),
                },
                ConversationModelCapability::default(),
            )
            .await;
            assert!(!tool_names.contains(&"MiniMaxTextToImage".to_owned()));
        });
    }

    #[test]
    fn capability_route_hides_service_tools_when_model_disallows_tool_calling() {
        block_on(async {
            let mut capability = ConversationModelCapability::default();
            capability.tool_calling = false;
            let tool_names = session_tool_names(enabled_image_route(), capability).await;
            assert!(tool_names.is_empty());
        });
    }

    #[test]
    fn capability_route_exposes_video_tools_when_video_route_exists() {
        block_on(async {
            let tool_names = session_tool_names(
                enabled_video_route(),
                ConversationModelCapability::default(),
            )
            .await;
            assert!(tool_names.contains(&"MiniMaxTextToVideo".to_owned()));
            assert!(tool_names.contains(&"MiniMaxVideoGenerationQuery".to_owned()));
        });
    }

    #[test]
    fn capability_route_exposes_tts_tools_when_tts_route_exists() {
        block_on(async {
            let tool_names =
                session_tool_names(enabled_tts_route(), ConversationModelCapability::default())
                    .await;
            assert!(tool_names.contains(&"MiniMaxTextToSpeech".to_owned()));
        });
    }
}

#[cfg(feature = "seedance-tools")]
mod seedance_runtime {
    use harness_tool::{BuiltinToolset, ToolRegistryBuilder};

    #[test]
    fn seedance_tools_register_with_default_builtin_toolset() {
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("registry should build");

        assert!(registry.get("SeedanceTextToVideo").is_some());
        assert!(registry.get("SeedanceImageToVideo").is_some());
        assert!(registry.get("SeedanceVideoGenerationQuery").is_some());
    }
}
