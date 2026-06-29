use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, Decision, DecisionScope, DeferPolicy, Event, Message,
    MessageId, MessagePart, MessageRole, ModelError, NoopRedactor, OverflowAction, PermissionMode,
    PermissionRequestSuppressedEvent, PermissionSubject, ProviderRestriction, ResultBudget, RunId,
    SessionId, StopReason, TenantId, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::{
    SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool, ToolPoolFilter,
    ToolPoolModelProfile, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[tokio::test]
async fn permission_suppression_emits_event() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let broker = Arc::new(CountingBroker::default());
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![Box::new(
            EchoTool::new(),
        )]))
        .build()
        .unwrap();
    let tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &harness_contracts::ToolSearchMode::Disabled,
        &ToolPoolModelProfile {
            provider: harness_contracts::ModelProvider("mock".to_owned()),
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
    let engine = Engine::builder()
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(Arc::new(TwoStepModel::new()))
        .with_tools(tools)
        .with_permission_broker(broker.clone())
        .with_workspace_root(workspace.path())
        .with_model_id("mock-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();

    engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("dedup"),
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

    assert_eq!(broker.calls.load(Ordering::SeqCst), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionRequestSuppressed(PermissionRequestSuppressedEvent {
            reused_decision: Some(Decision::AllowOnce),
            ..
        })
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::PermissionRequested(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn bypass_permission_mode_journals_request_context_for_audit() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let broker = Arc::new(CountingBroker::default());
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![Box::new(
            EchoTool::new(),
        )]))
        .build()
        .unwrap();
    let tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &harness_contracts::ToolSearchMode::Disabled,
        &ToolPoolModelProfile {
            provider: harness_contracts::ModelProvider("mock".to_owned()),
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
    let engine = Engine::builder()
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(Arc::new(TwoStepModel::new()))
        .with_tools(tools)
        .with_permission_broker(broker.clone())
        .with_workspace_root(workspace.path())
        .with_model_id("mock-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();

    engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("bypass audit"),
            RunContext::new(tenant_id, session_id, run_id)
                .with_permission_mode(PermissionMode::BypassPermissions),
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
    let requested = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionRequested(requested) => Some(requested),
            _ => None,
        })
        .expect("bypass mode should still journal permission request context");
    let resolved = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionResolved(resolved) if resolved.request_id == requested.request_id => {
                Some(resolved)
            }
            _ => None,
        })
        .expect("bypass mode should resolve the journaled permission request");

    assert_eq!(requested.run_id, run_id);
    assert_eq!(requested.session_id, session_id);
    assert_eq!(requested.tenant_id, tenant_id);
    assert!(matches!(resolved.decision, Decision::AllowOnce));
}

#[derive(Default)]
struct CountingBroker {
    calls: AtomicUsize,
}

#[async_trait]
impl PermissionBroker for CountingBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct TwoStepModel {
    responses: Mutex<Vec<Vec<ModelStreamEvent>>>,
}

impl TwoStepModel {
    fn new() -> Self {
        Self {
            responses: Mutex::new(vec![two_tool_call_events(), text_events("done")]),
        }
    }
}

#[async_trait]
impl ModelProvider for TwoStepModel {
    fn provider_id(&self) -> &str {
        "mock"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "mock".to_owned(),
            model_id: "mock-model".to_owned(),
            display_name: "Mock model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            conversation_capability: ConversationModelCapability::default(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter(
            self.responses.lock().await.remove(0),
        )))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

fn two_tool_call_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "Echo".to_owned(),
                input: json!({ "value": "same" }),
            },
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "Echo".to_owned(),
                input: json!({ "value": "same" }),
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
            message_id: "assistant-2".to_owned(),
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
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                input_schema: json!({
                    "type": "object",
                    "properties": { "value": { "type": "string" } },
                    "required": ["value"]
                }),
                output_schema: None,
                dynamic_schema: false,
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
            },
        }
    }
}

#[async_trait]
impl Tool for EchoTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(
        &self,
        input: &Value,
        _ctx: &ToolContext,
    ) -> harness_permission::PermissionCheck {
        harness_permission::PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            scope: DecisionScope::ExactArgs(input.clone()),
        }
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter(vec![ToolEvent::Final(
            ToolResult::Text(input["value"].as_str().unwrap_or_default().to_owned()),
        )])))
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
