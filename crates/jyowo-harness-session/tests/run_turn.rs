use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::{future, time::Duration};

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, DecidedBy, Decision, DecisionScope, DeferPolicy, Event,
    HookEventKind, Message, MessagePart, MessageRole, ModelError, NoopRedactor, OverflowAction,
    PermissionError, PermissionSubject, ProviderRestriction, ResultBudget, RunId, SessionId,
    StopReason, TenantId, ToolDescriptor, ToolGroup, ToolOrigin, ToolProperties, ToolResult,
    ToolUseId, TrustLevel, UsageSnapshot,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookOutcome, HookRegistry,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    ContentDelta, ConversationModelCapability, ErrorClass, ErrorHints, HealthStatus, InferContext,
    ModelDescriptor, ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_session::{Session, SessionOptions, SessionTurnRuntime};
use harness_tool::{
    BuiltinToolset, SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool, ToolPoolFilter,
    ToolPoolModelProfile, ToolRegistry, ToolSearchMode, ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

#[tokio::test]
async fn run_turn_executes_list_dir_with_formal_runtime() {
    let harness = TestHarness::new(tool_call_events("ListDir", json!({ "path": "" }))).await;
    std::fs::write(harness.workspace.path().join("marker.txt"), "m3").unwrap();
    harness
        .model
        .replace_events(tool_call_events(
            "ListDir",
            json!({ "path": harness.workspace.path() }),
        ))
        .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let projection = harness.session.projection().await;
    let assistant = projection
        .messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Assistant)
        .map(message_text)
        .unwrap_or_default();
    assert!(!assistant.contains("marker.txt"));
    assert!(assistant.contains("Tool result withheld from conversation transcript."));
    assert_eq!(harness.user_prompt_hooks.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn run_turn_records_run_tool_permission_assistant_events() {
    let harness = TestHarness::new(tool_call_events("ListDir", json!({ "path": "" }))).await;
    harness
        .model
        .replace_events(tool_call_events(
            "ListDir",
            json!({ "path": harness.workspace.path() }),
        ))
        .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let events = harness.events().await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunStarted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::UserMessageAppended(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseRequested(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::PermissionResolved(resolved)
            if matches!(resolved.decided_by, DecidedBy::Broker { .. }))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseApproved(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::AssistantMessageCompleted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunEnded(_))));
}

#[tokio::test]
async fn run_turn_keeps_session_open_after_completed_run() {
    let harness = TestHarness::new(text_events("ok")).await;

    harness.session.run_turn("hello").await.unwrap();

    assert_eq!(harness.session.projection().await.end_reason, None);
}

#[tokio::test]
async fn run_turn_keeps_thinking_out_of_durable_events() {
    let harness =
        TestHarness::new(thinking_then_text_events("internal chain", "final answer")).await;

    harness.session.run_turn("hello").await.unwrap();

    let events = harness.events().await;
    assert!(!events.iter().any(|event| {
        matches!(
            event,
            Event::AssistantDeltaProduced(delta)
                if matches!(&delta.delta, harness_contracts::DeltaChunk::Thought(thought)
                    if thought.text.as_deref() == Some("internal chain"))
        )
    }));
    let completed = events
        .iter()
        .find_map(|event| match event {
            Event::AssistantMessageCompleted(completed) => Some(completed),
            _ => None,
        })
        .expect("completed assistant message should be emitted");

    assert_eq!(
        completed.content,
        harness_contracts::MessageContent::Text("final answer".to_owned())
    );
}

#[tokio::test]
async fn run_turn_records_run_end_on_model_infer_error() {
    let harness = TestHarness::new(text_events("unused")).await;
    harness
        .model
        .replace_response(ModelResponse::Error(ModelError::ProviderUnavailable(
            "offline".to_owned(),
        )))
        .await;

    let error = harness.session.run_turn("hello").await.unwrap_err();

    assert!(error.to_string().contains("offline"));
    assert!(harness
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::RunEnded(ended)
            if matches!(&ended.reason, harness_contracts::EndReason::Error(message)
                if message.contains("offline")))));
}

#[tokio::test]
async fn run_turn_records_run_end_on_model_stream_error() {
    let harness = TestHarness::new(vec![ModelStreamEvent::StreamError {
        error: ModelError::UnexpectedResponse("bad chunk".to_owned()),
        class: ErrorClass::Fatal,
        hints: ErrorHints::default(),
    }])
    .await;

    let error = harness.session.run_turn("hello").await.unwrap_err();

    assert!(error.to_string().contains("bad chunk"));
    assert!(harness
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::RunEnded(ended)
            if matches!(&ended.reason, harness_contracts::EndReason::Error(message)
                if message.contains("bad chunk")))));
}

#[tokio::test]
async fn run_turn_finalizes_when_model_stream_message_stop_arrives_without_eof() {
    let harness = TestHarness::new(text_events("unused")).await;
    harness
        .model
        .replace_response(ModelResponse::EventsThenPending(text_events(
            "complete answer",
        )))
        .await;

    let result = tokio::time::timeout(
        Duration::from_millis(200),
        harness.session.run_turn("hello"),
    )
    .await
    .expect("run should finalize after MessageStop without waiting for stream EOF");

    result.unwrap();

    let events = harness.events().await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessageCompleted(completed)
                if completed.content
                    == harness_contracts::MessageContent::Text("complete answer".to_owned())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::RunEnded(ended)
                if matches!(ended.reason, harness_contracts::EndReason::Completed)
        )
    }));
}

#[tokio::test]
async fn run_turn_rejects_missing_runtime() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .build()
        .await
        .unwrap();

    let error = session.run_turn("hello").await.unwrap_err();

    assert!(error.to_string().contains("turn runtime missing"));
}

#[cfg(feature = "steering")]
#[tokio::test]
async fn run_turn_drains_steering_before_model_infer() {
    use harness_contracts::{SteeringBody, SteeringKind, SteeringSource};
    use harness_session::SteeringRequest;

    let harness = TestHarness::new(text_events("ok")).await;
    harness
        .session
        .push_steering(SteeringRequest {
            kind: SteeringKind::Append,
            body: SteeringBody::Text("include hidden files".to_owned()),
            priority: None,
            correlation_id: None,
            source: SteeringSource::User,
        })
        .await
        .unwrap();

    harness.session.run_turn("list current dir").await.unwrap();

    let requests = harness.model.requests().await;
    let user_text = requests[0]
        .messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
        .map(message_text)
        .unwrap_or_default();
    assert!(user_text.contains("list current dir"));
    assert!(user_text.contains("include hidden files"));
    assert!(harness
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::SteeringMessageApplied(_))));
}

struct TestHarness {
    workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    store: Arc<InMemoryEventStore>,
    session: Session,
    model: Arc<RecordingModelProvider>,
    user_prompt_hooks: Arc<AtomicUsize>,
}

impl TestHarness {
    async fn new(events: Vec<ModelStreamEvent>) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(RecordingModelProvider::new(events));
        let user_prompt_hooks = Arc::new(AtomicUsize::new(0));
        let hooks = HookRegistry::builder()
            .with_hook(Box::new(CountingHook {
                calls: user_prompt_hooks.clone(),
            }))
            .build()
            .unwrap();
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Custom(vec![Box::new(
                TestListDirTool::new(),
            )]))
            .build()
            .unwrap();
        let tools = ToolPool::assemble(
            &registry.snapshot(),
            &ToolPoolFilter::default(),
            &ToolSearchMode::Disabled,
            &ToolPoolModelProfile {
                provider: harness_contracts::ModelProvider("mock".to_owned()),
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
        let runtime = SessionTurnRuntime {
            context: ContextEngine::builder().build().unwrap(),
            hooks: HookDispatcher::new(hooks.snapshot()),
            model: model.clone(),
            tools,
            permission_broker: Arc::new(AllowBroker),
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            blob_store: None,
            model_id: "mock-model".to_owned(),
            model_extra: serde_json::Value::Null,
            protocol: ModelProtocol::Messages,
            system_prompt: Some("system".to_owned()),
        };
        let session = Session::builder()
            .with_options(
                SessionOptions::new(workspace.path())
                    .with_tenant_id(tenant_id)
                    .with_session_id(session_id),
            )
            .with_event_store(store.clone())
            .with_turn_runtime(runtime)
            .build()
            .await
            .unwrap();

        Self {
            workspace,
            tenant_id,
            session_id,
            store,
            session,
            model,
            user_prompt_hooks,
        }
    }

    async fn events(&self) -> Vec<Event> {
        self.store
            .read_envelopes(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .map(|envelope| envelope.payload)
            .collect()
            .await
    }
}

struct RecordingModelProvider {
    response: Mutex<ModelResponse>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl RecordingModelProvider {
    fn new(events: Vec<ModelStreamEvent>) -> Self {
        Self {
            response: Mutex::new(ModelResponse::Events(events)),
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn replace_events(&self, events: Vec<ModelStreamEvent>) {
        self.replace_response(ModelResponse::Events(events)).await;
    }

    async fn replace_response(&self, response: ModelResponse) {
        *self.response.lock().await = response;
    }

    #[cfg(feature = "steering")]
    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for RecordingModelProvider {
    fn provider_id(&self) -> &'static str {
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
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        match self.response.lock().await.clone() {
            ModelResponse::Events(events) => Ok(Box::pin(stream::iter(events))),
            ModelResponse::EventsThenPending(events) => Ok(Box::pin(stream::unfold(
                events.into_iter(),
                |mut events| async move {
                    if let Some(event) = events.next() {
                        Some((event, events))
                    } else {
                        future::pending().await
                    }
                },
            ))),
            ModelResponse::Error(error) => Err(error),
        }
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Clone)]
enum ModelResponse {
    Events(Vec<ModelStreamEvent>),
    EventsThenPending(Vec<ModelStreamEvent>),
    Error(ModelError),
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

struct CountingHook {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl HookHandler for CountingHook {
    fn handler_id(&self) -> &'static str {
        "count-user-prompt"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(HookOutcome::Continue)
    }
}

struct TestListDirTool {
    descriptor: ToolDescriptor,
}

impl TestListDirTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "ListDir".to_owned(),
                display_name: "List directory".to_owned(),
                description: "List workspace directory entries.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::FileSystem,
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": { "path": { "type": "string" } }
                }),
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
            },
        }
    }
}

#[async_trait]
impl Tool for TestListDirTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if input.get("path").and_then(Value::as_str).is_none() {
            return Err(ValidationError::from("path is required"));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        input: &Value,
        _ctx: &ToolContext,
    ) -> harness_permission::PermissionCheck {
        harness_permission::PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: "ListDir".to_owned(),
                input: input.clone(),
            },
            scope: DecisionScope::PathPrefix(input["path"].as_str().unwrap_or_default().into()),
        }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        let path = input["path"].as_str().unwrap_or_default();
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)
            .map_err(|error| harness_contracts::ToolError::Message(error.to_string()))?
        {
            let entry =
                entry.map_err(|error| harness_contracts::ToolError::Message(error.to_string()))?;
            entries.push(entry.file_name().to_string_lossy().into_owned());
        }
        entries.sort();
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!(entries)),
        )])))
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
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
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

fn thinking_then_text_events(thinking: &str, text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(harness_model::ThinkingDelta {
                provider_native: None,
                signature: None,
                text: Some(thinking.to_owned()),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
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
