#![cfg(feature = "recall-memory")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    CapabilityRegistry, Decision, MemoryError, MemoryId, MemorySessionCtx, Message, MessageId,
    MessagePart, MessageRole, MessageView, ModelError, NoopRedactor, PermissionError, SessionId,
    SessionSummaryView, TenantId, TurnInput, UserMessageView,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_memory::{
    MemoryLifecycle, MemoryListScope, MemoryManager, MemoryQuery, MemoryRecord, MemoryStore,
    MemorySummary,
};
use harness_model::{
    ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor, ModelProtocol,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::ToolPool;

mod authorization_support;
use authorization_support::test_authorization_service;

#[tokio::test]
async fn engine_calls_memory_lifecycle_at_turn_start() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let provider = Arc::new(RecordingMemoryProvider::default());
    let manager = MemoryManager::new();
    manager.register_provider(provider.clone()).unwrap();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("memory-lifecycle-test"))
        .with_event_store(store.clone())
        .with_context(
            ContextEngine::builder()
                .with_memory_manager(Arc::new(manager))
                .build()
                .unwrap(),
        )
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(Arc::new(StopModel))
        .with_tools(ToolPool::default())
        .with_authorization_service(test_authorization_service(
            Arc::new(DenyBroker),
            store.clone(),
        ))
        .with_workspace_root(workspace.path())
        .with_model_id("stop")
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
            turn_input("remember this"),
            RunContext::new(tenant_id, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(provider.turn_start.load(Ordering::SeqCst), 1);
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::json!({}),
    }
}

#[derive(Default)]
struct RecordingMemoryProvider {
    turn_start: AtomicUsize,
}

#[async_trait]
impl MemoryStore for RecordingMemoryProvider {
    fn provider_id(&self) -> &'static str {
        "recording"
    }

    async fn recall(&self, _query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryLifecycle for RecordingMemoryProvider {
    async fn on_turn_start(
        &self,
        _turn: u32,
        _message: &UserMessageView<'_>,
    ) -> Result<(), MemoryError> {
        self.turn_start.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn initialize(&self, _ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn on_pre_compress(
        &self,
        _messages: &[MessageView<'_>],
    ) -> Result<Option<String>, MemoryError> {
        Ok(None)
    }

    async fn on_session_end(
        &self,
        _ctx: &MemorySessionCtx<'_>,
        _summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        Ok(())
    }
}

impl harness_memory::MemoryProvider for RecordingMemoryProvider {}

struct StopModel;

#[async_trait]
impl ModelProvider for StopModel {
    fn provider_id(&self) -> &'static str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "stop".to_owned(),
            display_name: "Stop".to_owned(),
            context_window: 8_192,
            max_output_tokens: 1_024,
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
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter([ModelStreamEvent::MessageStop])))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct DenyBroker;

#[async_trait]
impl PermissionBroker for DenyBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::DenyOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}
