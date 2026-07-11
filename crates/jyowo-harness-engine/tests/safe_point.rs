use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, Decision, DeferPolicy, DeltaChunk, EndReason, Event, Message,
    MessageId, MessagePart, MessageRole, ModelError, NetworkAccess, OverflowAction,
    PermissionError, ProviderRestriction, ResultBudget, RunId, SessionId, StopReason, TenantId,
    ToolActionPlan, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
    WorkspaceAccess,
};
use harness_engine::{
    Engine, EngineRunner, RunContext, RunControl, RunControlHandle, SafePointDecision,
    SessionHandle, TurnOutcome,
};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, SchemaResolverContext, Tool,
    ToolContext, ToolEvent, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry,
    ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tokio::sync::Semaphore;

mod authorization_support;
use authorization_support::test_authorization_service;

#[tokio::test]
async fn yield_request_is_observed_only_at_a_safe_boundary() {
    let control = RunControlHandle::new();

    assert_eq!(control.decision(), SafePointDecision::Continue);
    control.request(RunControl::YieldAfterAtomicOperation);
    assert_eq!(control.decision(), SafePointDecision::Yield);

    control.finish(TurnOutcome::YieldedAtSafePoint);
    assert_eq!(control.outcome().await, TurnOutcome::YieldedAtSafePoint);
}

#[test]
fn finished_outcome_is_available_without_blocking_after_the_run_finishes() {
    let control = RunControlHandle::new();

    assert_eq!(control.finished_outcome(), None);
    control.finish(TurnOutcome::YieldedAtSafePoint);
    assert_eq!(
        control.finished_outcome(),
        Some(TurnOutcome::YieldedAtSafePoint)
    );
}

#[tokio::test]
async fn force_stop_dominates_a_pending_yield_request() {
    let control = RunControlHandle::new();
    control.request(RunControl::YieldAfterAtomicOperation);
    control.request(RunControl::ForceStop);

    assert_eq!(control.decision(), SafePointDecision::ForceStop);
    control.finish(TurnOutcome::ForceStopped {
        non_revertible_tool_use_ids: Vec::new(),
    });
    assert_eq!(
        control.outcome().await,
        TurnOutcome::ForceStopped {
            non_revertible_tool_use_ids: Vec::new(),
        }
    );
}

#[tokio::test]
async fn daemon_yield_waits_for_the_running_tool_and_does_not_start_the_next_tool() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(
        harness_contracts::NoopRedactor,
    )));
    let model = Arc::new(TwoToolModel::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![Box::new(
            BlockingTool::new(calls.clone(), started.clone(), release.clone()),
        )]))
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
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model.clone())
        .with_tools(tools)
        .with_authorization_service(test_authorization_service(Arc::new(AllowBroker), store))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();
    let control = RunControlHandle::new();
    let run = tokio::spawn({
        let control = control.clone();
        async move {
            engine
                .run(
                    SessionHandle {
                        tenant_id,
                        session_id,
                    },
                    turn_input("run both tools"),
                    RunContext::new(tenant_id, session_id, RunId::new()).with_run_control(control),
                )
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
        }
    });

    started.acquire().await.unwrap().forget();
    control.request(RunControl::YieldAfterAtomicOperation);
    release.add_permits(2);
    let events = run.await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(control.outcome().await, TurnOutcome::YieldedAtSafePoint);
    assert!(events.iter().any(
        |event| matches!(event, Event::RunEnded(ended) if ended.reason == EndReason::Interrupted)
    ));
}

#[tokio::test]
async fn yield_during_model_stream_keeps_partial_assistant_text() {
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let control = RunControlHandle::new();
    let (_workspace, engine) = test_engine(
        tenant_id,
        session_id,
        Arc::new(YieldAfterPartialModel {
            control: control.clone(),
        }),
        Vec::new(),
    )
    .await;

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("stream"),
            RunContext::new(tenant_id, session_id, RunId::new()).with_run_control(control.clone()),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantDeltaProduced(delta)
            if matches!(&delta.delta, DeltaChunk::Text(text) if text == "partial")
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::AssistantMessageCompleted(_))));
    assert_eq!(control.outcome().await, TurnOutcome::YieldedAtSafePoint);
}

#[tokio::test]
async fn force_stop_interrupts_a_cancellable_tool() {
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let control = RunControlHandle::new();
    let started = Arc::new(Semaphore::new(0));
    let interrupted = Arc::new(AtomicBool::new(false));
    let (_workspace, engine) = test_engine(
        tenant_id,
        session_id,
        Arc::new(OneToolModel),
        vec![Box::new(CancellableTool::new(
            started.clone(),
            interrupted.clone(),
        ))],
    )
    .await;
    let run = tokio::spawn({
        let control = control.clone();
        async move {
            engine
                .run(
                    SessionHandle {
                        tenant_id,
                        session_id,
                    },
                    turn_input("run tool"),
                    RunContext::new(tenant_id, session_id, RunId::new()).with_run_control(control),
                )
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
        }
    });

    started.acquire().await.unwrap().forget();
    control.request(RunControl::ForceStop);
    let events = run.await.unwrap();

    assert!(interrupted.load(Ordering::SeqCst));
    assert!(matches!(
        control.outcome().await,
        TurnOutcome::ForceStopped {
            non_revertible_tool_use_ids,
        } if non_revertible_tool_use_ids.len() == 1
    ));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseFailed(_))));
}

#[tokio::test]
async fn force_stop_timeout_marks_the_atomic_tool_indeterminate() {
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let control = RunControlHandle::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (_workspace, engine) = test_engine(
        tenant_id,
        session_id,
        Arc::new(NamedOneToolModel("BlockingTool")),
        vec![Box::new(BlockingTool::new(
            calls,
            started.clone(),
            release.clone(),
        ))],
    )
    .await;
    let mut run = tokio::spawn({
        let control = control.clone();
        async move {
            engine
                .run(
                    SessionHandle {
                        tenant_id,
                        session_id,
                    },
                    turn_input("run tool"),
                    RunContext::new(tenant_id, session_id, RunId::new()).with_run_control(control),
                )
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
        }
    });

    started.acquire().await.unwrap().forget();
    control.request(RunControl::ForceStop);
    tokio::time::timeout(Duration::from_millis(5_500), &mut run)
        .await
        .expect("force stop must leave an unresponsive atomic tool after a finite grace period")
        .unwrap();
    assert!(matches!(
        control.outcome().await,
        TurnOutcome::ForceStopTimedOut {
            indeterminate_tool_use_ids,
        } if indeterminate_tool_use_ids.len() == 1
    ));
}

struct YieldAfterPartialModel {
    control: RunControlHandle,
}

#[async_trait]
impl ModelProvider for YieldAfterPartialModel {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        test_model_descriptors()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let control = self.control.clone();
        Ok(Box::pin(stream::unfold(0_u8, move |step| {
            let control = control.clone();
            async move {
                match step {
                    0 => Some((
                        ModelStreamEvent::MessageStart {
                            message_id: "assistant-partial".to_owned(),
                            usage: UsageSnapshot::default(),
                        },
                        1,
                    )),
                    1 => {
                        control.request(RunControl::YieldAfterAtomicOperation);
                        Some((
                            ModelStreamEvent::ContentBlockDelta {
                                index: 0,
                                delta: ContentDelta::Text("partial".to_owned()),
                            },
                            2,
                        ))
                    }
                    _ => None,
                }
            }
        })))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct OneToolModel;

#[async_trait]
impl ModelProvider for OneToolModel {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        test_model_descriptors()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter(one_tool_events("CancellableTool"))))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct NamedOneToolModel(&'static str);

#[async_trait]
impl ModelProvider for NamedOneToolModel {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        test_model_descriptors()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter(one_tool_events(self.0))))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Default)]
struct TwoToolModel {
    calls: AtomicUsize,
}

#[async_trait]
impl ModelProvider for TwoToolModel {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: ModelProtocol::Messages,
            supported_parameters: Vec::new(),
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Test model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            provider_declared_capability: ConversationModelCapability::default(),
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                ModelProtocol::Messages,
            ),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Ok(Box::pin(stream::iter(two_tool_events())));
        }
        Ok(Box::pin(stream::iter(text_events("unexpected"))))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
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

struct BlockingTool {
    descriptor: ToolDescriptor,
    calls: Arc<AtomicUsize>,
    started: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

impl BlockingTool {
    fn new(calls: Arc<AtomicUsize>, started: Arc<Semaphore>, release: Arc<Semaphore>) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "BlockingTool".to_owned(),
                display_name: "Blocking tool".to_owned(),
                description: "Blocks until the test releases it.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::FileSystem,
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
                metadata: harness_contracts::ToolDescriptorMetadata::default(),
            },
            calls,
            started,
            release,
        }
    }
}

#[async_trait]
impl Tool for BlockingTool {
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
            harness_permission::PermissionCheck::Allowed,
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("done".to_owned()),
        )])))
    }
}

struct CancellableTool {
    descriptor: ToolDescriptor,
    started: Arc<Semaphore>,
    interrupted: Arc<AtomicBool>,
}

impl CancellableTool {
    fn new(started: Arc<Semaphore>, interrupted: Arc<AtomicBool>) -> Self {
        Self {
            descriptor: test_tool_descriptor("CancellableTool"),
            started,
            interrupted,
        }
    }
}

#[async_trait]
impl Tool for CancellableTool {
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
            harness_permission::PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        self.started.add_permits(1);
        loop {
            if ctx.interrupt.is_interrupted() {
                self.interrupted.store(true, Ordering::SeqCst);
                return Err(ToolError::Interrupted);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

async fn test_engine(
    tenant_id: TenantId,
    session_id: SessionId,
    model: Arc<dyn ModelProvider>,
    custom_tools: Vec<Box<dyn Tool>>,
) -> (tempfile::TempDir, Engine) {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(
        harness_contracts::NoopRedactor,
    )));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(custom_tools))
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
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model)
        .with_tools(tools)
        .with_authorization_service(test_authorization_service(Arc::new(AllowBroker), store))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();
    (workspace, engine)
}

fn test_model_descriptors() -> Vec<ModelDescriptor> {
    vec![ModelDescriptor {
        protocol: ModelProtocol::Messages,
        supported_parameters: Vec::new(),
        lifecycle: harness_model::ModelLifecycle::Stable,
        provider_id: "test".to_owned(),
        model_id: "test-model".to_owned(),
        display_name: "Test model".to_owned(),
        context_window: 8_000,
        max_output_tokens: 1_000,
        provider_declared_capability: ConversationModelCapability::default(),
        conversation_capability: ConversationModelCapability::default(),
        runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
            ModelProtocol::Messages,
        ),
        pricing: None,
    }]
}

fn test_tool_descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: "Test tool".to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::FileSystem,
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
        metadata: harness_contracts::ToolDescriptorMetadata::default(),
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

fn two_tool_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-tools".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "BlockingTool".to_owned(),
                input: json!({}),
            },
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "BlockingTool".to_owned(),
                input: json!({}),
            },
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn one_tool_events(name: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-tool".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: name.to_owned(),
                input: json!({}),
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
            message_id: "assistant-text".to_owned(),
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
