#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{executor::block_on, StreamExt};
use harness_contracts::{
    Decision, DecisionId, DecisionLifetime, DecisionMatcherKind, DecisionMatcherSummary,
    DecisionScope, Event, FallbackPolicy, HookEventKind, InteractivityLevel, McpServerId,
    ModelError, PermissionDecisionOption, PermissionError, PermissionMode, PermissionOptionId,
    PermissionSubject, RequestId, RuleSource, Severity, ToolUseId,
};
use harness_mcp::ElicitationHandler;
use harness_model::{
    AuxModelProvider, AuxOptions, AuxTask, InferContext, InferMiddleware, ModelRequest,
};
use harness_permission::{
    DecisionHistory, DecisionLookup, DecisionPersistence, PermissionBroker, PermissionContext,
    PermissionRequest, PermissionRule, PersistedDecision, ResolverHandle, RuleAction, RuleProvider,
    StreamBasedBroker, StreamBrokerConfig,
};
use jyowo_harness_sdk::{builtin::*, prelude::*, testing::*};
use parking_lot::Mutex;
use serde_json::json;

#[test]
fn harness_builder_creates_testing_harness_and_session() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-session");
        std::fs::create_dir_all(&workspace).unwrap();

        let model = Arc::new(TestModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model_provider)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("complete type-state builder should build");

        let session_id = SessionId::new();
        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created through facade");

        assert_eq!(
            session.paths().runtime_sessions,
            workspace
                .canonicalize()
                .unwrap()
                .join("runtime")
                .join("sessions")
        );

        let events: Vec<_> = harness
            .event_store()
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("session events should be readable")
            .collect()
            .await;
        assert!(matches!(events.first(), Some(Event::SessionCreated(_))));

        session
            .run_turn("hello from sdk facade")
            .await
            .expect("facade-created session should have runnable turn runtime");

        assert_eq!(model.requests().await.len(), 1);
    });
}

#[test]
fn harness_builder_accepts_full_facade_dependencies_and_overrides() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-facade-full-builder");
        std::fs::create_dir_all(&workspace).unwrap();

        let first_policy = TenantPolicy::default();
        let second_policy = TenantPolicy {
            display_name: "second".to_owned(),
            ..TenantPolicy::default()
        };
        let first_model = Arc::new(TestModelProvider::default());
        let first_model_provider: Arc<dyn ModelProvider> = first_model.clone();
        let second_model = Arc::new(TestModelProvider::default());
        let second_model_provider: Arc<dyn ModelProvider> = second_model.clone();
        let first_store: Arc<dyn EventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let second_store: Arc<dyn EventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let first_sandbox: Arc<dyn SandboxBackend> = Arc::new(NoopSandbox::new());
        let second_sandbox: Arc<dyn SandboxBackend> = Arc::new(NoopSandbox::new());
        let first_memory = Arc::new(InMemoryMemoryProvider::new("first-memory"));
        let second_memory = Arc::new(InMemoryMemoryProvider::new("second-memory"));
        let aux = TestAuxModelProvider::default();
        let tool_registry = ToolRegistry::builder()
            .with_tool(Box::new(TestTool::new("test_tool")))
            .build()
            .expect("tool registry should build");
        let hook_registry = HookRegistry::builder()
            .with_hook(Box::new(TestHookHandler::new(
                "test-hook",
                vec![HookEventKind::UserPromptSubmit],
            )))
            .build()
            .expect("hook registry should build");

        let builder = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_id("first-model")
            .with_model_arc(first_model_provider)
            .with_model_arc(second_model_provider)
            .with_model_id("test-model")
            .with_store_arc(first_store)
            .with_store_arc(second_store)
            .with_sandbox_arc(first_sandbox)
            .with_sandbox_arc(second_sandbox)
            .with_tenant_policy(first_policy)
            .with_tenant_policy(second_policy)
            .with_memory_provider_arc(first_memory)
            .with_memory_provider_arc(second_memory)
            .with_blob_store(InMemoryBlobStore::default())
            .with_skill_loader(SkillLoader::default())
            .with_mcp_config(McpConfig::default())
            .with_stream_elicitation_handler(StreamElicitationHandler::default())
            .with_plugin_registry(PluginRegistry::builder().build().expect("plugin registry"))
            .with_observability(Arc::new(NoopTracer))
            .with_aux_model(aux)
            .with_rule_provider(Arc::new(StaticRuleProvider))
            .with_tool_registry(tool_registry)
            .with_hook_registry(hook_registry);

        #[cfg(feature = "agents-subagent")]
        let builder = builder.with_subagent_runner(Arc::new(DenySubagentRunner));

        let harness = builder.build().await.expect("full builder should build");

        assert_eq!(harness.options().model_id, "test-model");
        assert_eq!(harness.options().tenant_policy.display_name, "second");
        assert_eq!(
            harness
                .memory_provider()
                .expect("memory provider should be set")
                .provider_id(),
            "second-memory"
        );
        let memory_items = harness
            .list_memory_items(SessionOptions::new(&workspace))
            .await
            .expect("all configured memory providers should be registered");
        assert!(memory_items.is_empty());
        assert!(harness.blob_store().is_some());
        assert!(harness.skill_loader().is_some());
        assert!(harness.mcp_config().is_some());
        assert!(harness.elicitation_handler().is_some());
        assert!(harness.plugin_registry().is_some());
        assert!(harness.tracer().is_some());
        assert!(harness.aux_model().is_some());
        assert_eq!(harness.rule_providers().len(), 1);

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should use overridden required dependencies");
        session
            .run_turn("setter override check")
            .await
            .expect("overridden model should be wired into runtime");
        assert_eq!(first_model.requests().await.len(), 0);
        assert_eq!(second_model.requests().await.len(), 1);
    });
}

#[test]
fn harness_builder_injects_model_middlewares_into_session_engine() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-model-middleware");
        std::fs::create_dir_all(&workspace).unwrap();

        let model = Arc::new(TestModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let middleware: Arc<dyn InferMiddleware> = Arc::new(RecordingModelMiddleware {
            label: "sdk",
            calls: Arc::clone(&calls),
        });
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model_provider)
            .with_model_middleware(middleware)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("builder should preserve model middleware");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should use middleware-enabled engine");
        session
            .run_turn("middleware through sdk")
            .await
            .expect("turn should run through injected middleware");

        let requests = model.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].extra["middleware"], json!("sdk"));
        assert_eq!(
            calls.lock().clone(),
            vec!["before:sdk".to_owned(), "end:sdk".to_owned()]
        );
    });
}

#[test]
fn harness_builder_installs_integrity_checked_decision_persistence() {
    block_on(async {
        let persistence = Arc::new(RecordingDecisionPersistence::trusted());
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_rule_provider(Arc::new(StaticRuleProvider))
            .with_decision_persistence(persistence.clone())
            .build()
            .await
            .expect("trusted persistence should build");

        let decision = persisted_decision(RuleSource::Session);
        harness
            .permission_broker()
            .expect("rule engine broker should be installed")
            .persist(decision.clone())
            .await
            .expect("persistence should receive learned decision");

        assert_eq!(persistence.persisted(), vec![decision]);
    });
}

#[test]
fn harness_builder_applies_policy_deny_gate_to_explicit_permission_broker() {
    block_on(async {
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker_arc(Arc::new(StaticPermissionBroker(Decision::AllowOnce)))
            .with_rule_provider(Arc::new(PolicyDenyRuleProvider))
            .build()
            .await
            .expect("harness should build with explicit broker and policy provider");

        let request = permission_request();
        let mut ctx = permission_context_for(&request);
        ctx.permission_mode = PermissionMode::BypassPermissions;
        let broker = harness
            .permission_broker()
            .expect("permission broker should be configured");

        assert!(broker.hard_policy_denies(&request, &ctx).await);

        assert_eq!(broker.decide(request, ctx).await, Decision::DenyOnce);
    });
}

#[test]
fn permission_authority_preserves_explicit_inner_hard_policy_probe() {
    block_on(async {
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker_arc(Arc::new(HardPolicyPermissionBroker))
            .with_rule_provider(Arc::new(StaticRuleProvider))
            .build()
            .await
            .expect("harness should build with explicit hard-policy broker and rule provider");

        let broker = harness
            .permission_broker()
            .expect("permission broker should be configured");
        let request = permission_request();
        let ctx = permission_context_for(&request);

        assert!(broker.hard_policy_denies(&request, &ctx).await);
        assert_eq!(broker.decide(request, ctx).await, Decision::DenyOnce);
    });
}

#[test]
fn harness_builder_rejects_untrusted_decision_persistence() {
    block_on(async {
        let err = match Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_rule_provider(Arc::new(StaticRuleProvider))
            .with_decision_persistence(Arc::new(RecordingDecisionPersistence::untrusted()))
            .build()
            .await
        {
            Ok(_) => panic!("untrusted persistence should fail closed"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("integrity"));
    });
}

#[test]
fn harness_resolves_stream_permission_requests() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-facade-permission");
        std::fs::create_dir_all(&workspace).unwrap();
        let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
            default_timeout: Some(Duration::from_secs(5)),
            heartbeat_interval: None,
            max_pending: 8,
        });
        let resolver_handle = resolver.clone();

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker(broker, resolver)
            .build()
            .await
            .expect("stream permission harness should build");

        let request = permission_request();
        let request_id = request.request_id;
        let tenant_id = request.tenant_id;
        let session_id = request.session_id;
        let ctx = permission_context_for(&request);
        let broker = harness
            .permission_broker()
            .expect("permission broker should be configured");
        let decision_task = tokio::spawn(async move { broker.decide(request, ctx).await });

        let outbound = receiver
            .recv()
            .await
            .expect("permission request should be emitted");
        assert_eq!(outbound.request_id, request_id);

        let option_id =
            pending_option_id_for_decision(&resolver_handle, request_id, Decision::AllowOnce);
        harness
            .resolve_permission_option(
                request_id,
                tenant_id,
                session_id,
                option_id,
                Decision::AllowOnce,
                None,
            )
            .await
            .expect("permission request should resolve through facade");
        assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
    });
}

#[test]
fn harness_rejects_stream_permission_scope_mismatch_without_consuming_pending() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-facade-permission-scope");
        std::fs::create_dir_all(&workspace).unwrap();
        let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
            default_timeout: Some(Duration::from_secs(5)),
            heartbeat_interval: None,
            max_pending: 8,
        });
        let resolver_handle = resolver.clone();

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker(broker, resolver)
            .build()
            .await
            .expect("stream permission harness should build");

        let request = permission_request();
        let request_id = request.request_id;
        let tenant_id = request.tenant_id;
        let session_id = request.session_id;
        let ctx = permission_context_for(&request);
        let broker = harness
            .permission_broker()
            .expect("permission broker should be configured");
        let decision_task = tokio::spawn(async move { broker.decide(request, ctx).await });

        receiver
            .recv()
            .await
            .expect("permission request should emit");
        let option_id =
            pending_option_id_for_decision(&resolver_handle, request_id, Decision::AllowOnce);

        let error = harness
            .resolve_permission_option(
                request_id,
                TenantId::new(),
                session_id,
                option_id,
                Decision::AllowOnce,
                None,
            )
            .await
            .expect_err("wrong tenant should be rejected");
        assert!(matches!(error, HarnessError::Permission(_)));
        assert_eq!(resolver_handle.pending_permission_requests().len(), 1);

        let error = harness
            .resolve_permission_option(
                request_id,
                tenant_id,
                SessionId::new(),
                option_id,
                Decision::AllowOnce,
                None,
            )
            .await
            .expect_err("wrong session should be rejected");
        assert!(matches!(error, HarnessError::Permission(_)));
        assert_eq!(resolver_handle.pending_permission_requests().len(), 1);

        harness
            .resolve_permission_option(
                request_id,
                tenant_id,
                session_id,
                option_id,
                Decision::AllowOnce,
                None,
            )
            .await
            .expect("matching scope should resolve");
        assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
    });
}

#[test]
fn stream_permission_runtime_does_not_backpressure_resolved_requests() {
    tokio_runtime().block_on(async {
        let runtime = jyowo_harness_sdk::StreamPermissionRuntime::new(StreamBrokerConfig {
            default_timeout: Some(Duration::from_secs(5)),
            heartbeat_interval: None,
            max_pending: 1,
        });
        let broker = runtime.broker();

        let first = permission_request_named("first");
        let first_request_id = first.request_id;
        let first_tenant_id = first.tenant_id;
        let first_session_id = first.session_id;
        let first_ctx = permission_context_for(&first);
        let first_task = tokio::spawn(async move { broker.decide(first, first_ctx).await });
        wait_for_pending_permission(&runtime, first_request_id).await;
        let option_id = pending_option_id_for_decision(
            &runtime.resolver_handle(),
            first_request_id,
            Decision::AllowOnce,
        );
        runtime
            .resolve_permission_option(
                first_request_id,
                first_tenant_id,
                first_session_id,
                option_id,
                Decision::AllowOnce,
                None,
            )
            .await
            .expect("first permission should resolve");
        assert_eq!(first_task.await.unwrap(), Decision::AllowOnce);

        let broker = runtime.broker();
        let second = permission_request_named("second");
        let second_request_id = second.request_id;
        let second_tenant_id = second.tenant_id;
        let second_session_id = second.session_id;
        let second_ctx = permission_context_for(&second);
        let second_task = tokio::spawn(async move { broker.decide(second, second_ctx).await });
        wait_for_pending_permission(&runtime, second_request_id).await;
        let option_id = pending_option_id_for_decision(
            &runtime.resolver_handle(),
            second_request_id,
            Decision::DenyOnce,
        );
        runtime
            .resolve_permission_option(
                second_request_id,
                second_tenant_id,
                second_session_id,
                option_id,
                Decision::DenyOnce,
                None,
            )
            .await
            .expect("second permission should resolve");

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), second_task)
                .await
                .expect("stale emitted requests must not block new decisions")
                .unwrap(),
            Decision::DenyOnce
        );
    });
}

#[test]
fn stream_permission_runtime_does_not_reuse_permanent_allow_across_sessions() {
    tokio_runtime().block_on(async {
        let runtime = jyowo_harness_sdk::StreamPermissionRuntime::new(StreamBrokerConfig {
            default_timeout: Some(Duration::from_secs(5)),
            heartbeat_interval: None,
            max_pending: 16,
        });
        let broker = runtime.broker();

        let mut first = permission_request_named("shared-no-workspace-permission");
        first.decision_options = reusable_permission_options(&first);
        let first_request_id = first.request_id;
        let first_tenant_id = first.tenant_id;
        let first_session_id = first.session_id;
        let first_ctx = permission_context_for(&first);
        let first_task = tokio::spawn(async move { broker.decide(first, first_ctx).await });
        wait_for_pending_permission(&runtime, first_request_id).await;
        let allow_always = pending_option_id_for_decision(
            &runtime.resolver_handle(),
            first_request_id,
            Decision::AllowPermanent,
        );
        runtime
            .resolve_permission_option(
                first_request_id,
                first_tenant_id,
                first_session_id,
                allow_always,
                Decision::AllowPermanent,
                None,
            )
            .await
            .expect("first permission should resolve");
        assert_eq!(first_task.await.unwrap(), Decision::AllowPermanent);

        let broker = runtime.broker();
        let mut second = permission_request_named("shared-no-workspace-permission");
        second.decision_options = reusable_permission_options(&second);
        let second_request_id = second.request_id;
        let second_tenant_id = second.tenant_id;
        let second_session_id = second.session_id;
        let second_ctx = permission_context_for(&second);
        let second_task = tokio::spawn(async move { broker.decide(second, second_ctx).await });
        wait_for_pending_permission(&runtime, second_request_id).await;
        let deny = pending_option_id_for_decision(
            &runtime.resolver_handle(),
            second_request_id,
            Decision::DenyOnce,
        );
        runtime
            .resolve_permission_option(
                second_request_id,
                second_tenant_id,
                second_session_id,
                deny,
                Decision::DenyOnce,
                None,
            )
            .await
            .expect("second permission should resolve independently");
        assert_eq!(second_task.await.unwrap(), Decision::DenyOnce);
    });
}

#[test]
fn harness_resolves_stream_elicitation_requests() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-facade-elicitation");
        std::fs::create_dir_all(&workspace).unwrap();
        let handler = StreamElicitationHandler::default();
        let request_id = RequestId::new();

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_elicitation_handler(handler.clone())
            .build()
            .await
            .expect("stream elicitation harness should build");

        let handle_task = tokio::spawn(async move {
            handler
                .handle(harness_mcp::ElicitationRequest {
                    request_id,
                    server_id: McpServerId("test-server".to_owned()),
                    schema: json!({"type": "object"}),
                    subject: "credentials".to_owned(),
                    detail: None,
                    timeout: Some(Duration::from_secs(5)),
                    mode: harness_mcp::ElicitationMode::Form,
                })
                .await
        });

        tokio::task::yield_now().await;
        harness
            .resolve_elicitation(request_id, json!({"token": "redacted"}))
            .await
            .expect("elicitation request should resolve through facade");

        assert_eq!(
            handle_task.await.unwrap().unwrap(),
            json!({"token": "redacted"})
        );
    });
}

#[test]
fn testing_module_exports_expected_testing_adapters() {
    let store: TestEventStore = test_event_store(Arc::new(NoopRedactor));
    let memory = InMemoryMemoryProvider::new("test-memory");
    let tool = TestTool::new("test-tool");
    let hook = TestHookHandler::new("test-hook", vec![HookEventKind::UserPromptSubmit]);

    assert_eq!(memory.provider_id(), "test-memory");
    assert_eq!(tool.descriptor().name, "test-tool");
    assert_eq!(hook.handler_id(), "test-hook");
    drop(store);
}

#[test]
fn facade_exports_production_runtime_adapter_types() {
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::ForkReason>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::JournalError>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::ToolUseCompletedEvent>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::SessionOptions>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::McpServerScope>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::McpError>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::ContextPatchRequest>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::ContextPatchSource>();
    let _ = std::any::TypeId::of::<jyowo_harness_sdk::ext::ContextPatchLifecycle>();
}

#[test]
fn tool_registry_builtin_skills_registers_only_safe_skill_tools() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Skills)
        .build()
        .expect("skill tool registry should build");

    let mut names = registry
        .snapshot()
        .as_descriptors()
        .into_iter()
        .map(|descriptor| descriptor.name.clone())
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(
        names,
        vec![
            "skills_invoke",
            "skills_list",
            "skills_run_script",
            "skills_view"
        ]
    );
}

#[test]
fn session_push_context_patch_injects_knowledge_retrieval_before_turn() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-context-patch");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let session_id = SessionId::new();
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");

        session
            .push_context_patch(ContextPatchRequest {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id: RunId::new(),
                source: ContextPatchSource::KnowledgeRetrieval {
                    provider_id: "jyowo_knowledge".to_owned(),
                    knowledge_base_ids: vec!["kb-agent".to_owned()],
                    reference_chunk_count: 1,
                },
                body: "knowledge citation body".to_owned(),
                lifecycle: ContextPatchLifecycle::Transient,
            })
            .await
            .expect("knowledge patch should be accepted through session facade");
        session
            .run_turn("use retrieved context")
            .await
            .expect("turn should run with context patch");

        let requests = model.requests().await;
        let serialized = serde_json::to_string(&requests[0].messages).expect("messages serialize");
        assert!(serialized.contains("knowledge citation body"));
        assert!(serialized.contains("use retrieved context"));
        let events = harness
            .event_store()
            .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should replay")
            .collect::<Vec<_>>()
            .await;
        assert!(events
            .iter()
            .any(|event| matches!(event.payload, Event::ContextPatchApplied(_))));
    });
}

#[cfg(all(
    feature = "builtin-toolset",
    feature = "blob-file",
    feature = "blob-sqlite"
))]
#[test]
fn builtin_module_exports_blob_stores_and_builtin_tools() {
    let _ = std::any::TypeId::of::<FileBlobStore>();
    let _ = std::any::TypeId::of::<SqliteBlobStore>();
    let _ = std::any::TypeId::of::<BashTool>();
    let _ = std::any::TypeId::of::<FileReadTool>();
    let _ = std::any::TypeId::of::<FileWriteTool>();
    let _ = std::any::TypeId::of::<GrepTool>();
    let _ = std::any::TypeId::of::<ListDirTool>();
    let _ = std::any::TypeId::of::<ReadBlobTool>();
    let _ = std::any::TypeId::of::<SendMessageTool>();
    let _ = std::any::TypeId::of::<SkillsListTool>();
    let _ = std::any::TypeId::of::<SkillsViewTool>();
    let _ = std::any::TypeId::of::<SkillsInvokeTool>();
    let _ = std::any::TypeId::of::<WebSearchTool>();
}

#[cfg(feature = "observability-redactor")]
#[test]
fn builtin_module_exports_default_redactor() {
    let _ = std::any::TypeId::of::<DefaultRedactor>();
}

#[test]
fn enabled_features_reports_compiled_sdk_features() {
    let features = Harness::enabled_feature_set();

    assert!(features.contains("testing"));
    assert!(features.contains("in-memory-store"));
    assert!(features.contains("noop-sandbox"));
    assert!(features.contains("provider-anthropic"));
}

#[test]
fn harness_can_create_workspace_directory() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-workspace");

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("complete type-state builder should build");

        let created = harness
            .create_workspace(&workspace)
            .await
            .expect("workspace should be created");

        assert_eq!(created, workspace.canonicalize().unwrap());
    });
}

#[test]
fn create_workspace_initializes_governed_layout() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-workspace-layout");
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let created = harness
            .create_workspace(&workspace)
            .await
            .expect("workspace should be created");

        assert_eq!(created, workspace.canonicalize().unwrap());
        for relative in [
            "config",
            "data",
            "runtime/events",
            "runtime/sessions",
            "logs",
            "tmp",
        ] {
            assert!(
                created.join(relative).is_dir(),
                "missing governed workspace path: {relative}"
            );
        }
    });
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        harness_contracts::SessionId::new()
    ))
}

fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

fn permission_request() -> PermissionRequest {
    permission_request_named("test")
}

fn permission_request_named(kind: &str) -> PermissionRequest {
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "test-tool".to_owned(),
        subject: PermissionSubject::Custom {
            kind: kind.to_owned(),
            payload: json!({}),
        },
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("test-tool".to_owned()),
        action_plan_hash: harness_contracts::ActionPlanHash::default(),
        decision_options: Vec::new(),
        confirmation_expected: None,
        created_at: harness_contracts::now(),
    }
}

fn reusable_permission_options(request: &PermissionRequest) -> Vec<PermissionDecisionOption> {
    vec![
        PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::AllowPermanent,
            scope: request.scope_hint.clone(),
            lifetime: DecisionLifetime::Persisted,
            matcher_summary: DecisionMatcherSummary {
                kind: DecisionMatcherKind::ToolName,
                label: "test-tool".to_owned(),
            },
            label: "Always allow".to_owned(),
            requires_confirmation: false,
            action_plan_hash: request.action_plan_hash.clone(),
            fingerprint: None,
        },
        PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::DenyOnce,
            scope: request.scope_hint.clone(),
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: DecisionMatcherKind::ToolName,
                label: "test-tool".to_owned(),
            },
            label: "Deny once".to_owned(),
            requires_confirmation: false,
            action_plan_hash: request.action_plan_hash.clone(),
            fingerprint: None,
        },
    ]
}

fn permission_context_for(request: &PermissionRequest) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: request.session_id,
        tenant_id: request.tenant_id,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}

fn pending_option_id_for_decision(
    resolver: &ResolverHandle,
    request_id: RequestId,
    decision: Decision,
) -> harness_contracts::PermissionOptionId {
    resolver
        .pending_permission_requests()
        .into_iter()
        .find(|pending| pending.request.request_id == request_id)
        .expect("pending request should exist")
        .decision_options
        .into_iter()
        .find(|option| option.decision == decision)
        .expect("pending option should exist")
        .option_id
}

async fn wait_for_pending_permission(
    runtime: &jyowo_harness_sdk::StreamPermissionRuntime,
    request_id: RequestId,
) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if runtime
                .pending_requests()
                .iter()
                .any(|request| request.request_id == request_id)
            {
                return;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("permission request should become pending");
}

#[derive(Default)]
struct TestAuxModelProvider {
    inner: Arc<TestModelProvider>,
}

#[async_trait]
impl AuxModelProvider for TestAuxModelProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        self.inner.clone()
    }

    fn aux_options(&self) -> AuxOptions {
        AuxOptions::default()
    }

    async fn call_aux(
        &self,
        _task: AuxTask,
        _req: ModelRequest,
    ) -> Result<String, harness_contracts::ModelError> {
        Ok("aux".to_owned())
    }
}

struct StaticRuleProvider;

#[async_trait]
impl RuleProvider for StaticRuleProvider {
    fn provider_id(&self) -> &str {
        "static"
    }

    fn source(&self) -> RuleSource {
        RuleSource::Session
    }

    async fn resolve_rules(
        &self,
        _tenant: TenantId,
    ) -> Result<Vec<PermissionRule>, harness_contracts::PermissionError> {
        Ok(Vec::new())
    }

    fn watch(
        &self,
    ) -> Option<futures::stream::BoxStream<'static, harness_permission::RulesUpdated>> {
        None
    }
}

struct PolicyDenyRuleProvider;

#[async_trait]
impl RuleProvider for PolicyDenyRuleProvider {
    fn provider_id(&self) -> &str {
        "policy-deny"
    }

    fn source(&self) -> RuleSource {
        RuleSource::Policy
    }

    async fn resolve_rules(
        &self,
        _tenant: TenantId,
    ) -> Result<Vec<PermissionRule>, harness_contracts::PermissionError> {
        Ok(vec![PermissionRule {
            id: "deny-test-tool".to_owned(),
            priority: 0,
            scope: DecisionScope::ToolName("test-tool".to_owned()),
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }])
    }

    fn watch(
        &self,
    ) -> Option<futures::stream::BoxStream<'static, harness_permission::RulesUpdated>> {
        None
    }
}

struct StaticPermissionBroker(Decision);

#[async_trait]
impl PermissionBroker for StaticPermissionBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.0.clone()
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct HardPolicyPermissionBroker;

#[async_trait]
impl PermissionBroker for HardPolicyPermissionBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        true
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct RecordingModelMiddleware {
    label: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl InferMiddleware for RecordingModelMiddleware {
    fn middleware_id(&self) -> &str {
        self.label
    }

    async fn before_request(
        &self,
        req: &mut ModelRequest,
        _ctx: &mut InferContext,
    ) -> Result<(), ModelError> {
        req.extra = json!({ "middleware": self.label });
        self.calls.lock().push(format!("before:{}", self.label));
        Ok(())
    }

    async fn on_request_end(
        &self,
        _usage: &harness_contracts::UsageSnapshot,
        _ctx: &InferContext,
    ) -> Result<(), ModelError> {
        self.calls.lock().push(format!("end:{}", self.label));
        Ok(())
    }
}

#[derive(Debug)]
struct RecordingDecisionPersistence {
    supports_integrity: bool,
    persisted: Mutex<Vec<PersistedDecision>>,
}

impl RecordingDecisionPersistence {
    fn trusted() -> Self {
        Self {
            supports_integrity: true,
            persisted: Mutex::new(Vec::new()),
        }
    }

    fn untrusted() -> Self {
        Self {
            supports_integrity: false,
            persisted: Mutex::new(Vec::new()),
        }
    }

    fn persisted(&self) -> Vec<PersistedDecision> {
        self.persisted.lock().clone()
    }
}

#[async_trait]
impl DecisionPersistence for RecordingDecisionPersistence {
    fn supports_integrity(&self) -> bool {
        self.supports_integrity
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persisted.lock().push(decision);
        Ok(())
    }
}

#[async_trait]
impl DecisionHistory for RecordingDecisionPersistence {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(None)
    }
}

fn persisted_decision(source: RuleSource) -> PersistedDecision {
    PersistedDecision {
        decision_id: DecisionId::new(),
        decision: Decision::AllowPermanent,
        scope: DecisionScope::ToolName("shell".to_owned()),
        source,
        session_id: None,
        fingerprint: None,
    }
}

#[cfg(feature = "agents-subagent")]
struct DenySubagentRunner;

#[cfg(feature = "agents-subagent")]
#[async_trait]
impl harness_subagent::SubagentRunner for DenySubagentRunner {
    async fn spawn(
        &self,
        _spec: harness_subagent::SubagentSpec,
        _input: harness_contracts::TurnInput,
        _parent_ctx: harness_subagent::ParentContext,
    ) -> Result<harness_subagent::SubagentHandle, harness_subagent::SubagentError> {
        Err(harness_subagent::SubagentError::Cancelled)
    }
}
