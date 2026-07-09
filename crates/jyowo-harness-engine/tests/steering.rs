use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::NoopRedactor;
use harness_contracts::{
    CapabilityRegistry, Decision, Event, Message, MessageId, MessagePart, MessageRole, ModelError,
    PermissionError, RunId, SessionId, SteeringId, SteeringKind, SteeringMessageAppliedEvent,
    StopReason, TenantId, ToolSearchMode, TurnInput, UsageSnapshot,
};
use harness_engine::{
    Engine, EngineError, EngineRunner, RunContext, SessionHandle, SteeringDrain, SteeringMerge,
};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::{
    BuiltinToolset, SchemaResolverContext, ToolPool, ToolPoolFilter, ToolPoolModelProfile,
    ToolRegistry,
};
use tokio::sync::Mutex;

mod authorization_support;
use authorization_support::test_authorization_service;

#[tokio::test]
async fn steering_drain_runs_before_model_infer_and_merges_prompt() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let model = Arc::new(RecordingModelProvider::default());
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
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
    let steering = Arc::new(OneShotSteeringDrain::new("include blockers"));
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let engine = Engine::builder()
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model.clone())
        .with_tools(tools)
        .with_authorization_service(test_authorization_service(
            Arc::new(AllowBroker),
            store.clone(),
        ))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .with_steering_drain(steering.clone())
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("summarize audit"),
            RunContext::new(tenant_id, session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let request_text = model
        .requests()
        .await
        .first()
        .expect("model should receive request")
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(request_text.contains("summarize audit"));
    assert!(request_text.contains("include blockers"));
    assert_eq!(steering.call_count(), 1);
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SteeringMessageApplied(_))));
}

#[derive(Default)]
struct RecordingModelProvider {
    requests: Mutex<Vec<ModelRequest>>,
}

impl RecordingModelProvider {
    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for RecordingModelProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
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
        Ok(Box::pin(stream::iter([
            ModelStreamEvent::MessageStart {
                message_id: "assistant-1".to_owned(),
                usage: UsageSnapshot::default(),
            },
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("ok".to_owned()),
            },
            ModelStreamEvent::MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                usage_delta: UsageSnapshot::default(),
            },
            ModelStreamEvent::MessageStop,
        ])))
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

struct OneShotSteeringDrain {
    body: &'static str,
    calls: AtomicUsize,
}

impl OneShotSteeringDrain {
    fn new(body: &'static str) -> Self {
        Self {
            body,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SteeringDrain for OneShotSteeringDrain {
    async fn drain_and_merge(
        &self,
        session: &SessionHandle,
        run_id: RunId,
        merged_into_message_id: MessageId,
    ) -> Result<Option<SteeringMerge>, EngineError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            return Ok(None);
        }
        let mut kind_distribution = BTreeMap::new();
        kind_distribution.insert(SteeringKind::Append, 1);
        Ok(Some(SteeringMerge {
            body: self.body.to_owned(),
            applied_event: Event::SteeringMessageApplied(SteeringMessageAppliedEvent {
                ids: vec![SteeringId::new()],
                session_id: session.session_id,
                run_id,
                merged_into_message_id: Some(merged_into_message_id),
                kind_distribution,
                at: harness_contracts::now(),
            }),
            already_persisted: false,
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
        metadata: serde_json::Value::Null,
    }
}
