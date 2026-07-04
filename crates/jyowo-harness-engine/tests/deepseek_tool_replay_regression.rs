use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::{
    CompactHint, ContextBuffer, ContextEngine, ContextOutcome, ContextProvider, TokenBudget,
};
use harness_contracts::{
    CapabilityRegistry, ContextError, ContextStageId, Decision, DeferPolicy, Event, Message,
    MessageContent, MessageId, MessagePart, MessageRole, ModelError, ModelProtocol, NetworkAccess,
    NoopRedactor, PermissionError, ProviderRestriction, RunId, RunModelSnapshot, SessionId,
    StopReason, TenantId, ToolActionPlan, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
    WorkspaceAccess,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelLifecycle, ModelProvider, ModelRequest, ModelRuntimeSemantics, ModelStream,
    ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_provider_state::{
    FileProviderContinuationStore, ProviderContinuationKind, ProviderContinuationQuery,
    ProviderContinuationRecord, ProviderContinuationScope, ProviderContinuationStore,
};
use harness_tool::{
    action_plan_from_permission_check, default_result_budget, AuthorizedToolInput, BuiltinToolset,
    SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool, ToolPoolFilter,
    ToolPoolModelProfile, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

const MODEL_CONFIG_ID: &str = "deepseek-regression-config";
const PRIVATE_SENTINEL: &str = "PRIVATE_DEEPSEEK_TOOL_REPLAY_SENTINEL";
const DEEPSEEK_CONTINUATION_DIALECT: &str = "openai_chat.deepseek";

mod authorization_support;

use authorization_support::test_authorization_service;

mod deepseek_tool_replay {
    use super::*;

    #[tokio::test]
    async fn deepseek_tool_replay_uses_private_continuation_after_tool_result() {
        let harness = DeepSeekReplayHarness::new(
            ContextEngine::builder().build().unwrap(),
            vec![
                deepseek_tool_call_events(),
                text_events("answer after tool"),
            ],
        )
        .await;

        let events = harness.run("use the lookup tool").await;

        assert_completed(&events);
        assert_eq!(harness.tool_calls.load(Ordering::SeqCst), 1);
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::ToolUseRequested(_))));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::ToolUseCompleted(_))));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessageCompleted(completed)
                    if completed
                        .tool_uses
                        .iter()
                        .any(|tool| tool.tool_name == "lookup")
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessageCompleted(completed)
                    if message_content_contains(&completed.content, "answer after tool")
            )
        }));

        let requests = harness.model.requests().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[0].provider_context.continuations.is_empty());
        assert_eq!(requests[1].provider_context.continuations.len(), 1);
        assert_eq!(
            requests[1].provider_context.continuations[0].kind,
            ProviderContinuationKind::ReasoningReplay
        );
        assert_eq!(
            requests[1].provider_context.continuations[0].payload["private"],
            PRIVATE_SENTINEL
        );

        let tool_assistant_ids = assistant_tool_message_ids(&events);
        assert_eq!(tool_assistant_ids.len(), 1);
        let records = harness.load_records(tool_assistant_ids).await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ProviderContinuationKind::ReasoningReplay);
        assert_eq!(records[0].payload["private"], PRIVATE_SENTINEL);

        let public_events = serde_json::to_string(&events).unwrap();
        assert!(!public_events.contains(PRIVATE_SENTINEL));
    }

    #[tokio::test]
    async fn compacted_out_assistant_message_does_not_transfer_deepseek_continuation() {
        let harness = DeepSeekReplayHarness::new(
            ContextEngine::builder()
                .with_budget(TokenBudget {
                    max_tokens_per_turn: 10,
                    soft_budget_ratio: 0.5,
                    hard_budget_ratio: 0.95,
                    ..TokenBudget::default()
                })
                .with_provider(RemoveFirstMessageProvider)
                .build()
                .unwrap(),
            vec![text_events("done")],
        )
        .await;
        let compacted_out_id = MessageId::new();
        let kept_id = MessageId::new();
        harness
            .append_records(vec![
                harness.continuation_record(
                    compacted_out_id,
                    json!({"private": "compacted-out-continuation"}),
                ),
                harness.continuation_record(kept_id, json!({"private": "kept-continuation"})),
            ])
            .await;

        let events = harness
            .run_with_seed(
                "continue",
                vec![
                    assistant_tool_message(compacted_out_id),
                    assistant_tool_message(kept_id),
                ],
            )
            .await;

        assert_completed(&events);
        let requests = harness.model.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].provider_context.continuations.len(), 1);
        assert_eq!(
            requests[0].provider_context.continuations[0].message_id,
            kept_id
        );
        assert_eq!(
            requests[0].provider_context.continuations[0].payload["private"],
            "kept-continuation"
        );
        assert!(!requests[0]
            .provider_context
            .continuations
            .iter()
            .any(|record| record.message_id == compacted_out_id));
    }
}

struct DeepSeekReplayHarness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    store: Arc<FileProviderContinuationStore>,
    engine: Engine,
    model: Arc<ScriptedDeepSeekProvider>,
    tool_calls: Arc<AtomicUsize>,
}

impl DeepSeekReplayHarness {
    async fn new(context: ContextEngine, responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let store = Arc::new(FileProviderContinuationStore::open(workspace.path()).unwrap());
        let model = Arc::new(ScriptedDeepSeekProvider::new(responses));
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let tools = tool_pool(
            tenant_id,
            session_id,
            Box::new(CountingLookupTool::new(Arc::clone(&tool_calls))),
            "deepseek",
        )
        .await;
        let engine = Engine::builder()
            .with_engine_id(EngineId::new("deepseek-tool-replay-regression"))
            .with_event_store(event_store.clone())
            .with_context(context)
            .with_hooks(HookDispatcher::new(
                HookRegistry::builder().build().unwrap().snapshot(),
            ))
            .with_model(model.clone())
            .with_model_snapshot(model_snapshot())
            .with_tools(tools)
            .with_authorization_service(test_authorization_service(
                Arc::new(AllowBroker),
                event_store.clone(),
            ))
            .with_workspace_root(workspace.path())
            .with_model_id("deepseek-chat")
            .with_protocol(ModelProtocol::ChatCompletions)
            .with_provider_continuation_store(store.clone())
            .with_cap_registry(Arc::new(CapabilityRegistry::default()))
            .build()
            .unwrap();

        Self {
            _workspace: workspace,
            tenant_id,
            session_id,
            store,
            engine,
            model,
            tool_calls,
        }
    }

    async fn run(&self, text: &str) -> Vec<Event> {
        self.run_with_seed(text, Vec::new()).await
    }

    async fn run_with_seed(&self, text: &str, seed: Vec<Message>) -> Vec<Event> {
        self.engine
            .run(
                SessionHandle {
                    tenant_id: self.tenant_id,
                    session_id: self.session_id,
                },
                turn_input(text),
                self.run_context(seed),
            )
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
    }

    fn run_context(&self, seed: Vec<Message>) -> RunContext {
        RunContext::new(self.tenant_id, self.session_id, RunId::new())
            .with_context_seed(seed)
            .with_model_snapshot(RunModelSnapshot {
                model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
                provider_id: "deepseek".to_owned(),
                model_id: "deepseek-chat".to_owned(),
                display_name: "DeepSeek Chat".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                context_window: 8_000,
                max_output_tokens: 1_000,
                conversation_capability: ConversationModelCapability::default(),
            })
    }

    async fn append_records(&self, records: Vec<ProviderContinuationRecord>) {
        self.store.append_batch(records).await.unwrap();
    }

    fn continuation_record(
        &self,
        message_id: MessageId,
        payload: Value,
    ) -> ProviderContinuationRecord {
        ProviderContinuationRecord {
            provider_id: "deepseek".to_owned(),
            model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
            protocol: ModelProtocol::ChatCompletions,
            dialect: DEEPSEEK_CONTINUATION_DIALECT.to_owned(),
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            producing_run_id: RunId::new(),
            message_id,
            scope: ProviderContinuationScope::Conversation,
            kind: ProviderContinuationKind::ReasoningReplay,
            payload,
            created_at: harness_contracts::now(),
        }
    }

    async fn load_records(&self, message_ids: Vec<MessageId>) -> Vec<ProviderContinuationRecord> {
        self.store
            .load_for_messages(ProviderContinuationQuery {
                provider_id: "deepseek".to_owned(),
                model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
                protocol: ModelProtocol::ChatCompletions,
                dialect: DEEPSEEK_CONTINUATION_DIALECT.to_owned(),
                tenant_id: self.tenant_id,
                session_id: self.session_id,
                message_ids,
                kinds: vec![ProviderContinuationKind::ReasoningReplay],
            })
            .await
            .unwrap()
    }
}

struct ScriptedDeepSeekProvider {
    requests: Mutex<Vec<ModelRequest>>,
    responses: Mutex<VecDeque<Vec<ModelStreamEvent>>>,
}

impl ScriptedDeepSeekProvider {
    fn new(responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }

    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for ScriptedDeepSeekProvider {
    fn provider_id(&self) -> &str {
        "deepseek"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: "deepseek".to_owned(),
            model_id: "deepseek-chat".to_owned(),
            display_name: "DeepSeek Chat".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            context_window: 8_000,
            max_output_tokens: 1_000,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: ModelRuntimeSemantics::openai_chat_deepseek(),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let response = self
            .responses
            .lock()
            .await
            .pop_front()
            .expect("scripted response");
        Ok(Box::pin(stream::iter(response)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct CountingLookupTool {
    descriptor: ToolDescriptor,
    calls: Arc<AtomicUsize>,
}

impl CountingLookupTool {
    fn new(calls: Arc<AtomicUsize>) -> Self {
        Self {
            descriptor: lookup_descriptor(),
            calls,
        }
    }
}

#[async_trait]
impl Tool for CountingLookupTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(&self, _ctx: &SchemaResolverContext) -> Result<Value, ToolError> {
        Ok(self.descriptor.input_schema.clone())
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("lookup result".to_owned()),
        )])))
    }
}

struct RemoveFirstMessageProvider;

#[async_trait]
impl ContextProvider for RemoveFirstMessageProvider {
    fn provider_id(&self) -> &str {
        "remove-first-message"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Snip
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        if !ctx.active.history.is_empty() {
            ctx.active.history.remove(0);
        }
        Ok(ContextOutcome::Modified { bytes_saved: 10 })
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

async fn tool_pool(
    tenant_id: TenantId,
    session_id: SessionId,
    tool: Box<dyn Tool>,
    provider_id: &str,
) -> ToolPool {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Custom(vec![tool]))
        .build()
        .unwrap();
    ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile {
            provider: harness_contracts::ModelProvider(provider_id.to_owned()),
            max_context_tokens: Some(8_000),
        },
        &SchemaResolverContext {
            run_id: RunId::new(),
            session_id,
            tenant_id,
        },
    )
    .await
    .unwrap()
}

fn model_snapshot() -> harness_model::ModelRuntimeSnapshot {
    harness_model::ModelRuntimeSnapshot {
        provider_id: "deepseek".to_owned(),
        model_id: "deepseek-chat".to_owned(),
        display_name: "DeepSeek Chat".to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window: 8_000,
        max_output_tokens: 1_000,
        conversation_capability: ConversationModelCapability::default(),
        runtime_semantics: ModelRuntimeSemantics::openai_chat_deepseek(),
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}

fn deepseek_tool_call_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "deepseek-assistant-tool".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({ "private": PRIVATE_SENTINEL }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "lookup".to_owned(),
                input: json!({ "query": "jyowo" }),
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
            message_id: "deepseek-assistant-final".to_owned(),
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

fn assistant_tool_message(message_id: MessageId) -> Message {
    Message {
        id: message_id,
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: ToolUseId::new(),
            name: "lookup".to_owned(),
            input: json!({ "query": "jyowo" }),
        }],
        created_at: harness_contracts::now(),
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

fn lookup_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "lookup".to_owned(),
        display_name: "Lookup".to_owned(),
        description: "Return a fixed lookup result.".to_owned(),
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
        required_capabilities: Vec::new(),
        budget: default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
        service_binding: None,
    }
}

fn assert_completed(events: &[Event]) {
    assert!(events.iter().any(
        |event| matches!(event, Event::RunEnded(ended) if ended.reason == harness_contracts::EndReason::Completed)
    ));
}

fn assistant_tool_message_ids(events: &[Event]) -> Vec<MessageId> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessageCompleted(completed) if !completed.tool_uses.is_empty() => {
                Some(completed.message_id)
            }
            _ => None,
        })
        .collect()
}

fn message_content_contains(content: &MessageContent, needle: &str) -> bool {
    match content {
        MessageContent::Text(text) => text.contains(needle),
        MessageContent::Multimodal(parts) => parts
            .iter()
            .any(|part| matches!(part, MessagePart::Text(text) if text.contains(needle))),
        MessageContent::Structured(value) => value.to_string().contains(needle),
    }
}
