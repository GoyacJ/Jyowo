use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BlobMeta, BlobRetention, BlobStore, BudgetMetric, CapabilityRegistry, Decision, DecisionScope,
    DeferPolicy, Event, Message, MessageId, MessagePart, MessageRole, ModelError, NoopRedactor,
    OverflowAction, PermissionError, PermissionSubject, ProviderRestriction, ResultBudget, RunId,
    SessionId, TenantId, ToolCapability, ToolDescriptor, ToolGroup, ToolOrigin, ToolProperties,
    ToolResult, ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::{InMemoryBlobStore, InMemoryEventStore};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::{
    ReadBlobTool, SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool, ToolPoolFilter,
    ToolPoolModelProfile, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

#[tokio::test]
async fn with_blob_store_installs_read_blob_capabilities_and_denies_unoffloaded_blob() {
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let blob_ref = blob_store
        .put(
            tenant_id,
            Bytes::from_static(b"stored body"),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: 11,
                content_hash: *blake3::hash(b"stored body").as_bytes(),
                created_at: harness_contracts::now(),
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .unwrap();
    let harness = Harness::new(
        tenant_id,
        session_id,
        vec![Box::new(ReadBlobTool::default())],
        ModelResponse::Sequence(vec![
            tool_call_events("ReadBlob", json!({ "blob_ref": blob_ref })),
            text_events("read done"),
        ]),
        Some(blob_store),
        None,
    )
    .await
    .unwrap();

    let events = harness.run("read blob").await.unwrap();
    let requests = harness.model.requests().await;

    let tool_result = tool_result_debug(&requests[1]);
    assert!(!tool_result.contains("stored body"));
    assert!(tool_result.contains("permission denied"));
    assert!(tool_result.contains("blob was not offloaded in current run"));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended) if ended.reason == harness_contracts::EndReason::Completed
    )));
}

#[tokio::test]
async fn build_fails_when_tool_required_capability_is_missing() {
    let error = match Harness::new(
        TenantId::SINGLE,
        SessionId::new(),
        vec![Box::new(ReadBlobTool::default())],
        ModelResponse::Events(text_events("unused")),
        None,
        None,
    )
    .await
    {
        Ok(_) => panic!("engine build unexpectedly succeeded"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("missing required capability"));
}

#[tokio::test]
async fn builder_capability_overrides_base_registry_and_supports_custom_capability() {
    let mut base = CapabilityRegistry::default();
    let base_cap: Arc<dyn TestCustomCap> = Arc::new(LabelCap("base"));
    base.install::<dyn TestCustomCap>(custom_capability(), base_cap);
    let override_cap: Arc<dyn TestCustomCap> = Arc::new(LabelCap("override"));
    let harness = Harness::new(
        TenantId::SINGLE,
        SessionId::new(),
        vec![Box::new(CustomCapTool::new())],
        ModelResponse::Sequence(vec![
            tool_call_events("CustomCap", json!({})),
            text_events("custom done"),
        ]),
        None,
        Some((Arc::new(base), override_cap)),
    )
    .await
    .unwrap();

    let events = harness.run("use custom cap").await.unwrap();
    let requests = harness.model.requests().await;

    assert!(tool_result_debug(&requests[1]).contains("override"));
    assert!(!tool_result_debug(&requests[1]).contains("base"));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended) if ended.reason == harness_contracts::EndReason::Completed
    )));
}

struct Harness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    engine: Engine,
    _store: Arc<InMemoryEventStore>,
    model: Arc<SequenceModel>,
}

impl Harness {
    async fn new(
        tenant_id: TenantId,
        session_id: SessionId,
        tools: Vec<Box<dyn Tool>>,
        response: ModelResponse,
        blob_store: Option<Arc<dyn BlobStore>>,
        custom_override: Option<(Arc<CapabilityRegistry>, Arc<dyn TestCustomCap>)>,
    ) -> Result<Self, harness_contracts::EngineError> {
        let workspace = tempfile::tempdir().unwrap();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(SequenceModel::new(response));
        let hooks = HookRegistry::builder().build().unwrap();
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(harness_tool::BuiltinToolset::Custom(tools))
            .build()
            .unwrap();
        let tool_pool = ToolPool::assemble(
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

        let mut builder = Engine::builder()
            .with_engine_id(EngineId::new("capability-test"))
            .with_event_store(store.clone())
            .with_context(ContextEngine::builder().build().unwrap())
            .with_hooks(HookDispatcher::new(hooks.snapshot()))
            .with_model(model.clone())
            .with_tools(tool_pool)
            .with_permission_broker(Arc::new(AllowBroker))
            .with_workspace_root(workspace.path())
            .with_model_id("test-model")
            .with_protocol(ModelProtocol::Messages);
        if let Some(blob_store) = blob_store {
            builder = builder.with_blob_store(blob_store);
        }
        if let Some((base, override_cap)) = custom_override {
            builder = builder
                .with_cap_registry(base)
                .with_capability::<dyn TestCustomCap>(custom_capability(), override_cap);
        }

        let engine = builder.build()?;
        Ok(Self {
            _workspace: workspace,
            tenant_id,
            session_id,
            engine,
            _store: store,
            model,
        })
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
}

struct SequenceModel {
    response: Mutex<ModelResponse>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl SequenceModel {
    fn new(response: ModelResponse) -> Self {
        Self {
            response: Mutex::new(response),
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
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let response = {
            let mut response = self.response.lock().await;
            match &mut *response {
                ModelResponse::Sequence(responses) => ModelResponse::Events(responses.remove(0)),
                ModelResponse::Events(events) => ModelResponse::Events(events.clone()),
            }
        };
        match response {
            ModelResponse::Events(events) => Ok(Box::pin(stream::iter(events))),
            ModelResponse::Sequence(_) => {
                unreachable!("sequence response is expanded before infer")
            }
        }
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Clone)]
enum ModelResponse {
    Events(Vec<ModelStreamEvent>),
    Sequence(Vec<Vec<ModelStreamEvent>>),
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

trait TestCustomCap: Send + Sync + 'static {
    fn label(&self) -> &'static str;
}

struct LabelCap(&'static str);

impl TestCustomCap for LabelCap {
    fn label(&self) -> &'static str {
        self.0
    }
}

struct CustomCapTool {
    descriptor: ToolDescriptor,
}

impl CustomCapTool {
    fn new() -> Self {
        Self {
            descriptor: descriptor(
                "CustomCap",
                vec![custom_capability()],
                ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 1_000,
                    on_overflow: OverflowAction::Reject,
                    preview_head_chars: 0,
                    preview_tail_chars: 0,
                },
            ),
        }
    }
}

#[async_trait]
impl Tool for CustomCapTool {
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
            scope: DecisionScope::ToolName(self.descriptor.name.clone()),
        }
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        let cap = ctx.capability::<dyn TestCustomCap>(custom_capability())?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text(cap.label().to_owned()),
        )])))
    }
}

fn descriptor(
    name: &str,
    required_capabilities: Vec<ToolCapability>,
    budget: ResultBudget,
) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: "test tool".to_owned(),
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
        required_capabilities,
        budget,
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
    }
}

fn custom_capability() -> ToolCapability {
    ToolCapability::Custom("custom_test_cap".to_owned())
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
            stop_reason: Some(harness_contracts::StopReason::ToolUse),
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
            stop_reason: Some(harness_contracts::StopReason::EndTurn),
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
