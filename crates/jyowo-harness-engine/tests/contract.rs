use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    CapabilityRegistry, Decision, Message, MessageId, MessagePart, MessageRole, ModelError,
    NoopRedactor, PermissionError, RunId, TenantId, TurnInput,
};
use harness_engine::{
    Engine, EngineBuilder, EngineId, EngineRunner, LoopState, RunContext, SessionHandle,
};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ConversationModelCapability, HealthStatus, InferContext, InferMiddleware, ModelDescriptor,
    ModelProvider, ModelRequest, ModelStream,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::ToolPool;
use parking_lot::Mutex;
use serde_json::json;

#[test]
fn engine_builder_exposes_stable_engine_id() {
    let engine = Engine::builder()
        .with_engine_id(EngineId::new("contract-engine"))
        .with_required_test_dependencies()
        .build()
        .unwrap();

    assert_eq!(engine.engine_id(), EngineId::new("contract-engine"));
}

#[tokio::test]
async fn engine_runner_is_object_safe_and_uses_engine_id() {
    let runner: Arc<dyn EngineRunner> = Arc::new(
        EngineBuilder::default()
            .with_engine_id(EngineId::new("runner-engine"))
            .with_required_test_dependencies()
            .build()
            .unwrap(),
    );

    assert_eq!(runner.engine_id(), EngineId::new("runner-engine"));
}

#[test]
fn engine_builder_rejects_unknown_model_without_snapshot() {
    let error = match EngineBuilder::default()
        .with_required_test_dependencies()
        .with_model_id("unknown-model")
        .build()
    {
        Ok(_) => panic!("unknown model should fail closed"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("unsupported model id for provider dummy: unknown-model"));
}

#[test]
fn loop_state_exposes_m5_five_state_contract() {
    let tool_call = harness_tool::ToolCall {
        tool_use_id: harness_contracts::ToolUseId::new(),
        tool_name: "contract-tool".to_owned(),
        input: json!({}),
    };

    let states = [
        LoopState::AwaitingModel,
        LoopState::ProcessingToolUses {
            pending: vec![tool_call],
        },
        LoopState::ApplyingHookResults,
        LoopState::MergingContext,
        LoopState::Ended(harness_contracts::StopReason::EndTurn),
    ];

    assert_eq!(states.len(), 5);
}

#[tokio::test]
async fn engine_builder_injects_model_middlewares_into_turn() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let middleware: Arc<dyn InferMiddleware> = Arc::new(RecordingMiddleware {
        label: "engine",
        calls: Arc::clone(&calls),
    });
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();
    let workspace = tempfile::tempdir().unwrap();
    let engine = EngineBuilder::default()
        .with_engine_id(EngineId::new("middleware-engine"))
        .with_required_test_dependencies()
        .with_workspace_root(workspace.path())
        .with_model_middleware(middleware)
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("middleware check"),
            RunContext::new(tenant_id, session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(events
        .iter()
        .any(|event| matches!(event, harness_contracts::Event::RunEnded(_))));
    assert_eq!(
        *calls.lock(),
        vec!["before:engine".to_owned(), "end:engine".to_owned()]
    );
}

#[cfg(feature = "programmatic-tool-calling")]
#[test]
fn engine_builder_installs_code_runtime_capability_from_code_sandbox() {
    let engine = Engine::builder()
        .with_required_test_dependencies()
        .with_code_sandbox(Arc::new(harness_sandbox::MiniLuaCodeSandbox::new()))
        .build()
        .unwrap();

    assert!(engine.has_capability(&harness_contracts::ToolCapability::CodeRuntime));
}

trait EngineBuilderTestExt {
    fn with_required_test_dependencies(self) -> Self;
}

impl EngineBuilderTestExt for EngineBuilder {
    fn with_required_test_dependencies(self) -> Self {
        let root = tempfile::tempdir().unwrap();
        self.with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_context(ContextEngine::builder().build().unwrap())
            .with_hooks(HookDispatcher::new(
                HookRegistry::builder().build().unwrap().snapshot(),
            ))
            .with_model(Arc::new(DummyModel))
            .with_tools(ToolPool::default())
            .with_permission_broker(Arc::new(DummyBroker))
            .with_workspace_root(root.path())
            .with_model_id("dummy-model")
            .with_cap_registry(Arc::new(CapabilityRegistry::default()))
    }
}

struct DummyModel;

#[async_trait]
impl ModelProvider for DummyModel {
    fn provider_id(&self) -> &'static str {
        "dummy"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "dummy".to_owned(),
            model_id: "dummy-model".to_owned(),
            display_name: "Dummy model".to_owned(),
            context_window: 1_000,
            max_output_tokens: 100,
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
        Ok(Box::pin(stream::empty()))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct DummyBroker;

#[async_trait]
impl PermissionBroker for DummyBroker {
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

struct RecordingMiddleware {
    label: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl InferMiddleware for RecordingMiddleware {
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
