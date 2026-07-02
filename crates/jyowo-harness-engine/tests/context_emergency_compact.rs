use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::{
    CompactHint, ContextBuffer, ContextEngine, ContextOutcome, ContextProvider, TokenBudget,
};
use harness_contracts::{
    BudgetExceedanceSource, CapabilityRegistry, ContextError, ContextStageId, Decision, EndReason,
    Event, Message, MessageContent, MessageId, MessagePart, MessageRole, ModelError, NoopRedactor,
    PermissionError, RunId, SessionId, StopReason, TenantId, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::ToolPool;
use serde_json::json;
use tokio::sync::Mutex;

#[tokio::test]
async fn context_too_long_retries_once_with_emergency_compacted_prompt() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ContextTooLongThenOkModel::default());
    let context = ContextEngine::builder()
        .with_provider(EmergencyTestProvider)
        .build()
        .unwrap();
    let engine = Engine::builder()
        .with_engine_id(EngineId::new("context-emergency-compact-test"))
        .with_event_store(store)
        .with_context(context)
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model.clone())
        .with_tools(ToolPool::default())
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("make this fit"),
            RunContext::new(tenant_id, session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let requests = model.requests().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1]
        .messages
        .iter()
        .any(|message| message_text(message).contains("[EMERGENCY_COMPACTED]")));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ContextBudgetExceeded(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ContextStageTransitioned(stage)
            if stage.stage == ContextStageId::Snip && stage.provider_id == "emergency-test"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(done)
            if done.content == MessageContent::Text("after compact".to_owned())
    )));
    assert!(events.iter().any(
        |event| matches!(event, Event::RunEnded(ended) if ended.reason == EndReason::Completed)
    ));
}

#[tokio::test]
async fn soft_budget_compacts_before_first_model_request() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(OkRecordingModel::default());
    let context = ContextEngine::builder()
        .with_budget(TokenBudget {
            max_tokens_per_turn: 10,
            soft_budget_ratio: 0.5,
            hard_budget_ratio: 0.95,
            ..TokenBudget::default()
        })
        .with_provider(EmergencyTestProvider)
        .build()
        .unwrap();
    let engine = Engine::builder()
        .with_engine_id(EngineId::new("context-soft-budget-compact-test"))
        .with_event_store(store)
        .with_context(context)
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model.clone())
        .with_tools(ToolPool::default())
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("this user prompt is long enough to cross the soft context budget"),
            RunContext::new(tenant_id, session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let requests = model.requests().await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .messages
        .iter()
        .any(|message| message_text(message).contains("[EMERGENCY_COMPACTED]")));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ContextBudgetExceeded(exceeded)
            if exceeded.source == BudgetExceedanceSource::LocalEstimate
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ContextStageTransitioned(stage)
            if stage.stage == ContextStageId::Snip && stage.provider_id == "emergency-test"
    )));
}

#[derive(Default)]
struct ContextTooLongThenOkModel {
    requests: Mutex<Vec<ModelRequest>>,
}

impl ContextTooLongThenOkModel {
    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for ContextTooLongThenOkModel {
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
            context_window: 100,
            max_output_tokens: 10,
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
        let mut requests = self.requests.lock().await;
        requests.push(req);
        if requests.len() == 1 {
            return Err(ModelError::ContextTooLong {
                tokens: 1_000,
                max: 100,
            });
        }
        Ok(Box::pin(stream::iter(text_events("after compact"))))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Default)]
struct OkRecordingModel {
    requests: Mutex<Vec<ModelRequest>>,
}

impl OkRecordingModel {
    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for OkRecordingModel {
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
            context_window: 100,
            max_output_tokens: 10,
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
        Ok(Box::pin(stream::iter(text_events("after compact"))))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct EmergencyTestProvider;

#[async_trait]
impl ContextProvider for EmergencyTestProvider {
    fn provider_id(&self) -> &str {
        "emergency-test"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Snip
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        if let Some(first) = ctx.active.history.first_mut() {
            first.parts = vec![MessagePart::Text("[EMERGENCY_COMPACTED]".to_owned())];
        }
        Ok(ContextOutcome::Modified { bytes_saved: 8 })
    }
}

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

fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
