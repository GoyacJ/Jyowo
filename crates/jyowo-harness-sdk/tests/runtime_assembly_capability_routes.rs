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

mod session_assembly_filter_chain {
    use std::collections::{BTreeSet, HashSet};

    use async_trait::async_trait;
    use futures::stream;
    use harness_contracts::{
        BudgetMetric, CapabilityRouteKind, DeferPolicy, ModelModality, NetworkAccess,
        OverflowAction, ProviderCapabilityRouteSettings, ProviderRestriction, ToolActionPlan,
        ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolOrigin,
        ToolProperties, ToolResult, ToolSearchMode, ToolServiceBinding, TrustLevel,
        WorkspaceAccess,
    };
    use harness_tool::{
        action_plan_from_permission_check, AuthorizedToolInput, BuiltinToolset, PermissionCheck,
        Tool, ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
    };
    use jyowo_harness_sdk::{Harness, SessionOptions, TenantPolicy};
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn sdk_session_assembly_snapshots_filter_chain_and_deferred_partition() {
        block_on(async {
            let workspace = unique_workspace("sdk-session-assembly-filter-chain");
            let provider = Arc::new(CapabilityScriptedProvider::new(
                ConversationModelCapability::default(),
                vec![vec![ModelStreamEvent::MessageStop]],
            ));
            let registry = ToolRegistry::builder()
                .with_builtin_toolset(BuiltinToolset::Empty)
                .with_tool(Box::new(AssemblyTool::new(
                    "visible_direct",
                    DeferPolicy::AlwaysLoad,
                )))
                .with_tool(Box::new(
                    AssemblyTool::new("deferred_match", DeferPolicy::ForceDefer)
                        .with_group(ToolGroup::Search),
                ))
                .with_tool(Box::new(
                    AssemblyTool::new("missing_capability", DeferPolicy::AlwaysLoad)
                        .with_required_capability(ToolCapability::Custom(
                            "test.missing".to_owned(),
                        )),
                ))
                .with_tool(Box::new(
                    AssemblyTool::new("unrouted_service", DeferPolicy::AlwaysLoad)
                        .with_service_binding(),
                ))
                .with_tool(Box::new(AssemblyTool::new(
                    "tenant_filtered",
                    DeferPolicy::AlwaysLoad,
                )))
                .with_tool(Box::new(
                    AssemblyTool::new("profile_filtered", DeferPolicy::AlwaysLoad)
                        .with_group(ToolGroup::Shell),
                ))
                .build()
                .expect("registry should build");
            let harness = Harness::builder()
                .with_workspace_root(&workspace)
                .with_model_arc(Arc::clone(&provider) as Arc<dyn ModelProvider>)
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(NoopSandbox::new())
                .with_tool_registry(registry)
                .with_provider_capability_routes(ProviderCapabilityRouteSettings {
                    version: 1,
                    routes: Vec::new(),
                })
                .with_capability(
                    ToolCapability::Custom("test.present".to_owned()),
                    Arc::new(()),
                )
                .with_tenant_policy(TenantPolicy {
                    allowed_tools: Some(HashSet::from([
                        "visible_direct".to_owned(),
                        "deferred_match".to_owned(),
                        "missing_capability".to_owned(),
                        "unrouted_service".to_owned(),
                        "profile_filtered".to_owned(),
                        "tool_search".to_owned(),
                    ])),
                    ..TenantPolicy::default()
                })
                .build()
                .await
                .expect("harness should build");

            let session = harness
                .create_session(
                    SessionOptions::new(&workspace)
                        .with_tool_search_mode(ToolSearchMode::Always)
                        .with_tool_profile(ToolProfile::Custom {
                            allowlist: BTreeSet::new(),
                            denylist: BTreeSet::new(),
                            group_allowlist: vec![ToolGroup::Custom("assembly".to_owned())],
                            group_denylist: Vec::new(),
                            mcp_included: true,
                            plugin_included: true,
                        }),
                )
                .await
                .expect("session should be created");
            session
                .run_turn("show assembled tools")
                .await
                .expect("turn should run");

            let tool_names = provider
                .requests()
                .await
                .into_iter()
                .next()
                .and_then(|request| request.tools)
                .map(|tools| tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>())
                .expect("model request should include tool schemas");

            assert_eq!(
                tool_names,
                vec!["visible_direct".to_owned(), "tool_search".to_owned()]
            );
        });
    }

    struct AssemblyTool {
        descriptor: ToolDescriptor,
    }

    impl AssemblyTool {
        fn new(name: &str, defer_policy: DeferPolicy) -> Self {
            Self {
                descriptor: ToolDescriptor {
                    name: name.to_owned(),
                    display_name: name.to_owned(),
                    description: "assembly filter test tool".to_owned(),
                    category: "test".to_owned(),
                    group: ToolGroup::Custom("assembly".to_owned()),
                    version: "0.1.0".to_owned(),
                    input_schema: json!({ "type": "object" }),
                    output_schema: None,
                    dynamic_schema: false,
                    properties: ToolProperties {
                        is_concurrency_safe: true,
                        is_read_only: true,
                        is_destructive: false,
                        long_running: None,
                        defer_policy,
                    },
                    trust_level: TrustLevel::AdminTrusted,
                    required_capabilities: Vec::new(),
                    budget: harness_contracts::ResultBudget {
                        metric: BudgetMetric::Chars,
                        limit: 1_024,
                        on_overflow: OverflowAction::Truncate,
                        preview_head_chars: 256,
                        preview_tail_chars: 256,
                    },
                    provider_restriction: ProviderRestriction::All,
                    origin: ToolOrigin::Builtin,
                    search_hint: None,
                    service_binding: None,
                },
            }
        }

        fn with_group(mut self, group: ToolGroup) -> Self {
            self.descriptor.group = group;
            self
        }

        fn with_required_capability(mut self, capability: ToolCapability) -> Self {
            self.descriptor.required_capabilities.push(capability);
            self
        }

        fn with_service_binding(mut self) -> Self {
            self.descriptor.service_binding = Some(ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.image_generation".to_owned(),
                route_kind: CapabilityRouteKind::ImageGeneration,
                output_artifact: ModelModality::Image,
            });
            self
        }
    }

    #[async_trait]
    impl Tool for AssemblyTool {
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
            _authorized: AuthorizedToolInput,
            _ctx: ToolContext,
        ) -> Result<ToolStream, ToolError> {
            Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Text("ok".to_owned()),
            )])))
        }
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
    #[cfg(feature = "blob-file")]
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
            .with_workspace_root(&workspace)
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
    #![allow(unused_imports, dead_code)]

    use harness_contracts::{
        CapabilityRouteKind, ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
        ProviderCredential, ProviderCredentialResolveContext, ProviderCredentialResolverCap,
        ToolCapability, ToolError,
    };
    use harness_model::{ConversationModelCapability, ModelStreamEvent};
    use harness_tool::{BuiltinToolset, ToolRegistryBuilder};
    #[cfg(feature = "blob-file")]
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

    fn enabled_seedance_video_route() -> ProviderCapabilityRouteSettings {
        ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![ProviderCapabilityRoute {
                kind: CapabilityRouteKind::VideoGeneration,
                config_id: "doubao-seedance-video".to_owned(),
                provider_id: "doubao".to_owned(),
                operation_ids: vec![
                    "seedance.video_generation".to_owned(),
                    "seedance.video_generation.query".to_owned(),
                ],
                enabled: true,
            }],
        }
    }

    #[cfg(feature = "blob-file")]
    async fn session_tool_names(routes: ProviderCapabilityRouteSettings) -> Vec<String> {
        let workspace = unique_workspace("sdk-seedance-capability-route");
        std::fs::create_dir_all(&workspace).unwrap();
        let provider = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("registry should build");
        std::fs::create_dir_all(workspace.join(".jyowo").join("runtime").join("blobs")).unwrap();
        let blob_store =
            FileBlobStore::open(workspace.join(".jyowo").join("runtime").join("blobs"))
                .expect("blob store should open");
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
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
            .run_turn("use seedance service tools")
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
    fn seedance_tools_register_with_default_builtin_toolset() {
        let registry = ToolRegistryBuilder::new()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("registry should build");

        assert!(registry.get("SeedanceTextToVideo").is_some());
        assert!(registry.get("SeedanceImageToVideo").is_some());
        assert!(registry.get("SeedanceVideoGenerationQuery").is_some());
    }

    #[test]
    #[cfg(feature = "blob-file")]
    fn capability_route_exposes_seedance_video_tools_when_route_exists() {
        block_on(async {
            let tool_names = session_tool_names(enabled_seedance_video_route()).await;

            assert!(tool_names.contains(&"SeedanceTextToVideo".to_owned()));
            assert!(tool_names.contains(&"SeedanceImageToVideo".to_owned()));
            assert!(tool_names.contains(&"SeedanceVideoGenerationQuery".to_owned()));
        });
    }

    #[test]
    #[cfg(feature = "blob-file")]
    fn capability_route_hides_seedance_video_tools_without_route() {
        block_on(async {
            let tool_names = session_tool_names(ProviderCapabilityRouteSettings {
                version: 1,
                routes: Vec::new(),
            })
            .await;

            assert!(!tool_names.contains(&"SeedanceTextToVideo".to_owned()));
            assert!(!tool_names.contains(&"SeedanceImageToVideo".to_owned()));
            assert!(!tool_names.contains(&"SeedanceVideoGenerationQuery".to_owned()));
        });
    }
}
