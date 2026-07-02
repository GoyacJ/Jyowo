use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, Decision, DecisionScope, DeferPolicy, EndReason, Event,
    HookEventKind, HookFailureMode, InteractivityLevel, Message, MessageId, MessagePart,
    MessageRole, ModelError, NoopRedactor, OverflowAction, PermissionError, PermissionMode,
    PermissionSubject, ProviderRestriction, RedactRules, Redactor, ResultBudget, RunId, SessionId,
    StopReason, TenantId, ToolDescriptor, ToolGroup, ToolOrigin, ToolProperties, ToolResult,
    ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookMessageView, HookOutcome,
    HookRegistry, HookSessionView, PreToolUseOutcome, ReplayMode, ToolDescriptorView,
    ToolErrorView,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_observability::Observer;
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_sandbox::{ExecSpec, SandboxBaseConfig, StdioSpec};
use harness_tool::{
    builtin::BashTool, SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool,
    ToolPoolFilter, ToolPoolModelProfile, ToolRegistry, ToolStream, ValidationError,
};
use harness_tool_search::{
    ToolSearchPreHookOutcome, ToolSearchRuntimeCap, ToolSearchRuntimeSnapshot, ToolSearchTool,
    TOOL_SEARCH_RUNTIME_CAPABILITY,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

#[tokio::test]
async fn pre_tool_use_rewrites_before_permission() {
    let broker = Arc::new(RecordingBroker::new(Decision::DenyOnce));
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "original" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(RewritePreToolHook)],
        broker.clone(),
    )
    .await;

    let events = harness.run("rewrite").await.unwrap();

    assert!(broker.requests().await.is_empty());
    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionRequested(requested)
            if matches!(
                &requested.subject,
                PermissionSubject::ToolInvocation { input, .. }
                    if input == &json!({ "value": "rewritten" })
            ) && requested.fingerprint.is_some()
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::AllowOnce && resolved.fingerprint.is_some()
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolUseRequested(requested)
            if requested.input == json!({ "value": "rewritten" })
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolUseCompleted(completed)
            if completed.result == ToolResult::Text("rewritten".to_owned())
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::HookRewroteInput(_))));
}

#[tokio::test]
async fn user_prompt_hook_receives_redacted_prompt_payload() {
    let captured = Arc::new(Mutex::new(None));
    let harness = TestHarness::new_with_redactor(
        vec![text_events("done")],
        Box::new(EchoTool::new()),
        vec![Box::new(CaptureUserPromptInputHook {
            captured: Arc::clone(&captured),
        })],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
        Arc::new(SecretRedactor),
    )
    .await;

    harness.run("prompt secret-token").await.unwrap();

    let captured = captured.lock().await.take().expect("hook input captured");
    assert_eq!(captured["prompt"], json!("prompt [redacted]"));
}

#[tokio::test]
async fn pre_tool_use_hook_receives_redacted_tool_input_payload() {
    let captured = Arc::new(Mutex::new(None));
    let harness = TestHarness::new_with_redactor(
        vec![
            tool_call_events("Echo", json!({ "value": "secret-token" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(CapturePreToolInputHook {
            captured: Arc::clone(&captured),
        })],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
        Arc::new(SecretRedactor),
    )
    .await;

    harness.run("use tool").await.unwrap();

    let captured = captured.lock().await.take().expect("hook input captured");
    assert_eq!(captured["value"], json!("[redacted]"));
}

#[tokio::test]
async fn pre_tool_use_rewrite_recomputes_command_fingerprint() {
    let broker = Arc::new(RecordingBroker::new(Decision::DenyOnce));
    let harness = TestHarness::new(
        vec![
            tool_call_events("Bash", json!({ "command": "echo original" })),
            text_events("done"),
        ],
        Box::new(BashTool::default()),
        vec![Box::new(RewriteCommandPreToolHook)],
        broker.clone(),
    )
    .await;

    let events = harness.run("rewrite command").await.unwrap();
    let requests = broker.requests().await;
    assert_eq!(requests.len(), 1);
    let PermissionSubject::CommandExec {
        command,
        cwd,
        fingerprint: Some(fingerprint),
        ..
    } = &requests[0].subject
    else {
        panic!("expected command permission request");
    };
    let expected = ExecSpec {
        command: "echo rewritten".to_owned(),
        cwd: None,
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: harness_contracts::WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
    .canonical_fingerprint(&SandboxBaseConfig::default());
    let original = ExecSpec {
        command: "echo original".to_owned(),
        cwd: None,
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: harness_contracts::WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
    .canonical_fingerprint(&SandboxBaseConfig::default());

    assert_eq!(command, "echo rewritten");
    assert_eq!(cwd, &None);
    assert_eq!(*fingerprint, expected);
    assert_ne!(*fingerprint, original);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionRequested(requested)
            if requested.fingerprint == Some(expected)
                && matches!(
                    &requested.subject,
                    PermissionSubject::CommandExec { command, .. }
                        if command == "echo rewritten"
                )
    )));
}

#[tokio::test]
async fn transform_and_post_tool_hooks_are_applied() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "original" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(ToolLifecycleHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("transform").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolUseCompleted(completed)
            if completed.result == ToolResult::Text("result transformed".to_owned())
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::TransformTerminalOutput
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::TransformToolResult
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::PostToolUse
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::HookReturnedAdditionalContext(_))));
}

#[tokio::test]
async fn post_tool_hooks_receive_redacted_tool_output_payloads() {
    let captured = Arc::new(Mutex::new(Default::default()));
    let harness = TestHarness::new_with_redactor(
        vec![
            tool_call_events("Echo", json!({ "value": "secret-token" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(CapturePostToolPayloadHook {
            captured: Arc::clone(&captured),
        })],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
        Arc::new(SecretRedactor),
    )
    .await;

    harness.run("post tool redaction").await.unwrap();

    let captured = captured.lock().await.clone();
    assert_eq!(
        captured.terminal_raw.as_deref(),
        Some(b"[redacted]".as_ref())
    );
    assert_eq!(
        captured.transform_result,
        Some(ToolResult::Text("[redacted]".to_owned()))
    );
    assert_eq!(
        captured.post_result,
        Some(ToolResult::Text("[redacted]".to_owned()))
    );
}

#[tokio::test]
async fn post_tool_failure_hook_receives_redacted_error_payload() {
    let captured = Arc::new(Mutex::new(None));
    let harness = TestHarness::new_with_redactor(
        vec![
            tool_call_events("Fail", json!({ "value": "secret-token" })),
            text_events("done"),
        ],
        Box::new(FailingTool::new()),
        vec![Box::new(CapturePostToolFailureHook {
            captured: Arc::clone(&captured),
        })],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
        Arc::new(SecretRedactor),
    )
    .await;

    harness.run("post tool failure redaction").await.unwrap();

    let captured = captured.lock().await.take().expect("hook error captured");
    assert_eq!(captured.message, "internal: failed with [redacted]");
}

#[tokio::test]
async fn permission_request_hook_receives_redacted_detail_payload() {
    let captured = Arc::new(Mutex::new(None));
    let harness = TestHarness::new_with_redactor(
        vec![
            tool_call_events("Echo", json!({ "value": "secret-token" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(CapturePermissionRequestHook {
            captured: Arc::clone(&captured),
        })],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
        Arc::new(SecretRedactor),
    )
    .await;

    harness.run("permission redaction").await.unwrap();

    let captured = captured.lock().await.take().expect("hook detail captured");
    assert_eq!(captured.subject, "Echo");
    assert_eq!(
        captured.detail.as_deref(),
        Some(
            "ToolInvocation { tool: \"Echo\", input: Object {\"value\": String(\"[redacted]\")} }"
        )
    );
}

#[tokio::test]
async fn post_tool_add_context_is_injected_as_next_user_message() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "original" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(ToolLifecycleHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    harness.run("inject context").await.unwrap();

    let requests = harness.model.requests().await;
    assert_eq!(requests.len(), 2);
    let second = &requests[1];
    assert!(second.system.as_deref().unwrap_or_default().is_empty());
    assert!(second.messages.iter().any(|message| {
        message.role == MessageRole::User && message_text(message).contains("post context")
    }));
    assert!(!second.messages.iter().any(|message| {
        message.role == MessageRole::Tool && message_text(message).contains("post context")
    }));
}

#[tokio::test]
async fn llm_and_api_hooks_are_emitted() {
    let harness = TestHarness::new(
        vec![text_events("done")],
        Box::new(EchoTool::new()),
        vec![Box::new(LlmApiHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("hook phases").await.unwrap();

    for expected in [
        HookEventKind::PreLlmCall,
        HookEventKind::PreApiRequest,
        HookEventKind::PostLlmCall,
        HookEventKind::PostApiRequest,
    ] {
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::HookTriggered(triggered)
                    if triggered.hook_event_kind == expected
            )),
            "missing {expected:?}"
        );
    }
    assert!(events.iter().any(|event| {
        matches!(event, Event::RunEnded(ended) if ended.reason == EndReason::Completed)
    }));
}

#[tokio::test]
async fn user_prompt_hook_writes_journal_events() {
    let harness = TestHarness::new(
        vec![text_events("done")],
        Box::new(EchoTool::new()),
        vec![Box::new(UserPromptHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("prompt event").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::UserPromptSubmit
                && triggered.handler_id == "user-prompt"
    )));
}

#[tokio::test]
async fn missing_default_hook_events_are_triggered() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "needs permission" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(PermissionRequestHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("permission hook").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::PermissionRequest
                && triggered.handler_id == "permission-request"
    )));
}

#[tokio::test]
async fn hook_unsupported_and_inconsistent_emit_dedicated_events() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "original" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![
            Box::new(UnsupportedLlmHook),
            Box::new(InconsistentPreToolHook),
        ],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("hook failures").await.unwrap();

    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::HookFailed(failed)
                if failed.handler_id == "unsupported-llm"
                    && failed.cause_kind == harness_contracts::HookFailureCauseKind::Unsupported
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::HookReturnedUnsupported(unsupported)
                if unsupported.handler_id == "unsupported-llm"
                    && unsupported.hook_event_kind == HookEventKind::PreLlmCall
                    && unsupported.returned_kind == harness_contracts::HookOutcomeDiscriminant::Transform
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::HookFailed(failed)
                if failed.handler_id == "inconsistent-pre-tool"
                    && failed.cause_kind == harness_contracts::HookFailureCauseKind::Inconsistent
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::HookOutcomeInconsistent(inconsistent)
                if inconsistent.handler_id == "inconsistent-pre-tool"
                    && inconsistent.hook_event_kind == HookEventKind::PreToolUse
                    && inconsistent.reason == harness_contracts::InconsistentReason::PreToolUseBlockExclusive
        )
    }));
}

#[tokio::test]
async fn tool_search_hooks_are_emitted() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(SequenceModel::new(vec![
        tool_call_events("tool_search", json!({ "query": "select:Missing" })),
        text_events("done"),
    ]));
    let echo = Box::new(EchoTool::new());
    let deferred_descriptor = echo.descriptor().clone();
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![echo]))
        .build()
        .unwrap();
    let mut tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Always,
        &ToolPoolModelProfile {
            provider: harness_contracts::ModelProvider("test".to_owned()),
            max_context_tokens: Some(8_000),
        },
        &SchemaResolverContext {
            run_id,
            session_id,
            tenant_id,
        },
    )
    .await
    .unwrap();
    tools.append_runtime_tool(Arc::new(ToolSearchTool::builder().build()));

    let hook_registry = HookRegistry::builder()
        .with_hook(Box::new(ToolSearchLifecycleHook))
        .build()
        .unwrap();
    let hook_dispatcher = HookDispatcher::new(hook_registry.snapshot());
    let mut caps = CapabilityRegistry::default();
    let model_caps = ConversationModelCapability {
        tool_calling: true,
        ..Default::default()
    };
    let search_runtime: Arc<dyn ToolSearchRuntimeCap> = Arc::new(EngineToolSearchRuntime {
        snapshot: ToolSearchRuntimeSnapshot {
            deferred_tools: vec![deferred_descriptor],
            loaded_tool_names: BTreeSet::from(["tool_search".to_owned()]),
            discovered_tool_names: BTreeSet::new(),
            pending_mcp_servers: Vec::new(),
            model_caps: Arc::new(model_caps),
            reload_handle: None,
        },
        event_store: store.clone(),
        hooks: hook_dispatcher.clone(),
        tenant_id,
        session_id,
    });
    caps.install(
        harness_contracts::ToolCapability::Custom(TOOL_SEARCH_RUNTIME_CAPABILITY.to_owned()),
        search_runtime,
    );

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("tool-search-hook-test"))
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(hook_dispatcher)
        .with_model(model)
        .with_tools(tools)
        .with_permission_broker(Arc::new(RecordingBroker::new(Decision::AllowOnce)))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(caps))
        .build()
        .unwrap();

    engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("search"),
            RunContext::new(tenant_id, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let events = store
        .read(tenant_id, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    for expected in [
        HookEventKind::PreToolSearch,
        HookEventKind::PostToolSearchMaterialize,
    ] {
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::HookTriggered(triggered)
                    if triggered.hook_event_kind == expected
            )),
            "missing {expected:?}"
        );
    }
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::HookRewroteInput(_))));
}

struct TestHarness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    engine: Engine,
    store: Arc<InMemoryEventStore>,
    model: Arc<SequenceModel>,
}

impl TestHarness {
    async fn new(
        responses: Vec<Vec<ModelStreamEvent>>,
        tool: Box<dyn Tool>,
        hooks: Vec<Box<dyn HookHandler>>,
        broker: Arc<dyn PermissionBroker>,
    ) -> Self {
        Self::new_with_redactor(responses, tool, hooks, broker, Arc::new(NoopRedactor)).await
    }

    async fn new_with_redactor(
        responses: Vec<Vec<ModelStreamEvent>>,
        tool: Box<dyn Tool>,
        hooks: Vec<Box<dyn HookHandler>>,
        broker: Arc<dyn PermissionBroker>,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(SequenceModel::new(responses));
        let mut hook_builder = HookRegistry::builder();
        for hook in hooks {
            hook_builder = hook_builder.with_hook(hook);
        }
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![tool]))
            .build()
            .unwrap();
        let tools = ToolPool::assemble(
            &registry.snapshot(),
            &ToolPoolFilter::default(),
            &ToolSearchMode::Disabled,
            &ToolPoolModelProfile {
                provider: harness_contracts::ModelProvider("test".to_owned()),
                max_context_tokens: Some(8_000),
            },
            &SchemaResolverContext {
                run_id: RunId::new(),
                session_id,
                tenant_id,
            },
        )
        .await
        .unwrap();
        let engine = Engine::builder()
            .with_engine_id(EngineId::new("hook-pipeline-test"))
            .with_event_store(store.clone())
            .with_context(ContextEngine::builder().build().unwrap())
            .with_hooks(HookDispatcher::new(
                hook_builder.build().unwrap().snapshot(),
            ))
            .with_model(model.clone())
            .with_tools(tools)
            .with_permission_broker(broker)
            .with_workspace_root(workspace.path())
            .with_model_id("test-model")
            .with_protocol(ModelProtocol::Messages)
            .with_cap_registry(Arc::new(CapabilityRegistry::default()))
            .with_observer(Arc::new(
                Observer::builder().with_redactor(redactor).build().unwrap(),
            ))
            .build()
            .unwrap();

        Self {
            _workspace: workspace,
            tenant_id,
            session_id,
            engine,
            store,
            model,
        }
    }

    async fn run(&self, text: &str) -> Result<Vec<Event>, harness_contracts::EngineError> {
        self.run_with_permission_mode(text, PermissionMode::Default)
            .await
    }

    async fn run_with_permission_mode(
        &self,
        text: &str,
        permission_mode: PermissionMode,
    ) -> Result<Vec<Event>, harness_contracts::EngineError> {
        let events = self
            .engine
            .run(
                SessionHandle {
                    tenant_id: self.tenant_id,
                    session_id: self.session_id,
                },
                turn_input(text),
                RunContext::new(self.tenant_id, self.session_id, RunId::new())
                    .with_permission_mode(permission_mode),
            )
            .await?
            .collect::<Vec<_>>()
            .await;
        let stored = self
            .store
            .read(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert_eq!(events, stored);
        Ok(events)
    }
}

struct SequenceModel {
    responses: Mutex<Vec<Vec<ModelStreamEvent>>>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl SequenceModel {
    fn new(responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for SequenceModel {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Test model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        Ok(Box::pin(stream::iter(
            self.responses.lock().await.remove(0),
        )))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct EchoTool {
    descriptor: ToolDescriptor,
}

impl EchoTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "Echo".to_owned(),
                display_name: "Echo".to_owned(),
                description: "Echoes value.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["value"],
                    "properties": { "value": { "type": "string" } }
                }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 32_000,
                    on_overflow: OverflowAction::Reject,
                    preview_head_chars: 1_000,
                    preview_tail_chars: 1_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for EchoTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if input.get("value").and_then(Value::as_str).is_none() {
            return Err(ValidationError::from("value is required"));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        input: &Value,
        _ctx: &ToolContext,
    ) -> harness_permission::PermissionCheck {
        harness_permission::PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: "Echo".to_owned(),
                input: input.clone(),
            },
            scope: DecisionScope::ExactArgs(input.clone()),
        }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text(input["value"].as_str().unwrap_or_default().to_owned()),
        )])))
    }
}

struct FailingTool {
    descriptor: ToolDescriptor,
}

impl FailingTool {
    fn new() -> Self {
        let mut descriptor = EchoTool::new().descriptor;
        descriptor.name = "Fail".to_owned();
        descriptor.display_name = "Fail".to_owned();
        Self { descriptor }
    }
}

#[async_trait]
impl Tool for FailingTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if input.get("value").and_then(Value::as_str).is_none() {
            return Err(ValidationError::from("value is required"));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        input: &Value,
        _ctx: &ToolContext,
    ) -> harness_permission::PermissionCheck {
        harness_permission::PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: "Fail".to_owned(),
                input: input.clone(),
            },
            scope: DecisionScope::ExactArgs(input.clone()),
        }
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        Err(harness_contracts::ToolError::Internal(
            "failed with secret-token".to_owned(),
        ))
    }
}

struct RecordingBroker {
    decision: Decision,
    requests: Mutex<Vec<PermissionRequest>>,
}

impl RecordingBroker {
    fn new(decision: Decision) -> Self {
        Self {
            decision,
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<PermissionRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl PermissionBroker for RecordingBroker {
    async fn decide(&self, request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.requests.lock().await.push(request);
        self.decision.clone()
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct CaptureUserPromptInputHook {
    captured: Arc<Mutex<Option<Value>>>,
}

#[async_trait]
impl HookHandler for CaptureUserPromptInputHook {
    fn handler_id(&self) -> &str {
        "capture-user-prompt-input"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        let HookEvent::UserPromptSubmit { input, .. } = event else {
            unreachable!("unexpected event");
        };
        *self.captured.lock().await = Some(input);
        Ok(HookOutcome::Continue)
    }
}

struct CapturePreToolInputHook {
    captured: Arc<Mutex<Option<Value>>>,
}

#[async_trait]
impl HookHandler for CapturePreToolInputHook {
    fn handler_id(&self) -> &str {
        "capture-pre-tool-input"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        let HookEvent::PreToolUse { input, .. } = event else {
            unreachable!("unexpected event");
        };
        *self.captured.lock().await = Some(input);
        Ok(HookOutcome::Continue)
    }
}

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret-token", "[redacted]")
    }
}

struct RewritePreToolHook;

#[async_trait]
impl HookHandler for RewritePreToolHook {
    fn handler_id(&self) -> &str {
        "rewrite-pre-tool"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailClosed
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        assert!(matches!(event, HookEvent::PreToolUse { .. }));
        Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: Some(json!({ "value": "rewritten" })),
            override_permission: Some(Decision::AllowOnce),
            additional_context: None,
            block: None,
        }))
    }
}

struct RewriteCommandPreToolHook;

#[async_trait]
impl HookHandler for RewriteCommandPreToolHook {
    fn handler_id(&self) -> &str {
        "rewrite-command-pre-tool"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailClosed
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        assert!(matches!(event, HookEvent::PreToolUse { .. }));
        Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: Some(json!({ "command": "echo rewritten" })),
            override_permission: None,
            additional_context: None,
            block: None,
        }))
    }
}

struct ToolLifecycleHook;

#[async_trait]
impl HookHandler for ToolLifecycleHook {
    fn handler_id(&self) -> &str {
        "tool-lifecycle"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[
            HookEventKind::TransformTerminalOutput,
            HookEventKind::TransformToolResult,
            HookEventKind::PostToolUse,
            HookEventKind::PostToolUseFailure,
        ]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        match event {
            HookEvent::TransformTerminalOutput { raw, .. } => {
                assert_eq!(raw.as_ref(), b"original");
                Ok(HookOutcome::Transform(json!("terminal transformed")))
            }
            HookEvent::TransformToolResult { result, .. } => {
                assert_eq!(result, ToolResult::Text("terminal transformed".to_owned()));
                Ok(HookOutcome::Transform(json!("result transformed")))
            }
            HookEvent::PostToolUse { result, .. } => {
                assert_eq!(result, ToolResult::Text("result transformed".to_owned()));
                Ok(HookOutcome::AddContext(harness_hook::ContextPatch {
                    role: harness_hook::ContextPatchRole::AssistantHint,
                    content: "post context".to_owned(),
                    apply_to_next_turn_only: true,
                }))
            }
            HookEvent::PostToolUseFailure {
                error: ToolErrorView { .. },
                ..
            } => Ok(HookOutcome::Continue),
            _ => unreachable!("unexpected event"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CapturedPostToolPayloads {
    terminal_raw: Option<Vec<u8>>,
    transform_result: Option<ToolResult>,
    post_result: Option<ToolResult>,
}

struct CapturePostToolPayloadHook {
    captured: Arc<Mutex<CapturedPostToolPayloads>>,
}

#[async_trait]
impl HookHandler for CapturePostToolPayloadHook {
    fn handler_id(&self) -> &str {
        "capture-post-tool-payload"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[
            HookEventKind::TransformTerminalOutput,
            HookEventKind::TransformToolResult,
            HookEventKind::PostToolUse,
        ]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        let mut captured = self.captured.lock().await;
        match event {
            HookEvent::TransformTerminalOutput { raw, .. } => {
                captured.terminal_raw = Some(raw.to_vec());
            }
            HookEvent::TransformToolResult { result, .. } => {
                captured.transform_result = Some(result);
            }
            HookEvent::PostToolUse { result, .. } => {
                captured.post_result = Some(result);
            }
            _ => unreachable!("unexpected event"),
        }
        Ok(HookOutcome::Continue)
    }
}

struct CapturePostToolFailureHook {
    captured: Arc<Mutex<Option<ToolErrorView>>>,
}

#[async_trait]
impl HookHandler for CapturePostToolFailureHook {
    fn handler_id(&self) -> &str {
        "capture-post-tool-failure"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PostToolUseFailure]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        let HookEvent::PostToolUseFailure { error, .. } = event else {
            unreachable!("unexpected event");
        };
        *self.captured.lock().await = Some(error);
        Ok(HookOutcome::Continue)
    }
}

struct LlmApiHook;

#[async_trait]
impl HookHandler for LlmApiHook {
    fn handler_id(&self) -> &str {
        "llm-api"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[
            HookEventKind::PreLlmCall,
            HookEventKind::PreApiRequest,
            HookEventKind::PostLlmCall,
            HookEventKind::PostApiRequest,
        ]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        match event {
            HookEvent::PreLlmCall { request_view, .. } => {
                assert_eq!(request_view.model_id, "test-model");
            }
            HookEvent::PreApiRequest { endpoint, .. } => {
                assert!(endpoint.contains("test"));
            }
            HookEvent::PostLlmCall { .. } | HookEvent::PostApiRequest { status: 200, .. } => {}
            _ => unreachable!("unexpected event"),
        }
        Ok(HookOutcome::Continue)
    }
}

struct UserPromptHook;

#[async_trait]
impl HookHandler for UserPromptHook {
    fn handler_id(&self) -> &str {
        "user-prompt"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        match event {
            HookEvent::UserPromptSubmit { input, .. } => {
                assert_eq!(input["prompt"], json!("prompt event"));
                Ok(HookOutcome::Continue)
            }
            _ => unreachable!("unexpected event"),
        }
    }
}

struct PermissionRequestHook;

#[async_trait]
impl HookHandler for PermissionRequestHook {
    fn handler_id(&self) -> &str {
        "permission-request"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PermissionRequest]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        match event {
            HookEvent::PermissionRequest { subject, .. } => {
                assert_eq!(subject, "Echo");
                Ok(HookOutcome::Continue)
            }
            _ => unreachable!("unexpected event"),
        }
    }
}

#[derive(Debug, PartialEq)]
struct CapturedPermissionRequest {
    subject: String,
    detail: Option<String>,
}

struct CapturePermissionRequestHook {
    captured: Arc<Mutex<Option<CapturedPermissionRequest>>>,
}

#[async_trait]
impl HookHandler for CapturePermissionRequestHook {
    fn handler_id(&self) -> &str {
        "capture-permission-request"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PermissionRequest]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        let HookEvent::PermissionRequest {
            subject, detail, ..
        } = event
        else {
            unreachable!("unexpected event");
        };
        *self.captured.lock().await = Some(CapturedPermissionRequest { subject, detail });
        Ok(HookOutcome::Continue)
    }
}

struct ToolSearchLifecycleHook;

#[async_trait]
impl HookHandler for ToolSearchLifecycleHook {
    fn handler_id(&self) -> &str {
        "tool-search-lifecycle"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[
            HookEventKind::PreToolSearch,
            HookEventKind::PostToolSearchMaterialize,
        ]
    }

    async fn handle(
        &self,
        event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        match event {
            HookEvent::PreToolSearch { query, .. } => {
                assert_eq!(query, "select:Missing");
                Ok(HookOutcome::RewriteInput(json!({ "query": "select:Echo" })))
            }
            HookEvent::PostToolSearchMaterialize { materialized, .. } => {
                assert_eq!(materialized, vec!["Echo".to_owned()]);
                Ok(HookOutcome::Continue)
            }
            _ => unreachable!("unexpected event"),
        }
    }
}

struct EngineToolSearchRuntime {
    snapshot: ToolSearchRuntimeSnapshot,
    event_store: Arc<InMemoryEventStore>,
    hooks: HookDispatcher,
    tenant_id: TenantId,
    session_id: SessionId,
}

#[async_trait]
impl ToolSearchRuntimeCap for EngineToolSearchRuntime {
    async fn snapshot(&self) -> Result<ToolSearchRuntimeSnapshot, harness_contracts::ToolError> {
        Ok(self.snapshot.clone())
    }

    async fn emit_event(&self, event: Event) -> Result<(), harness_contracts::ToolError> {
        self.event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await
            .map(|_| ())
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))
    }

    async fn dispatch_pre_tool_search_hook(
        &self,
        ctx: &ToolContext,
        tool_use_id: ToolUseId,
        query: &str,
        query_kind: harness_contracts::ToolSearchQueryKind,
    ) -> Result<ToolSearchPreHookOutcome, harness_contracts::ToolError> {
        let result = self
            .hooks
            .dispatch(
                HookEvent::PreToolSearch {
                    tool_use_id,
                    query: query.to_owned(),
                    query_kind,
                },
                test_tool_search_hook_context(ctx),
            )
            .await
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        self.emit_test_hook_events(HookEventKind::PreToolSearch, &result)
            .await?;
        match result.final_outcome {
            HookOutcome::Continue => Ok(ToolSearchPreHookOutcome::Continue),
            HookOutcome::Block { reason } => Ok(ToolSearchPreHookOutcome::Block { reason }),
            HookOutcome::RewriteInput(value) => Ok(ToolSearchPreHookOutcome::RewriteInput(value)),
            _ => Ok(ToolSearchPreHookOutcome::Continue),
        }
    }

    async fn dispatch_post_tool_search_hook(
        &self,
        ctx: &ToolContext,
        tool_use_id: ToolUseId,
        materialized: Vec<harness_contracts::ToolName>,
        backend: harness_contracts::ToolLoadingBackendName,
        cache_impact: harness_contracts::CacheImpact,
    ) -> Result<(), harness_contracts::ToolError> {
        let result = self
            .hooks
            .dispatch(
                HookEvent::PostToolSearchMaterialize {
                    tool_use_id,
                    materialized,
                    backend,
                    cache_impact,
                },
                test_tool_search_hook_context(ctx),
            )
            .await
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        self.emit_test_hook_events(HookEventKind::PostToolSearchMaterialize, &result)
            .await
    }
}

impl EngineToolSearchRuntime {
    async fn emit_test_hook_events(
        &self,
        kind: HookEventKind,
        result: &harness_hook::DispatchResult,
    ) -> Result<(), harness_contracts::ToolError> {
        for record in &result.trail {
            self.emit_event(Event::HookTriggered(
                harness_contracts::HookTriggeredEvent {
                    hook_event_kind: kind.clone(),
                    handler_id: record.handler_id.clone(),
                    outcome_summary: test_hook_outcome_summary(&record.outcome),
                    duration_ms: record.duration.as_millis() as u64,
                    at: harness_contracts::now(),
                },
            ))
            .await?;
        }
        Ok(())
    }
}

fn test_hook_outcome_summary(outcome: &HookOutcome) -> harness_contracts::HookOutcomeSummary {
    match outcome {
        HookOutcome::Continue => harness_contracts::HookOutcomeSummary {
            continued: true,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::RewriteInput(_) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: true,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::Block { reason } => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: Some(reason.clone()),
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        _ => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
    }
}

fn test_tool_search_hook_context(ctx: &ToolContext) -> HookContext {
    HookContext {
        tenant_id: ctx.tenant_id,
        session_id: ctx.session_id,
        run_id: Some(ctx.run_id),
        turn_index: None,
        correlation_id: ctx.correlation_id,
        causation_id: harness_contracts::CausationId::new(),
        trust_level: TrustLevel::AdminTrusted,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        at: harness_contracts::now(),
        view: Arc::new(TestToolSearchHookView {
            workspace_root: ctx.workspace_root.clone(),
            redactor: NoopRedactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

struct TestToolSearchHookView {
    workspace_root: PathBuf,
    redactor: NoopRedactor,
}

impl HookSessionView for TestToolSearchHookView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(&self.workspace_root)
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        Vec::new()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn Redactor {
        &self.redactor
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

struct UnsupportedLlmHook;

#[async_trait]
impl HookHandler for UnsupportedLlmHook {
    fn handler_id(&self) -> &str {
        "unsupported-llm"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreLlmCall]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Transform(json!("unsupported")))
    }
}

struct InconsistentPreToolHook;

#[async_trait]
impl HookHandler for InconsistentPreToolHook {
    fn handler_id(&self) -> &str {
        "inconsistent-pre-tool"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: Some(json!({ "value": "rewritten" })),
            override_permission: None,
            additional_context: None,
            block: Some("exclusive block".to_owned()),
        }))
    }
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: json!({}),
    }
}

fn tool_call_events(name: &str, input: Value) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: name.to_owned(),
                input,
            },
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn text_events(text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(text.to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            MessagePart::ToolResult {
                content: ToolResult::Text(text),
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
