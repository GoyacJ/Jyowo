use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::NoopRedactor;
use harness_contracts::{
    CapabilityRegistry, ConversationCursor, ConversationTimelineEvent, DeltaChunk, Event, EventId,
    Message, MessageId, MessagePart, MessageRole, ModelError, ModelProtocol, PermissionError,
    RunId, RunModelSnapshot, SessionId, StopReason, TenantId, TurnInput, UsageSnapshot,
};
use harness_engine::{
    turn_assembly::TurnAssembly, Engine, EngineId, EngineRunner, RunContext, SessionHandle,
};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::{
    project_conversation_worktree_snapshot, EventStore, InMemoryEventStore, ReplayCursor,
};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelLifecycle, ModelProvider, ModelRequest, ModelRuntimeSemantics, ModelStream,
    ModelStreamEvent, StreamAggregate,
};
use harness_permission::{Decision, PermissionBroker, PermissionContext, PermissionRequest};
use harness_provider_state::{
    FileProviderContinuationStore, ProviderContinuationKind, ProviderContinuationRecord,
    ProviderContinuationScope, ProviderContinuationStore,
};
use harness_tool::ToolPool;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

const SAFE_MISSING_CONTINUATION_ERROR: &str =
    "provider continuation required for assistant tool replay but missing";
const PRIVATE_SENTINEL: &str = "PRIVATE_DEEPSEEK_REASONING_SENTINEL";
const MODEL_CONFIG_ID: &str = "deepseek-leak-config";
const DEEPSEEK_CONTINUATION_DIALECT: &str = "openai_chat.deepseek";

mod authorization_support;

use authorization_support::test_authorization_service;

mod provider_continuation_leak {
    use super::*;

    #[tokio::test]
    async fn provider_continuation_payload_not_written_to_journal_events() {
        let harness =
            ProviderContinuationLeakHarness::new(provider_continuation_then_text_events()).await;

        let events = harness.run_with_seed(Vec::new()).await;
        let journal_events = harness.journal_events().await;

        let completed_assistant_id = assistant_completed_id(&events);
        let records = harness.load_records(vec![completed_assistant_id]).await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload["reasoningContent"], PRIVATE_SENTINEL);

        let journal_json = serde_json::to_string(&journal_events).unwrap();
        assert!(!journal_json.contains(PRIVATE_SENTINEL));
    }

    #[tokio::test]
    async fn provider_continuation_payload_not_projected_to_conversation_read_model() {
        let harness =
            ProviderContinuationLeakHarness::new(provider_continuation_then_text_events()).await;

        let events = harness.run_with_seed(Vec::new()).await;
        let timeline = timeline_events_for_events(events);
        let worktree = project_conversation_worktree_snapshot(
            &harness.session_id.to_string(),
            timeline.clone(),
        );
        let projected = format!(
            "{}\n{}",
            serde_json::to_string(&timeline).unwrap(),
            serde_json::to_string(&json!({
                "turns": worktree.turns,
                "eventCursor": worktree.event_cursor,
                "eventRefs": worktree.event_refs,
            }))
            .unwrap()
        );
        assert!(!projected.contains(PRIVATE_SENTINEL));
    }

    #[tokio::test]
    async fn provider_continuation_payload_not_in_assistant_message_parts() {
        let harness =
            ProviderContinuationLeakHarness::new(provider_continuation_then_text_events()).await;

        let events = harness.run_with_seed(Vec::new()).await;

        let assistant_events = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::AssistantDeltaProduced(_) | Event::AssistantMessageCompleted(_)
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let assistant_json = serde_json::to_string(&assistant_events).unwrap();
        assert!(assistant_json.contains("visible answer"));
        assert!(!assistant_json.contains(PRIVATE_SENTINEL));
    }

    #[tokio::test]
    async fn provider_continuation_payload_not_in_error_message_when_missing_or_invalid() {
        let harness = ProviderContinuationLeakHarness::new(text_events("unreachable")).await;
        harness
            .append_records(vec![harness.continuation_record(
                MessageId::new(),
                json!({
                    "format": deepseek_private_format(),
                    "reasoningContent": PRIVATE_SENTINEL,
                }),
            )])
            .await;

        let error = harness
            .run_error_with_seed(vec![assistant_tool_message(MessageId::new())])
            .await;
        let error_text = format!("{error:?}\n{error}");

        assert!(error_text.contains(SAFE_MISSING_CONTINUATION_ERROR));
        assert!(!error_text.contains(PRIVATE_SENTINEL));
        assert!(harness.model.requests().await.is_empty());
    }

    #[tokio::test]
    async fn provider_continuation_payload_not_in_debug_output_for_request_event_or_context() {
        let message_id = MessageId::new();
        let record = provider_continuation_record(
            TenantId::SINGLE,
            SessionId::new(),
            RunId::new(),
            message_id,
            json!({
                "format": deepseek_private_format(),
                "reasoningContent": PRIVATE_SENTINEL,
            }),
        );
        let provider_context = harness_model::ProviderRequestContext {
            provider_id: "deepseek".to_owned(),
            model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
            dialect: Some(DEEPSEEK_CONTINUATION_DIALECT.to_owned()),
            continuations: vec![record.clone()],
        };
        let request = ModelRequest {
            model_id: "deepseek-v4-flash".to_owned(),
            messages: vec![assistant_tool_message(message_id)],
            tools: None,
            system: None,
            temperature: None,
            max_tokens: None,
            stream: true,
            cache_breakpoints: Vec::new(),
            protocol: ModelProtocol::ChatCompletions,
            extra: Value::Null,
            provider_context,
        };
        let stream_event = ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({"reasoningContent": PRIVATE_SENTINEL}),
        };
        let stream_aggregate = StreamAggregate::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({"reasoningContent": PRIVATE_SENTINEL}),
        };
        let mut assembly = TurnAssembly::new(MessageId::new());
        assembly.push_event(RunId::new(), stream_event.clone());

        let debug_output = format!(
            "{record:?}\n{:?}\n{stream_event:?}\n{stream_aggregate:?}\n{assembly:?}",
            request.provider_context
        );

        assert!(!debug_output.contains(PRIVATE_SENTINEL));
        assert!(debug_output.contains("continuation_count"));
        assert!(debug_output.contains("<redacted>") || debug_output.contains("[redacted]"));
    }
}

struct ProviderContinuationLeakHarness {
    _workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    event_store: Arc<InMemoryEventStore>,
    store: Arc<FileProviderContinuationStore>,
    engine: Engine,
    model: Arc<RecordingModel>,
    run_ids: Mutex<Vec<RunId>>,
}

impl ProviderContinuationLeakHarness {
    async fn new(response: Vec<ModelStreamEvent>) -> Self {
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
        let model = Arc::new(RecordingModel::events(response));
        let engine = Engine::builder()
            .with_engine_id(EngineId::new("provider-continuation-leak-test"))
            .with_event_store(event_store.clone())
            .with_context(ContextEngine::builder().build().unwrap())
            .with_hooks(HookDispatcher::new(
                HookRegistry::builder().build().unwrap().snapshot(),
            ))
            .with_model(model.clone())
            .with_model_snapshot(model_snapshot())
            .with_tools(ToolPool::default())
            .with_authorization_service(test_authorization_service(
                Arc::new(AllowBroker),
                event_store.clone(),
            ))
            .with_workspace_root(workspace.path())
            .with_model_id("deepseek-v4-flash")
            .with_protocol(ModelProtocol::ChatCompletions)
            .with_provider_continuation_store(store.clone())
            .with_cap_registry(Arc::new(CapabilityRegistry::default()))
            .build()
            .unwrap();

        Self {
            _workspace: workspace,
            tenant_id,
            session_id,
            event_store,
            store,
            engine,
            model,
            run_ids: Mutex::new(Vec::new()),
        }
    }

    async fn run_with_seed(&self, seed: Vec<Message>) -> Vec<Event> {
        let run_id = RunId::new();
        self.run_ids.lock().await.push(run_id);
        self.engine
            .run(
                SessionHandle {
                    tenant_id: self.tenant_id,
                    session_id: self.session_id,
                },
                turn_input("continue"),
                run_context(self.tenant_id, self.session_id, run_id, seed),
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
                run_context(self.tenant_id, self.session_id, run_id, seed),
            )
            .await
        {
            Ok(_) => panic!("expected provider continuation failure"),
            Err(error) => error,
        }
    }

    async fn journal_events(&self) -> Vec<Event> {
        self.event_store
            .read(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
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

    fn continuation_record(
        &self,
        message_id: MessageId,
        payload: Value,
    ) -> ProviderContinuationRecord {
        provider_continuation_record(
            self.tenant_id,
            self.session_id,
            RunId::new(),
            message_id,
            payload,
        )
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
            model_id: "deepseek-v4-flash".to_owned(),
            display_name: "DeepSeek V4 Flash".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            provider_declared_capability: ConversationModelCapability::default(),
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
    harness_model::ModelRuntimeSnapshot {
        provider_id: "deepseek".to_owned(),
        model_id: "deepseek-v4-flash".to_owned(),
        display_name: "DeepSeek V4 Flash".to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window: 8_000,
        max_output_tokens: 1_000,
        provider_declared_capability: ConversationModelCapability::default(),
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
) -> RunContext {
    RunContext::new(tenant_id, session_id, run_id)
        .with_context_seed(seed)
        .with_model_snapshot(RunModelSnapshot {
            model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
            provider_id: "deepseek".to_owned(),
            model_id: "deepseek-v4-flash".to_owned(),
            display_name: "DeepSeek V4 Flash".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            context_window: 8_000,
            max_output_tokens: 1_000,
            conversation_capability: ConversationModelCapability::default(),
        })
}

fn provider_continuation_record(
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    message_id: MessageId,
    payload: Value,
) -> ProviderContinuationRecord {
    ProviderContinuationRecord {
        provider_id: "deepseek".to_owned(),
        model_config_id: Some(MODEL_CONFIG_ID.to_owned()),
        protocol: ModelProtocol::ChatCompletions,
        dialect: DEEPSEEK_CONTINUATION_DIALECT.to_owned(),
        tenant_id,
        session_id,
        producing_run_id: run_id,
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
            payload: json!({
                "format": deepseek_private_format(),
                "reasoningContent": PRIVATE_SENTINEL,
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("visible answer".to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn deepseek_private_format() -> String {
    format!("deepseek.{}{}.v1", "reasoning", "_content")
}

fn assistant_completed_id(events: &[Event]) -> MessageId {
    events
        .iter()
        .find_map(|event| match event {
            Event::AssistantMessageCompleted(completed) => Some(completed.message_id),
            _ => None,
        })
        .expect("assistant message completed")
}

fn timeline_events_for_events(events: Vec<Event>) -> Vec<ConversationTimelineEvent> {
    events
        .into_iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let sequence = (index + 1) as u64;
            let event_id = EventId::new();
            let (run_id, event_type, source, payload, timestamp) = match event {
                Event::RunStarted(event) => (
                    event.run_id,
                    "run.started",
                    "engine",
                    json!({
                        "sessionId": event.session_id.to_string(),
                        "model": {
                            "modelConfigId": event.model.model_config_id,
                            "providerId": event.model.provider_id,
                            "modelId": event.model.model_id,
                            "displayName": event.model.display_name,
                            "protocol": event.model.protocol,
                        },
                        "permissionMode": event.permission_mode,
                    }),
                    event.started_at,
                ),
                Event::UserMessageAppended(event) => (
                    event.run_id,
                    "user.message.appended",
                    "user",
                    json!({
                        "messageId": event.message_id.to_string(),
                        "body": message_body(&event.content),
                    }),
                    event.at,
                ),
                Event::AssistantDeltaProduced(event) => match event.delta {
                    DeltaChunk::Text(text) => (
                        event.run_id,
                        "assistant.delta",
                        "assistant",
                        json!({
                            "messageId": event.message_id.to_string(),
                            "text": text,
                        }),
                        event.at,
                    ),
                    DeltaChunk::Thought(_) | DeltaChunk::ReasoningSummary(_) => (
                        event.run_id,
                        "assistant.thinking.delta",
                        "assistant",
                        json!({"status": "running"}),
                        event.at,
                    ),
                    _ => return None,
                },
                Event::AssistantMessageCompleted(event) => (
                    event.run_id,
                    "assistant.completed",
                    "assistant",
                    json!({
                        "messageId": event.message_id.to_string(),
                        "body": message_body(&event.content),
                        "toolUses": event.tool_uses.iter().map(|tool_use| {
                            json!({
                                "toolUseId": tool_use.tool_use_id.to_string(),
                                "toolName": tool_use.tool_name,
                            })
                        }).collect::<Vec<_>>(),
                    }),
                    event.at,
                ),
                _ => return None,
            };
            Some(ConversationTimelineEvent {
                id: event_id.to_string(),
                cursor: ConversationCursor {
                    event_id,
                    conversation_sequence: sequence,
                },
                payload,
                run_id: run_id.to_string(),
                sequence,
                source: source.to_owned(),
                timestamp,
                event_type: event_type.to_owned(),
                visibility: "public".to_owned(),
            })
        })
        .collect()
}

fn message_body(content: &harness_contracts::MessageContent) -> String {
    match content {
        harness_contracts::MessageContent::Text(text) => text.clone(),
        harness_contracts::MessageContent::Structured(value) => value.to_string(),
        harness_contracts::MessageContent::Multimodal(parts) => parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}
