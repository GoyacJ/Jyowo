use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    Decision, Event, HookError, HookEventKind, InteractivityLevel, Message, MessagePart,
    MessageRole, ModelError, NoopRedactor, PermissionError, PermissionMode, RedactRules, Redactor,
    RunId, SessionId, StopReason, TenantId, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineRunner, RunContext, SessionHandle};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookOutcome, HookRegistry,
};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_observability::Observer;
use harness_permission::{
    PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
};
use harness_tool::ToolPool;
use serde_json::json;
use tokio::sync::Mutex;

#[tokio::test]
async fn hook_context_uses_runtime_permission_interactivity_and_redactor() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let captured = Arc::new(Mutex::new(None));
    let hook = CaptureContextHook {
        captured: Arc::clone(&captured),
    };
    let registry = HookRegistry::builder()
        .with_hook(Box::new(hook))
        .build()
        .unwrap();
    let observer = Arc::new(
        Observer::builder()
            .with_redactor(Arc::new(MarkerRedactor))
            .build()
            .unwrap(),
    );

    let engine = Engine::builder()
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(registry.snapshot()))
        .with_model(Arc::new(OneShotModel))
        .with_tools(ToolPool::default())
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_protocol(ModelProtocol::Messages)
        .with_observer(observer)
        .build()
        .unwrap();

    let correlation_id = harness_contracts::CorrelationId::new();
    let ctx = RunContext::new(tenant_id, session_id, run_id)
        .with_correlation_id(correlation_id)
        .with_permission_mode(PermissionMode::AcceptEdits)
        .with_interactivity(InteractivityLevel::FullyInteractive);

    engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("capture"),
            ctx,
        )
        .await
        .unwrap()
        .collect::<Vec<Event>>()
        .await;

    let captured = captured.lock().await.take().expect("context captured");
    assert_eq!(captured.permission_mode, PermissionMode::AcceptEdits);
    assert_eq!(captured.view_permission_mode, PermissionMode::AcceptEdits);
    assert_eq!(captured.interactivity, InteractivityLevel::FullyInteractive);
    assert_eq!(captured.correlation_id, correlation_id);
    assert_eq!(captured.redacted, "marked:secret");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedContext {
    permission_mode: PermissionMode,
    view_permission_mode: PermissionMode,
    interactivity: InteractivityLevel,
    correlation_id: harness_contracts::CorrelationId,
    redacted: String,
}

struct CaptureContextHook {
    captured: Arc<Mutex<Option<CapturedContext>>>,
}

#[async_trait]
impl HookHandler for CaptureContextHook {
    fn handler_id(&self) -> &str {
        "capture-context"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreLlmCall]
    }

    async fn handle(&self, _event: HookEvent, ctx: HookContext) -> Result<HookOutcome, HookError> {
        *self.captured.lock().await = Some(CapturedContext {
            permission_mode: ctx.permission_mode,
            view_permission_mode: ctx.view.permission_mode(),
            interactivity: ctx.interactivity,
            correlation_id: ctx.correlation_id,
            redacted: ctx
                .view
                .redacted()
                .redact("secret", &RedactRules::default()),
        });
        Ok(HookOutcome::Continue)
    }
}

struct MarkerRedactor;

impl Redactor for MarkerRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        format!("marked:{input}")
    }
}

struct OneShotModel;

#[async_trait]
impl ModelProvider for OneShotModel {
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
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter(text_events("done"))))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
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

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: harness_contracts::MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: json!({}),
    }
}
