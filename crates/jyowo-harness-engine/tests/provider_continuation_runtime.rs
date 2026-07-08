use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::{
    CompactHint, ContextBuffer, ContextEngine, ContextOutcome, ContextProvider, TokenBudget,
};
use harness_contracts::NoopRedactor;
use harness_contracts::{
    CapabilityRegistry, ContextError, ContextStageId, Decision, Event, Message, MessageId,
    MessagePart, MessageRole, ModelError, ModelProtocol, PermissionError, RunId, RunModelSnapshot,
    SessionId, StopReason, TenantId, TurnInput, UsageSnapshot,
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
    FileProviderContinuationStore, ProviderContinuationKind, ProviderContinuationRecord,
    ProviderContinuationScope, ProviderContinuationStore,
};
use harness_tool::ToolPool;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::Mutex;

const SAFE_MISSING_CONTINUATION_ERROR: &str =
    "provider continuation required for assistant tool replay but missing";
const PRIVATE_SENTINEL: &str = "PRIVATE_PROVIDER_CONTINUATION_SENTINEL";
const MODEL_CONFIG_ID: &str = "deepseek-config";
const DEEPSEEK_CONTINUATION_DIALECT: &str = "openai_chat.deepseek";

mod authorization_support;

use authorization_support::test_authorization_service;

mod provider_continuation {
    use super::*;

    #[tokio::test]
    async fn engine_loads_continuations_by_final_assembled_message_ids() {
        let harness = ProviderContinuationHarness::new(
            ContextEngine::builder().build().unwrap(),
            RecordingModel::events(text_events("done")),
            true,
        )
        .await;
        let assistant_id = MessageId::new();
        harness
            .append_records(vec![continuation_record(
                harness.tenant_id,
                harness.session_id,
                assistant_id,
                json!({"private": PRIVATE_SENTINEL}),
            )])
            .await;

        let events = harness
            .run_with_seed(vec![assistant_tool_message(assistant_id)])
            .await;

        assert!(completed(&events));
        let requests = harness.model.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].provider_context.provider_id, "deepseek");
        assert_eq!(
            requests[0].provider_context.model_config_id.as_deref(),
            Some(MODEL_CONFIG_ID)
        );
        assert_eq!(
            requests[0].provider_context.dialect.as_deref(),
            Some(DEEPSEEK_CONTINUATION_DIALECT)
        );
        assert_eq!(requests[0].provider_context.continuations.len(), 1);
        assert_eq!(
            requests[0].provider_context.continuations[0].message_id,
            assistant_id
        );
        assert_eq!(
            requests[0].provider_context.continuations[0].payload["private"],
            PRIVATE_SENTINEL
        );
    }

    #[tokio::test]
    async fn engine_does_not_load_continuation_for_compacted_out_message() {
        let context = ContextEngine::builder()
            .with_budget(TokenBudget {
                max_tokens_per_turn: 10,
                soft_budget_ratio: 0.5,
                hard_budget_ratio: 0.95,
                ..TokenBudget::default()
            })
            .with_provider(RemoveFirstMessageProvider)
            .build()
            .unwrap();
        let harness = ProviderContinuationHarness::new(
            context,
            RecordingModel::events(text_events("done")),
            true,
        )
        .await;
        let compacted_out_id = MessageId::new();
        let kept_id = MessageId::new();
        harness
            .append_records(vec![
                continuation_record(
                    harness.tenant_id,
                    harness.session_id,
                    compacted_out_id,
                    json!({"private": "compacted-out"}),
                ),
                continuation_record(
                    harness.tenant_id,
                    harness.session_id,
                    kept_id,
                    json!({"private": "kept"}),
                ),
            ])
            .await;

        let events = harness
            .run_with_seed(vec![
                assistant_tool_message(compacted_out_id),
                assistant_tool_message(kept_id),
            ])
            .await;

        assert!(completed(&events));
        let requests = harness.model.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].provider_context.continuations.len(), 1);
        assert_eq!(
            requests[0].provider_context.continuations[0].message_id,
            kept_id
        );
        assert!(!requests[0]
            .provider_context
            .continuations
            .iter()
            .any(|record| record.message_id == compacted_out_id));
    }

    #[tokio::test]
    async fn engine_fails_closed_when_required_assistant_tool_replay_continuation_is_missing() {
        let harness = ProviderContinuationHarness::new(
            ContextEngine::builder().build().unwrap(),
            RecordingModel::events(text_events("unreachable")),
            true,
        )
        .await;

        let error = harness
            .run_error_with_seed(vec![assistant_tool_message(MessageId::new())])
            .await;

        assert!(error.to_string().contains(SAFE_MISSING_CONTINUATION_ERROR));
        assert!(harness.model.requests().await.is_empty());
    }

    #[tokio::test]
    async fn engine_fails_closed_before_provider_request_when_required_replay_store_is_missing() {
        let harness = ProviderContinuationHarness::new(
            ContextEngine::builder().build().unwrap(),
            RecordingModel::events(text_events("unreachable")),
            false,
        )
        .await;

        let error = harness
            .run_error_with_seed(vec![assistant_tool_message(MessageId::new())])
            .await;

        assert!(error.to_string().contains(SAFE_MISSING_CONTINUATION_ERROR));
        assert!(harness.model.requests().await.is_empty());
    }

    #[tokio::test]
    async fn engine_requires_exact_continuation_match_not_record_count() {
        let harness = ProviderContinuationHarness::new(
            ContextEngine::builder().build().unwrap(),
            RecordingModel::events(text_events("unreachable")),
            true,
        )
        .await;
        let required_id = MessageId::new();
        harness
            .append_records(vec![continuation_record(
                harness.tenant_id,
                harness.session_id,
                MessageId::new(),
                json!({"private": "wrong-message"}),
            )])
            .await;

        let error = harness
            .run_error_with_seed(vec![assistant_tool_message(required_id)])
            .await;

        assert!(error.to_string().contains(SAFE_MISSING_CONTINUATION_ERROR));
        assert!(harness.model.requests().await.is_empty());
    }

    #[tokio::test]
    async fn engine_continuation_dialect_comes_from_runtime_semantics_not_provider_id() {
        let harness = ProviderContinuationHarness::new_with_model_snapshot(
            ContextEngine::builder().build().unwrap(),
            RecordingModel::events(text_events("done")),
            true,
            runtime_model_snapshot_for_provider("deepseek-compatible"),
        )
        .await;
        let assistant_id = MessageId::new();
        harness
            .append_records(vec![continuation_record_for_provider(
                "deepseek-compatible",
                harness.tenant_id,
                harness.session_id,
                assistant_id,
                json!({"private": PRIVATE_SENTINEL}),
            )])
            .await;

        let events = harness
            .run_with_seed_and_model_snapshot(
                vec![assistant_tool_message(assistant_id)],
                model_snapshot_for_provider("deepseek-compatible"),
            )
            .await;

        assert!(completed(&events));
        let requests = harness.model.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].provider_context.provider_id,
            "deepseek-compatible"
        );
        assert_eq!(
            requests[0].provider_context.dialect.as_deref(),
            Some(DEEPSEEK_CONTINUATION_DIALECT)
        );
        assert_eq!(requests[0].provider_context.continuations.len(), 1);
        assert_eq!(
            requests[0].provider_context.continuations[0].dialect,
            DEEPSEEK_CONTINUATION_DIALECT
        );
    }

    #[tokio::test]
    async fn engine_stores_provider_continuation_outside_journal() {
        let harness = ProviderContinuationHarness::new(
            ContextEngine::builder().build().unwrap(),
            RecordingModel::events(provider_continuation_then_text_events()),
            true,
        )
        .await;

        let events = harness.run_with_seed(Vec::new()).await;

        let completed_assistant_id = events
            .iter()
            .find_map(|event| match event {
                Event::AssistantMessageCompleted(completed) => Some(completed.message_id),
                _ => None,
            })
            .expect("assistant message completed");
        let records = harness.load_records(vec![completed_assistant_id]).await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].message_id, completed_assistant_id);
        assert_eq!(records[0].producing_run_id, harness.last_run_id().await);
        assert_eq!(records[0].payload["private"], PRIVATE_SENTINEL);

        let event_json = serde_json::to_string(&events).unwrap();
        assert!(!event_json.contains(PRIVATE_SENTINEL));
    }
}

fn completed(events: &[Event]) -> bool {
    events.iter().any(
        |event| matches!(event, Event::RunEnded(ended) if ended.reason == harness_contracts::EndReason::Completed),
    )
}

struct ProviderContinuationHarness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    store: Arc<FileProviderContinuationStore>,
    engine: Engine,
    model: Arc<RecordingModel>,
    run_ids: Mutex<Vec<RunId>>,
}

impl ProviderContinuationHarness {
    async fn new(context: ContextEngine, model: RecordingModel, configure_store: bool) -> Self {
        Self::new_with_model_snapshot(context, model, configure_store, model_snapshot()).await
    }

    async fn new_with_model_snapshot(
        context: ContextEngine,
        model: RecordingModel,
        configure_store: bool,
        model_snapshot: harness_model::ModelRuntimeSnapshot,
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let store = Arc::new(
            FileProviderContinuationStore::open_runtime_dir(
                workspace.path().join(".jyowo").join("runtime"),
            )
            .unwrap(),
        );
        let model = Arc::new(model);
        let mut builder = Engine::builder()
            .with_engine_id(EngineId::new("provider-continuation-runtime-test"))
            .with_event_store(event_store.clone())
            .with_context(context)
            .with_hooks(HookDispatcher::new(
                HookRegistry::builder().build().unwrap().snapshot(),
            ))
            .with_model(model.clone())
            .with_model_snapshot(model_snapshot)
            .with_tools(ToolPool::default())
            .with_authorization_service(test_authorization_service(
                Arc::new(AllowBroker),
                event_store.clone(),
            ))
            .with_workspace_root(workspace.path())
            .with_model_id("deepseek-chat")
            .with_protocol(ModelProtocol::ChatCompletions)
            .with_cap_registry(Arc::new(CapabilityRegistry::default()));
        if configure_store {
            builder = builder.with_provider_continuation_store(store.clone());
        }
        let engine = builder.build().unwrap();

        Self {
            _workspace: workspace,
            tenant_id,
            session_id,
            store,
            engine,
            model,
            run_ids: Mutex::new(Vec::new()),
        }
    }

    async fn run_with_seed(&self, seed: Vec<Message>) -> Vec<Event> {
        self.run_with_seed_and_model_snapshot(seed, run_model_snapshot())
            .await
    }

    async fn run_with_seed_and_model_snapshot(
        &self,
        seed: Vec<Message>,
        model_snapshot: RunModelSnapshot,
    ) -> Vec<Event> {
        let run_id = RunId::new();
        self.run_ids.lock().await.push(run_id);
        self.engine
            .run(
                SessionHandle {
                    tenant_id: self.tenant_id,
                    session_id: self.session_id,
                },
                turn_input("continue"),
                run_context(
                    self.tenant_id,
                    self.session_id,
                    run_id,
                    seed,
                    model_snapshot,
                ),
            )
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
    }

    async fn run_error_with_seed(&self, seed: Vec<Message>) -> harness_contracts::EngineError {
        let run_id = RunId::new();
        self.run_ids.lock().await.push(run_id);
        match self
            .engine
            .run(
                SessionHandle {
                    tenant_id: self.tenant_id,
                    session_id: self.session_id,
                },
                turn_input("continue"),
                run_context(
                    self.tenant_id,
                    self.session_id,
                    run_id,
                    seed,
                    run_model_snapshot(),
                ),
            )
            .await
        {
            Ok(_) => panic!("expected provider continuation failure"),
            Err(error) => error,
        }
    }

    async fn append_records(&self, records: Vec<ProviderContinuationRecord>) {
        self.store.append_batch(records).await.unwrap();
    }

    async fn load_records(&self, message_ids: Vec<MessageId>) -> Vec<ProviderContinuationRecord> {
        self.store
            .load_for_messages(harness_provider_state::ProviderContinuationQuery {
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

    async fn last_run_id(&self) -> RunId {
        *self.run_ids.lock().await.last().expect("run id")
    }
}

struct RecordingModel {
    requests: Mutex<Vec<ModelRequest>>,
    response: Vec<ModelStreamEvent>,
}

impl RecordingModel {
    fn events(response: Vec<ModelStreamEvent>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            response,
        }
    }

    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for RecordingModel {
    fn provider_id(&self) -> &str {
        "deepseek"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: ModelProtocol::ChatCompletions,
            lifecycle: ModelLifecycle::Stable,
            provider_id: "deepseek".to_owned(),
            model_id: "deepseek-chat".to_owned(),
            display_name: "DeepSeek Chat".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: ModelRuntimeSemantics::openai_chat_deepseek(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        Ok(Box::pin(stream::iter(self.response.clone())))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
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

fn model_snapshot() -> harness_model::ModelRuntimeSnapshot {
    runtime_model_snapshot_for_provider("deepseek")
}

fn runtime_model_snapshot_for_provider(provider_id: &str) -> harness_model::ModelRuntimeSnapshot {
    harness_model::ModelRuntimeSnapshot {
        provider_id: provider_id.to_owned(),
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

fn run_context(
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    seed: Vec<Message>,
    model_snapshot: RunModelSnapshot,
) -> RunContext {
    RunContext::new(tenant_id, session_id, run_id)
        .with_context_seed(seed)
        .with_model_snapshot(model_snapshot)
}

fn run_model_snapshot() -> RunModelSnapshot {
    model_snapshot_for_provider("deepseek")
}

fn model_snapshot_for_provider(provider_id: &str) -> RunModelSnapshot {
    RunModelSnapshot {
        model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
        provider_id: provider_id.to_owned(),
        model_id: "deepseek-chat".to_owned(),
        display_name: "DeepSeek Chat".to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window: 8_000,
        max_output_tokens: 1_000,
        conversation_capability: ConversationModelCapability::default(),
    }
}

fn continuation_record(
    tenant_id: TenantId,
    session_id: SessionId,
    message_id: MessageId,
    payload: serde_json::Value,
) -> ProviderContinuationRecord {
    continuation_record_for_provider("deepseek", tenant_id, session_id, message_id, payload)
}

fn continuation_record_for_provider(
    provider_id: &str,
    tenant_id: TenantId,
    session_id: SessionId,
    message_id: MessageId,
    payload: serde_json::Value,
) -> ProviderContinuationRecord {
    ProviderContinuationRecord {
        provider_id: provider_id.to_owned(),
        model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
        protocol: ModelProtocol::ChatCompletions,
        dialect: DEEPSEEK_CONTINUATION_DIALECT.to_owned(),
        tenant_id,
        session_id,
        producing_run_id: RunId::new(),
        message_id,
        scope: ProviderContinuationScope::Conversation,
        kind: ProviderContinuationKind::ReasoningReplay,
        payload,
        created_at: harness_contracts::now(),
    }
}

fn assistant_tool_message(message_id: MessageId) -> Message {
    Message {
        id: message_id,
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: harness_contracts::ToolUseId::new(),
            name: "lookup".to_owned(),
            input: json!({"query": "jyowo"}),
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

fn text_events(text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "provider-assistant".to_owned(),
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

fn provider_continuation_then_text_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "provider-assistant".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({"private": PRIVATE_SENTINEL}),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("done".to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}
