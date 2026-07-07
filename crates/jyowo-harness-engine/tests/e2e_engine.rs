use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, Decision, DecisionScope, DeferPolicy, EndReason, Event,
    Message, MessageContent, MessageId, MessagePart, MessageRole, ModelError, NetworkAccess,
    OverflowAction, PermissionError, PermissionSubject, ProviderRestriction, RedactRules, Redactor,
    ResultBudget, RunId, SessionId, StopReason, TenantId, ToolActionPlan, ToolDescriptor,
    ToolError, ToolExecutionChannel, ToolGroup, ToolOrigin, ToolProperties, ToolResult,
    ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot, WorkspaceAccess,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
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
use tempfile::TempDir;
use tokio::sync::Mutex;

mod authorization_support;
use authorization_support::test_authorization_service;

const SECRET: &str = "sk-e2e-secret-value";

#[tokio::test]
async fn engine_e2e_reinjects_budgeted_tool_result_and_persists_redacted_events() {
    let harness = Harness::new().await;

    let events = harness.run("dump the secret").await.unwrap();
    let requests = harness.model.requests().await;
    let stored = harness.stored_events().await;

    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages.iter().any(|message| {
        message.role == MessageRole::Assistant
            && message.parts.iter().any(
                |part| matches!(part, MessagePart::ToolUse { name, .. } if name == "SecretDump"),
            )
    }));
    let injected = tool_result_debug(&requests[1]);
    assert!(injected.contains("blob_ref"));
    assert!(!injected.contains(SECRET));

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolResultOffloaded(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(completed)
            if completed.content == MessageContent::Text("final answer".to_owned())
    )));
    assert!(events.iter().any(|event| {
        matches!(event, Event::RunEnded(ended) if ended.reason == EndReason::Completed)
    }));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::RunEnded(_)))
            .count(),
        1
    );

    let persisted = serde_json::to_string(&stored).unwrap();
    assert!(!persisted.contains(SECRET));
    assert!(persisted.contains("[REDACTED]"));
}

struct Harness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    engine: Engine,
    store: Arc<InMemoryEventStore>,
    model: Arc<SequenceModel>,
}

impl Harness {
    async fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(SecretRedactor)));
        let model = Arc::new(SequenceModel::new(vec![
            tool_call_events("SecretDump", json!({ "seed": SECRET })),
            text_events("final answer"),
        ]));
        let blob_store = Arc::new(InMemoryBlobStore::default());
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(vec![Box::new(
                SecretDumpTool::new(),
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
            .with_engine_id(EngineId::new("m5-e2e-engine"))
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
            .with_blob_store(blob_store)
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
        Ok(self
            .engine
            .run(
                SessionHandle {
                    tenant_id: self.tenant_id,
                    session_id: self.session_id,
                },
                turn_input(text),
                RunContext::new(self.tenant_id, self.session_id, RunId::new()),
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

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace(SECRET, "[REDACTED]")
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
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let events = self.responses.lock().await.remove(0);
        Ok(Box::pin(stream::iter(events)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct SecretDumpTool {
    descriptor: ToolDescriptor,
}

impl SecretDumpTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "SecretDump".to_owned(),
                display_name: "SecretDump".to_owned(),
                description: "Returns secret test output.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
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
                required_capabilities: vec![],
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 8,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2,
                    preview_tail_chars: 2,
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
impl Tool for SecretDumpTool {
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
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let seed = authorized
            .raw_input()
            .get("seed")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text(format!("aa{seed}zz")),
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

fn tool_call_events(name: &str, input: Value) -> Vec<ModelStreamEvent> {
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
            message_id: "assistant-final".to_owned(),
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

fn tool_result_debug(request: &ModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            MessagePart::ToolResult { content, .. } => Some(format!("{content:?}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
