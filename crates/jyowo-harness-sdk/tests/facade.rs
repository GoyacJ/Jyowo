#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{executor::block_on, StreamExt};
use harness_contracts::{
    Decision, DecisionId, DecisionScope, Event, FallbackPolicy, HookEventKind, InteractivityLevel,
    McpServerId, ModelError, NoopRedactor, PermissionError, PermissionMode, PermissionSubject,
    RequestId, RuleSource, Severity, ToolUseId,
};
use harness_mcp::ElicitationHandler;
use harness_model::{
    AuxModelProvider, AuxOptions, AuxTask, InferContext, InferMiddleware, ModelRequest,
};
use harness_permission::{
    DecisionPersistence, PermissionContext, PermissionRequest, PermissionRule, PersistedDecision,
    RuleProvider, RuleSnapshot, StreamBasedBroker, StreamBrokerConfig,
};
use jyowo_harness_sdk::{builtin::*, prelude::*, testing::*};
use parking_lot::Mutex;
use serde_json::json;

#[test]
fn harness_builder_creates_testing_harness_and_session() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-session");
        std::fs::create_dir_all(&workspace).unwrap();

        let model = Arc::new(MockProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let harness = Harness::builder()
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
        let first_model = Arc::new(MockProvider::default());
        let first_model_provider: Arc<dyn ModelProvider> = first_model.clone();
        let second_model = Arc::new(MockProvider::default());
        let second_model_provider: Arc<dyn ModelProvider> = second_model.clone();
        let first_store: Arc<dyn EventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let second_store: Arc<dyn EventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let first_sandbox: Arc<dyn SandboxBackend> = Arc::new(NoopSandbox::new());
        let second_sandbox: Arc<dyn SandboxBackend> = Arc::new(NoopSandbox::new());
        let first_memory = Arc::new(MockMemoryProvider::new("first-memory"));
        let second_memory = Arc::new(MockMemoryProvider::new("second-memory"));
        let aux = TestAuxModelProvider::default();
        let tool_registry = ToolRegistry::builder()
            .with_tool(Box::new(MockTool::new("mock_tool")))
            .build()
            .expect("tool registry should build");
        let hook_registry = HookRegistry::builder()
            .with_hook(Box::new(MockHookHandler::new(
                "mock-hook",
                vec![HookEventKind::UserPromptSubmit],
            )))
            .build()
            .expect("hook registry should build");

        let builder = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_id("first-model")
            .with_model_arc(first_model_provider)
            .with_model_arc(second_model_provider)
            .with_model_id("second-model")
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

        assert_eq!(harness.options().model_id, "second-model");
        assert_eq!(harness.options().tenant_policy.display_name, "second");
        assert_eq!(
            harness
                .memory_provider()
                .expect("memory provider should be set")
                .provider_id(),
            "second-memory"
        );
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

        let model = Arc::new(MockProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let middleware: Arc<dyn InferMiddleware> = Arc::new(RecordingModelMiddleware {
            label: "sdk",
            calls: Arc::clone(&calls),
        });
        let harness = Harness::builder()
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
            .with_model(MockProvider::default())
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
fn harness_builder_rejects_untrusted_decision_persistence() {
    block_on(async {
        let err = match Harness::builder()
            .with_model(MockProvider::default())
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

        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker(broker, resolver)
            .build()
            .await
            .expect("stream permission harness should build");

        let request = permission_request();
        let request_id = request.request_id;
        let broker = harness
            .permission_broker()
            .expect("permission broker should be configured");
        let decision_task =
            tokio::spawn(async move { broker.decide(request, permission_context()).await });

        let outbound = receiver
            .recv()
            .await
            .expect("permission request should be emitted");
        assert_eq!(outbound.request_id, request_id);

        harness
            .resolve_permission(request_id, Decision::AllowOnce)
            .await
            .expect("permission request should resolve through facade");
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

        let first = permission_request();
        let first_request_id = first.request_id;
        let first_task =
            tokio::spawn(async move { broker.decide(first, permission_context()).await });
        wait_for_pending_permission(&runtime, first_request_id).await;
        runtime
            .resolve_permission(first_request_id, Decision::AllowOnce)
            .await
            .expect("first permission should resolve");
        assert_eq!(first_task.await.unwrap(), Decision::AllowOnce);

        let broker = runtime.broker();
        let second = permission_request();
        let second_request_id = second.request_id;
        let second_task =
            tokio::spawn(async move { broker.decide(second, permission_context()).await });
        wait_for_pending_permission(&runtime, second_request_id).await;
        runtime
            .resolve_permission(second_request_id, Decision::DenyOnce)
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
fn harness_resolves_stream_elicitation_requests() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-facade-elicitation");
        std::fs::create_dir_all(&workspace).unwrap();
        let handler = StreamElicitationHandler::default();
        let request_id = RequestId::new();

        let harness = Harness::builder()
            .with_model(MockProvider::default())
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
fn testing_module_exports_expected_mocks() {
    let store: MockEventStore = mock_event_store(Arc::new(NoopRedactor));
    let memory = MockMemoryProvider::new("mock-memory");
    let tool = MockTool::new("mock-tool");
    let hook = MockHookHandler::new("mock-hook", vec![HookEventKind::UserPromptSubmit]);

    assert_eq!(memory.provider_id(), "mock-memory");
    assert_eq!(tool.descriptor().name, "mock-tool");
    assert_eq!(hook.handler_id(), "mock-hook");
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

    assert_eq!(names, vec!["skills_invoke", "skills_list", "skills_view"]);
}

#[test]
fn session_push_context_patch_injects_knowledge_retrieval_before_turn() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-context-patch");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(MockProvider::default());
        let session_id = SessionId::new();
        let harness = Harness::builder()
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
            .with_model(MockProvider::default())
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
            .with_model(MockProvider::default())
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
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "mock-tool".to_owned(),
        subject: PermissionSubject::Custom {
            kind: "test".to_owned(),
            payload: json!({}),
        },
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("mock-tool".to_owned()),
        created_at: harness_contracts::now(),
    }
}

fn permission_context() -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        rule_snapshot: Arc::new(RuleSnapshot {
            rules: Vec::new(),
            generation: 0,
            built_at: harness_contracts::now(),
        }),
        hook_overrides: Vec::new(),
    }
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
    inner: Arc<MockProvider>,
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

fn persisted_decision(source: RuleSource) -> PersistedDecision {
    PersistedDecision {
        decision_id: DecisionId::new(),
        scope: DecisionScope::ToolName("shell".to_owned()),
        source,
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
