#![cfg(feature = "programmatic-tool-calling")]

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    CapabilityRegistry, Decision, EmbeddedRefusedReason, Message, MessageId, MessagePart,
    MessageRole, ModelError, NoopRedactor, PermissionError, RunId, StopReason, TenantId,
    ToolCapability, ToolResult, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineBuilder, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::{
    BuiltinToolset, SchemaResolverContext, ToolPool, ToolPoolFilter, ToolPoolModelProfile,
    ToolRegistry, ToolSearchMode,
};
use parking_lot::Mutex;

#[tokio::test]
async fn engine_default_ptc_pool_installs_execute_code_tool_and_caps() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter {
            allowlist: Some(HashSet::from(["execute_code".to_owned()])),
            ..ToolPoolFilter::default()
        },
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &SchemaResolverContext {
            run_id: RunId::new(),
            session_id: harness_contracts::SessionId::new(),
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap();

    let engine = Engine::builder()
        .with_required_test_dependencies()
        .with_tools(tools)
        .with_code_sandbox(Arc::new(harness_sandbox::MiniLuaCodeSandbox::new()))
        .build()
        .unwrap();

    assert!(engine.has_tool("execute_code"));
    assert!(engine.has_capability(&ToolCapability::CodeRuntime));
    assert!(engine.has_capability(&ToolCapability::EmbeddedToolDispatcher));
}

#[tokio::test]
async fn engine_execute_code_embedded_step_keeps_metadata_in_final_result() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let session_id = harness_contracts::SessionId::new();
    let run_id = RunId::new();
    let tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter {
            allowlist: Some(HashSet::from([
                "execute_code".to_owned(),
                "ListDir".to_owned(),
            ])),
            ..ToolPoolFilter::default()
        },
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &SchemaResolverContext {
            run_id,
            session_id,
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap();

    let model = Arc::new(ScriptedModel::new(vec![
        tool_call_events(
            "execute_code",
            serde_json::json!({
                "language": "mini_lua",
                "source": "return emb.tool(\"ListDir\", \"{\\\"path\\\":\\\"\\\"}\")"
            }),
        ),
        text_events("done"),
    ]));
    let workspace = tempfile::tempdir().unwrap();
    let engine = Engine::builder()
        .with_required_test_dependencies()
        .with_model(model)
        .with_tools(tools)
        .with_workspace_root(workspace.path())
        .with_code_sandbox(Arc::new(harness_sandbox::MiniLuaCodeSandbox::new()))
        .with_max_iterations(2)
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id,
            },
            turn_input("list current dir"),
            RunContext::new(TenantId::SINGLE, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            harness_contracts::Event::ExecuteCodeStepInvoked(step)
                if step.embedded_tool == "ListDir"
                    && step.step_seq == 1
                    && step.refused_reason.is_none()
                    && step.duration_ms > 0
        )
    }));
    assert!(events.iter().any(|event| {
        let harness_contracts::Event::ToolUseCompleted(completed) = event else {
            return false;
        };
        let ToolResult::Structured(value) = &completed.result else {
            return false;
        };
        value["embedded_steps"][0]["tool_name"] == "ListDir"
            && value["embedded_steps"][0]["duration_ms"]
                .as_u64()
                .unwrap_or_default()
                > 0
    }));
}

#[tokio::test]
async fn engine_execute_code_embedded_permission_denial_is_structured() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let session_id = harness_contracts::SessionId::new();
    let run_id = RunId::new();
    let tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter {
            allowlist: Some(HashSet::from([
                "execute_code".to_owned(),
                "Grep".to_owned(),
            ])),
            ..ToolPoolFilter::default()
        },
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &SchemaResolverContext {
            run_id,
            session_id,
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap();

    let model = Arc::new(ScriptedModel::new(vec![
        tool_call_events(
            "execute_code",
            serde_json::json!({
                "language": "mini_lua",
                "source": "return emb.tool(\"Grep\", \"{\\\"path\\\":\\\"\\\",\\\"pattern\\\":\\\"x\\\"}\")"
            }),
        ),
        text_events("done"),
    ]));
    let workspace = tempfile::tempdir().unwrap();
    let engine = Engine::builder()
        .with_required_test_dependencies()
        .with_model(model)
        .with_tools(tools)
        .with_permission_broker(Arc::new(DenyGrepBroker))
        .with_workspace_root(workspace.path())
        .with_code_sandbox(Arc::new(harness_sandbox::MiniLuaCodeSandbox::new()))
        .with_max_iterations(2)
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id,
            },
            turn_input("list current dir"),
            RunContext::new(TenantId::SINGLE, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            harness_contracts::Event::ExecuteCodeStepInvoked(step)
                if step.embedded_tool == "Grep"
                    && step.step_seq == 1
                    && step.refused_reason == Some(EmbeddedRefusedReason::PermissionDenied)
        )
    }));
}

#[tokio::test]
async fn engine_execute_code_embedded_capability_denial_is_structured() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let session_id = harness_contracts::SessionId::new();
    let run_id = RunId::new();
    let tools = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter {
            allowlist: Some(HashSet::from([
                "execute_code".to_owned(),
                "WebSearch".to_owned(),
            ])),
            ..ToolPoolFilter::default()
        },
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &SchemaResolverContext {
            run_id,
            session_id,
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap();

    let model = Arc::new(ScriptedModel::new(vec![
        tool_call_events(
            "execute_code",
            serde_json::json!({
                "language": "mini_lua",
                "source": "return emb.tool(\"WebSearch\", \"{\\\"query\\\":\\\"x\\\"}\")"
            }),
        ),
        text_events("done"),
    ]));
    let workspace = tempfile::tempdir().unwrap();
    let engine = Engine::builder()
        .with_required_test_dependencies()
        .with_model(model)
        .with_tools(tools)
        .with_workspace_root(workspace.path())
        .with_code_sandbox(Arc::new(harness_sandbox::MiniLuaCodeSandbox::new()))
        .with_max_iterations(2)
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id,
            },
            turn_input("search"),
            RunContext::new(TenantId::SINGLE, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            harness_contracts::Event::ExecuteCodeStepInvoked(step)
                if step.embedded_tool == "WebSearch"
                    && step.step_seq == 1
                    && step.refused_reason == Some(EmbeddedRefusedReason::CapabilityDenied)
        )
    }));
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

struct ScriptedModel {
    responses: Mutex<Vec<Vec<ModelStreamEvent>>>,
}

impl ScriptedModel {
    fn new(responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl ModelProvider for ScriptedModel {
    fn provider_id(&self) -> &'static str {
        "scripted"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "scripted".to_owned(),
            model_id: "dummy-model".to_owned(),
            display_name: "Scripted model".to_owned(),
            context_window: 1_000,
            max_output_tokens: 100,
            conversation_capability: ConversationModelCapability::default(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let events = self.responses.lock().remove(0);
        Ok(Box::pin(stream::iter(events)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct DummyBroker;

#[async_trait]
impl PermissionBroker for DummyBroker {
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

struct DenyGrepBroker;

#[async_trait]
impl PermissionBroker for DenyGrepBroker {
    async fn decide(&self, request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        match request.subject {
            harness_contracts::PermissionSubject::ToolInvocation { tool, .. } if tool == "Grep" => {
                Decision::DenyOnce
            }
            _ => Decision::AllowOnce,
        }
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
        metadata: serde_json::json!({}),
    }
}

fn tool_call_events(name: &str, input: serde_json::Value) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-tool".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: harness_contracts::ToolUseId::new(),
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
