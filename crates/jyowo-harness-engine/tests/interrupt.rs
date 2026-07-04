use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CancelInitiator, CapabilityRegistry, Decision, DecisionScope, DeferPolicy,
    DeltaChunk, EndReason, Event, HookEventKind, Message, MessageId, MessagePart, MessageRole,
    ModelError, NetworkAccess, NoopRedactor, OverflowAction, PermissionError, PermissionSubject,
    ProviderRestriction, ResourceLimits, ResultBudget, RunId, SandboxExecutionCompletedEvent,
    SandboxExecutionStartedEvent, SandboxMode, SandboxPolicySummary, SandboxScope, SessionId,
    StopReason, TenantId, ToolActionPlan, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
    ToolProperties, ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
    WorkspaceAccess,
};
use harness_engine::{
    CancellationToken, Engine, EngineId, EngineRunner, InterruptCause, RunContext, SessionHandle,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookOutcome, HookRegistry,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_sandbox::{
    ActivityHandle, EventSink, ExecContext, ExecOutcome, ExecSpec, ProcessHandle, SandboxBackend,
    SandboxBaseConfig, SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, SchemaResolverContext, Tool,
    ToolContext, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry, ToolStream,
    ValidationError,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::{Mutex, Notify};

mod authorization_support;
use authorization_support::test_authorization_service;

#[tokio::test]
async fn pre_cancelled_run_records_user_cancel_without_calling_hook_or_model() {
    let token = CancellationToken::new();
    token.cancel(InterruptCause::User);
    let harness = InterruptHarness::new(
        ModelResponse::Events(text_events("unused")),
        HookMode::Count,
    )
    .await;

    let events = harness.run_with_token(token).await.unwrap();

    assert_end_reason(
        &events,
        EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
    );
    assert_eq!(harness.hook_calls.load(Ordering::SeqCst), 0);
    assert_eq!(harness.model.infer_calls().await, 0);
    assert_single_run_end(&events);
}

#[tokio::test]
async fn hook_cancel_stops_before_model_infer() {
    let token = CancellationToken::new();
    let harness = InterruptHarness::new(
        ModelResponse::Events(text_events("unused")),
        HookMode::Cancel(token.clone()),
    )
    .await;

    let events = harness.run_with_token(token).await.unwrap();

    assert_end_reason(
        &events,
        EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
    );
    assert_eq!(harness.hook_calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.model.infer_calls().await, 0);
    assert_single_run_end(&events);
}

#[tokio::test]
async fn model_stream_cancel_keeps_prior_delta_and_skips_completion() {
    let token = CancellationToken::new();
    let harness = InterruptHarness::new(
        ModelResponse::CancelAfterFirstDelta(token.clone()),
        HookMode::Count,
    )
    .await;

    let events = harness.run_with_token(token).await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantDeltaProduced(delta)
            if matches!(&delta.delta, DeltaChunk::Text(text) if text == "partial")
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::AssistantMessageCompleted(_))));
    assert_end_reason(
        &events,
        EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
    );
    assert_single_run_end(&events);
}

#[tokio::test]
async fn tool_dispatch_pre_cancel_does_not_execute_tool() {
    let token = CancellationToken::new();
    let harness = InterruptHarness::new(
        ModelResponse::CancelAfterToolRequest(token.clone()),
        HookMode::Count,
    )
    .await;

    let events = harness.run_with_token(token).await.unwrap();

    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert_eq!(harness.tool_executed.load(Ordering::SeqCst), 0);
    assert_end_reason(
        &events,
        EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
    );
    assert_single_run_end(&events);
}

#[tokio::test]
async fn tool_dispatch_mid_cancel_interrupts_tool_token() {
    let token = CancellationToken::new();
    let harness = InterruptHarness::new(
        ModelResponse::Events(tool_call_events("InterruptibleTool")),
        HookMode::Count,
    )
    .await;
    let run = tokio::spawn({
        let engine = harness.engine.clone();
        let session = harness.session_handle();
        let ctx = harness.run_context(token.clone());
        async move {
            engine
                .run(session, turn_input("call tool"), ctx)
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
        }
    });

    harness.tool_started.notified().await;
    token.cancel(InterruptCause::User);
    let events = run.await.unwrap();

    assert!(harness.tool_interrupted.load(Ordering::SeqCst));
    assert_end_reason(
        &events,
        EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
    );
    assert_single_run_end(&events);
}

#[tokio::test]
async fn tool_dispatch_mid_cancel_drains_queued_journal_events() {
    let token = CancellationToken::new();
    let harness = InterruptHarness::new_with_tool(
        ModelResponse::Events(tool_call_events("Bash")),
        HookMode::Count,
        "Bash",
        true,
    )
    .await;
    let run = tokio::spawn({
        let engine = harness.engine.clone();
        let session = harness.session_handle();
        let ctx = harness.run_context(token.clone());
        async move {
            engine
                .run(session, turn_input("call tool"), ctx)
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
        }
    });

    harness.tool_started.notified().await;
    token.cancel(InterruptCause::User);
    let events = run.await.unwrap();
    let stored_events = harness.stored_events().await;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxExecutionStarted(started) if started.backend_id == "queued-journal"
        )
    }));
    assert!(stored_events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxExecutionStarted(started) if started.backend_id == "queued-journal"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxExecutionCompleted(completed) if completed.backend_id == "queued-journal"
        )
    }));
    assert!(stored_events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxExecutionCompleted(completed) if completed.backend_id == "queued-journal"
        )
    }));
    assert_end_reason(
        &events,
        EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
    );
    assert_single_run_end(&events);
}

#[tokio::test]
async fn interrupt_causes_map_to_end_reason() {
    let cases = [
        (
            InterruptCause::Parent,
            EndReason::Cancelled {
                initiator: CancelInitiator::Parent,
            },
        ),
        (
            InterruptCause::System {
                reason: "quota".to_owned(),
            },
            EndReason::Cancelled {
                initiator: CancelInitiator::System {
                    reason: "quota".to_owned(),
                },
            },
        ),
        (InterruptCause::Timeout, EndReason::Interrupted),
        (InterruptCause::Budget, EndReason::TokenBudgetExhausted),
    ];

    for (cause, expected) in cases {
        let token = CancellationToken::new();
        token.cancel(cause);
        let harness = InterruptHarness::new(
            ModelResponse::Events(text_events("unused")),
            HookMode::Count,
        )
        .await;

        let events = harness.run_with_token(token).await.unwrap();

        assert_end_reason(&events, expected);
        assert_single_run_end(&events);
    }
}

struct InterruptHarness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    engine: Engine,
    store: Arc<InMemoryEventStore>,
    model: Arc<RecordingModelProvider>,
    hook_calls: Arc<AtomicUsize>,
    tool_executed: Arc<AtomicUsize>,
    tool_started: Arc<Notify>,
    tool_interrupted: Arc<AtomicBool>,
}

impl InterruptHarness {
    async fn new(response: ModelResponse, hook_mode: HookMode) -> Self {
        Self::new_with_tool(response, hook_mode, "InterruptibleTool", false).await
    }

    async fn new_with_tool(
        response: ModelResponse,
        hook_mode: HookMode,
        tool_name: &str,
        emit_journal_before_wait: bool,
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(RecordingModelProvider::new(response));
        let hook_calls = Arc::new(AtomicUsize::new(0));
        let hooks = HookRegistry::builder()
            .with_hook(Box::new(TestHook {
                calls: hook_calls.clone(),
                mode: hook_mode,
            }))
            .build()
            .unwrap();
        let tool_executed = Arc::new(AtomicUsize::new(0));
        let tool_started = Arc::new(Notify::new());
        let tool_interrupted = Arc::new(AtomicBool::new(false));
        let mut registry_builder = ToolRegistry::builder();
        let sandbox = if emit_journal_before_wait {
            registry_builder =
                registry_builder.with_builtin_toolset(harness_tool::BuiltinToolset::Shell);
            Some(Arc::new(QueuedJournalSandbox {
                started: tool_started.clone(),
                interrupted: tool_interrupted.clone(),
            }) as Arc<dyn SandboxBackend>)
        } else {
            registry_builder =
                registry_builder.with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![
                    Box::new(InterruptibleTool::new(
                        tool_name,
                        tool_executed.clone(),
                        tool_started.clone(),
                        tool_interrupted.clone(),
                    )),
                ]));
            None
        };
        let registry = registry_builder.build().unwrap();
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
        let mut engine_builder = Engine::builder()
            .with_engine_id(EngineId::new("interrupt-test"))
            .with_event_store(store.clone())
            .with_context(ContextEngine::builder().build().unwrap())
            .with_hooks(HookDispatcher::new(hooks.snapshot()))
            .with_model(model.clone())
            .with_tools(tools)
            .with_authorization_service(test_authorization_service(
                Arc::new(AllowBroker),
                store.clone(),
            ))
            .with_workspace_root(workspace.path())
            .with_model_id("test-model")
            .with_protocol(ModelProtocol::Messages)
            .with_cap_registry(Arc::new(CapabilityRegistry::default()));
        if let Some(sandbox) = sandbox {
            engine_builder = engine_builder.with_sandbox(sandbox);
        }
        let engine = engine_builder.build().unwrap();

        Self {
            _workspace: workspace,
            tenant_id,
            session_id,
            engine,
            store,
            model,
            hook_calls,
            tool_executed,
            tool_started,
            tool_interrupted,
        }
    }

    fn session_handle(&self) -> SessionHandle {
        SessionHandle {
            tenant_id: self.tenant_id,
            session_id: self.session_id,
        }
    }

    fn run_context(&self, cancellation: CancellationToken) -> RunContext {
        RunContext::new(self.tenant_id, self.session_id, RunId::new())
            .with_cancellation(cancellation)
    }

    async fn run_with_token(
        &self,
        token: CancellationToken,
    ) -> Result<Vec<Event>, harness_contracts::EngineError> {
        Ok(self
            .engine
            .run(
                self.session_handle(),
                turn_input("hello"),
                self.run_context(token),
            )
            .await?
            .collect::<Vec<_>>()
            .await)
    }

    async fn stored_events(&self) -> Vec<Event> {
        self.store
            .read(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
    }
}

#[derive(Clone)]
enum HookMode {
    Count,
    Cancel(CancellationToken),
}

struct TestHook {
    calls: Arc<AtomicUsize>,
    mode: HookMode,
}

#[async_trait]
impl HookHandler for TestHook {
    fn handler_id(&self) -> &'static str {
        "interrupt-test-hook"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let HookMode::Cancel(token) = &self.mode {
            token.cancel(InterruptCause::User);
        }
        Ok(HookOutcome::Continue)
    }
}

struct RecordingModelProvider {
    response: ModelResponse,
    calls: Mutex<usize>,
}

impl RecordingModelProvider {
    fn new(response: ModelResponse) -> Self {
        Self {
            response,
            calls: Mutex::new(0),
        }
    }

    async fn infer_calls(&self) -> usize {
        *self.calls.lock().await
    }
}

#[async_trait]
impl ModelProvider for RecordingModelProvider {
    fn provider_id(&self) -> &'static str {
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
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        *self.calls.lock().await += 1;
        match self.response.clone() {
            ModelResponse::Events(events) => Ok(Box::pin(stream::iter(events))),
            ModelResponse::CancelAfterFirstDelta(token) => Ok(Box::pin(stream::unfold(
                (0_u8, token),
                |(step, token): (u8, CancellationToken)| async move {
                    match step {
                        0 => Some((
                            ModelStreamEvent::MessageStart {
                                message_id: "assistant-1".to_owned(),
                                usage: UsageSnapshot::default(),
                            },
                            (1, token),
                        )),
                        1 => {
                            token.cancel(InterruptCause::User);
                            Some((
                                ModelStreamEvent::ContentBlockDelta {
                                    index: 0,
                                    delta: ContentDelta::Text("partial".to_owned()),
                                },
                                (2, token),
                            ))
                        }
                        _ => None,
                    }
                },
            ))),
            ModelResponse::CancelAfterToolRequest(token) => Ok(Box::pin(stream::unfold(
                (0_u8, token),
                |(step, token): (u8, CancellationToken)| async move {
                    match step {
                        0 => Some((
                            ModelStreamEvent::MessageStart {
                                message_id: "assistant-1".to_owned(),
                                usage: UsageSnapshot::default(),
                            },
                            (1, token),
                        )),
                        1 => Some((
                            ModelStreamEvent::ContentBlockDelta {
                                index: 0,
                                delta: ContentDelta::ToolUseComplete {
                                    id: ToolUseId::new(),
                                    name: "InterruptibleTool".to_owned(),
                                    input: json!({}),
                                },
                            },
                            (2, token),
                        )),
                        2 => {
                            token.cancel(InterruptCause::User);
                            Some((ModelStreamEvent::MessageStop, (3, token)))
                        }
                        _ => None,
                    }
                },
            ))),
        }
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Clone)]
enum ModelResponse {
    Events(Vec<ModelStreamEvent>),
    CancelAfterFirstDelta(CancellationToken),
    CancelAfterToolRequest(CancellationToken),
}

#[derive(Default)]
struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct InterruptibleTool {
    descriptor: ToolDescriptor,
    executed: Arc<AtomicUsize>,
    started: Arc<Notify>,
    interrupted: Arc<AtomicBool>,
}

impl InterruptibleTool {
    fn new(
        name: &str,
        executed: Arc<AtomicUsize>,
        started: Arc<Notify>,
        interrupted: Arc<AtomicBool>,
    ) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: "Interruptible tool".to_owned(),
                description: "Waits until interrupted.".to_owned(),
                category: "test".to_owned(),
                group: if name == "Bash" {
                    ToolGroup::Shell
                } else {
                    ToolGroup::FileSystem
                },
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
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 32_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
            executed,
            started,
            interrupted,
        }
    }
}

#[async_trait]
impl Tool for InterruptibleTool {
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
            harness_permission::PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: "InterruptibleTool".to_owned(),
                    input: input.clone(),
                },
                scope: DecisionScope::Any,
            },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        self.executed.fetch_add(1, Ordering::SeqCst);
        self.started.notify_waiters();
        loop {
            if ctx.interrupt.is_interrupted() {
                self.interrupted.store(true, Ordering::SeqCst);
                return Err(ToolError::Interrupted);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

struct QueuedJournalSandbox {
    started: Arc<Notify>,
    interrupted: Arc<AtomicBool>,
}

#[async_trait]
impl SandboxBackend for QueuedJournalSandbox {
    fn backend_id(&self) -> &str {
        "queued-journal"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            supports_network: true,
            supports_filesystem_write: true,
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, harness_contracts::SandboxError> {
        let fingerprint = spec.canonical_fingerprint(&SandboxBaseConfig::default());
        ctx.event_sink.emit(Event::SandboxExecutionStarted(
            SandboxExecutionStartedEvent {
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                backend_id: "queued-journal".to_owned(),
                fingerprint,
                policy: SandboxPolicySummary {
                    mode: SandboxMode::None,
                    scope: SandboxScope::WorkspaceOnly,
                    network: NetworkAccess::None,
                    resource_limits: ResourceLimits {
                        max_memory_bytes: None,
                        max_cpu_cores: None,
                        max_pids: None,
                        max_wall_clock_ms: None,
                        max_open_files: None,
                    },
                },
                at: harness_contracts::now(),
            },
        ))?;
        self.started.notify_waiters();
        Ok(ProcessHandle {
            pid: Some(7),
            stdout: Some(Box::pin(stream::pending())),
            stderr: None,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(QueuedJournalActivity {
                interrupted: self.interrupted.clone(),
                completed_emitted: AtomicBool::new(false),
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                event_sink: ctx.event_sink.clone(),
                fingerprint,
            }),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, harness_contracts::SandboxError> {
        Ok(SessionSnapshotFile::default())
    }

    async fn restore_session(
        &self,
        _snapshot: &SessionSnapshotFile,
    ) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }
}

struct QueuedJournalActivity {
    interrupted: Arc<AtomicBool>,
    completed_emitted: AtomicBool,
    session_id: SessionId,
    run_id: RunId,
    tool_use_id: Option<ToolUseId>,
    event_sink: Arc<dyn EventSink>,
    fingerprint: harness_contracts::ExecFingerprint,
}

#[async_trait]
impl ActivityHandle for QueuedJournalActivity {
    async fn wait(&self) -> Result<ExecOutcome, harness_contracts::SandboxError> {
        loop {
            if self.interrupted.load(Ordering::SeqCst) {
                let now = harness_contracts::now();
                if self
                    .completed_emitted
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    self.event_sink.emit(Event::SandboxExecutionCompleted(
                        SandboxExecutionCompletedEvent {
                            session_id: self.session_id,
                            run_id: self.run_id,
                            tool_use_id: self.tool_use_id,
                            backend_id: "queued-journal".to_owned(),
                            fingerprint: self.fingerprint,
                            exit_status: harness_contracts::SandboxExitStatus::Cancelled,
                            stdout_bytes_observed: 0,
                            stderr_bytes_observed: 0,
                            duration_ms: 0,
                            overflow: None,
                            at: now,
                        },
                    ))?;
                }
                return Ok(ExecOutcome {
                    exit_status: harness_contracts::SandboxExitStatus::Cancelled,
                    started_at: now,
                    finished_at: now,
                    stdout_bytes_observed: 0,
                    stderr_bytes_observed: 0,
                    overflow: None,
                });
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    async fn kill(
        &self,
        _signal: i32,
        _scope: harness_contracts::KillScope,
    ) -> Result<(), harness_contracts::SandboxError> {
        self.interrupted.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> std::time::Instant {
        std::time::Instant::now()
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

fn tool_call_events(name: &str) -> Vec<ModelStreamEvent> {
    let input = if name == "Bash" {
        json!({ "command": "printf queued-journal" })
    } else {
        json!({})
    };
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

fn assert_end_reason(events: &[Event], expected: EndReason) {
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunEnded(ended) if ended.reason == expected)));
}

fn assert_single_run_end(events: &[Event]) {
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::RunEnded(_)))
            .count(),
        1
    );
}
