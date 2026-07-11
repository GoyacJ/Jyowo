use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    AssistantMessageCompletedEvent, BlobId, BlobRef, ConfigHash, ContentHash, EndReason, Event,
    JournalOffset, Message, MessageContent, MessageId, MessagePart, MessageRole, ModelError,
    ModelProtocol, NoopRedactor, PermissionMode, RunId, RunModelSnapshot, RunStartedEvent,
    SessionId, SnapshotId, StopReason, SubagentContextReport, SubagentStatus,
    SubagentTerminationReason, ToolResult, ToolUseCompletedEvent, ToolUseId, TranscriptRef,
    UsageSnapshot, UserMessageAppendedEvent,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    AuxModelProvider, AuxOptions, AuxTask, ConversationModelCapability, HealthStatus, InferContext,
    ModelDescriptor, ModelProvider, ModelRequest, ModelStream,
};
use harness_subagent::{
    AnnounceMode, AuxAnnouncementSummarizer, ChildRunOutcome, ChildRunRequest, ChildSessionRunner,
    ConcurrencyPolicy, ConcurrentSubagentPool, DefaultSubagentRunner, MemorySelector,
    ParentContext, SubagentContextMode, SubagentEngineFactory, SubagentInputSelection,
    SubagentInputSelector, SubagentInputStrategy, SubagentMemoryScope, SubagentMemoryScopeRequest,
    SubagentMemoryScopeResolver, SubagentRunner, SubagentSpec,
};
use tokio::sync::{Mutex, Notify};

#[path = "default_runner/default_runner_cases.rs"]
mod default_runner_cases;

#[derive(Default)]
struct RecordingChildRunner {
    request: Mutex<Option<ChildRunRequest>>,
}

#[derive(Default)]
struct RecordingEngineFactory {
    request: Mutex<Option<ChildRunRequest>>,
}

struct AssistantOnlySelector;

impl SubagentInputSelector for AssistantOnlySelector {
    fn selector_id(&self) -> &str {
        "assistant-only"
    }

    fn select(
        &self,
        selection: SubagentInputSelection<'_>,
    ) -> Result<Vec<Message>, harness_subagent::SubagentError> {
        Ok(selection
            .parent_transcript
            .iter()
            .filter(|message| message.role == MessageRole::Assistant)
            .cloned()
            .collect())
    }
}

struct TagMemoryResolver;

impl SubagentMemoryScopeResolver for TagMemoryResolver {
    fn resolve(
        &self,
        request: SubagentMemoryScopeRequest<'_>,
    ) -> Result<Vec<Message>, harness_subagent::SubagentError> {
        let selectors = request
            .selectors
            .iter()
            .map(|selector| match selector {
                MemorySelector::Tag(tag) => format!("memory tag: {tag}"),
                MemorySelector::Provider(provider) => format!("memory provider: {provider}"),
            })
            .collect::<Vec<_>>();
        Ok(selectors.into_iter().map(text_message).collect())
    }
}

#[async_trait]
impl SubagentEngineFactory for RecordingEngineFactory {
    async fn run_child_engine(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, harness_subagent::SubagentError> {
        self.request.lock().await.replace(request);
        Ok(ChildRunOutcome {
            status: SubagentStatus::Completed,
            summary: "factory completed".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

#[async_trait]
impl ChildSessionRunner for RecordingChildRunner {
    async fn run_child(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, harness_subagent::SubagentError> {
        self.request.lock().await.replace(request);
        Ok(ChildRunOutcome {
            status: SubagentStatus::Completed,
            summary: "child completed".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

struct TranscriptChildRunner;

#[async_trait]
impl ChildSessionRunner for TranscriptChildRunner {
    async fn run_child(
        &self,
        _request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, harness_subagent::SubagentError> {
        Ok(ChildRunOutcome {
            status: SubagentStatus::Completed,
            summary: "child completed".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: Some(fake_transcript_ref()),
            context_report: None,
        })
    }
}

struct ContextReportChildRunner;

#[async_trait]
impl ChildSessionRunner for ContextReportChildRunner {
    async fn run_child(
        &self,
        _request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, harness_subagent::SubagentError> {
        Ok(ChildRunOutcome {
            status: SubagentStatus::Completed,
            summary: "child completed".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: Some(fake_context_report()),
        })
    }
}

struct FailingChildRunner;

#[async_trait]
impl ChildSessionRunner for FailingChildRunner {
    async fn run_child(
        &self,
        _request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, harness_subagent::SubagentError> {
        Err(harness_subagent::SubagentError::Engine(
            "child failed".to_owned(),
        ))
    }
}

struct RecordingAuxProvider {
    result: Mutex<Result<String, ModelError>>,
    tasks: Mutex<Vec<AuxTask>>,
}

impl RecordingAuxProvider {
    fn new(result: Result<String, ModelError>) -> Self {
        Self {
            result: Mutex::new(result),
            tasks: Mutex::new(Vec::new()),
        }
    }

    async fn tasks(&self) -> Vec<AuxTask> {
        self.tasks.lock().await.clone()
    }
}

#[async_trait]
impl AuxModelProvider for RecordingAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::new(DummyModelProvider)
    }

    fn aux_options(&self) -> AuxOptions {
        AuxOptions {
            fail_open: true,
            ..AuxOptions::default()
        }
    }

    async fn call_aux(&self, task: AuxTask, _req: ModelRequest) -> Result<String, ModelError> {
        self.tasks.lock().await.push(task);
        self.result.lock().await.clone()
    }
}

struct DummyModelProvider;

#[async_trait]
impl ModelProvider for DummyModelProvider {
    fn provider_id(&self) -> &str {
        "dummy"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            supported_parameters: Vec::new(),
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "dummy".to_owned(),
            model_id: "dummy-aux".to_owned(),
            display_name: "Dummy Aux".to_owned(),
            context_window: 1_000,
            max_output_tokens: 100,
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
        Ok(Box::pin(stream::empty()))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Default)]
struct CancellableChildRunner {
    started: Notify,
}

#[async_trait]
impl ChildSessionRunner for CancellableChildRunner {
    async fn run_child(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, harness_subagent::SubagentError> {
        self.started.notify_one();
        request.cancellation.cancelled().await;
        Err(harness_subagent::SubagentError::Cancelled)
    }
}

fn test_input(text: &str) -> harness_contracts::TurnInput {
    harness_contracts::TurnInput {
        message: harness_contracts::Message {
            id: harness_contracts::MessageId::new(),
            role: harness_contracts::MessageRole::User,
            parts: vec![harness_contracts::MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::Value::Null,
    }
}

fn text_message(text: String) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(text)],
        created_at: harness_contracts::now(),
    }
}

async fn wait_for_terminated_events(
    store: &InMemoryEventStore,
    parent: &ParentContext,
    expected: usize,
) -> Vec<Event> {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let events: Vec<_> = store
                .read(
                    parent.tenant_id,
                    parent.parent_session_id,
                    ReplayCursor::FromStart,
                )
                .await
                .unwrap()
                .collect()
                .await;
            let terminated = events
                .iter()
                .filter(|event| matches!(event, Event::SubagentTerminated(_)))
                .count();
            if terminated >= expected {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("termination event should be written")
}

async fn spawn_with_context_seed(
    context_mode: SubagentContextMode,
    input_strategy: SubagentInputStrategy,
) -> ChildRunRequest {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    store
        .append(
            parent.tenant_id,
            parent.parent_session_id,
            &parent_transcript_events("old user", "assistant context", "latest user"),
        )
        .await
        .unwrap();
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = context_mode;
    spec.input_strategy = input_strategy;

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    request
}

fn parent_transcript_events(
    first_user: &str,
    assistant_text: &str,
    latest_user: &str,
) -> Vec<Event> {
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    vec![
        Event::RunStarted(RunStartedEvent {
            run_id,
            session_id: harness_contracts::SessionId::new(),
            tenant_id: harness_contracts::TenantId::SINGLE,
            parent_run_id: None,
            model: test_run_model_snapshot(),
            input: test_input(first_user),
            snapshot_id: SnapshotId::from_u128(0),
            effective_config_hash: ConfigHash([0; 32]),
            started_at: harness_contracts::now(),
            correlation_id: harness_contracts::CorrelationId::new(),
            permission_mode: PermissionMode::Default,
        }),
        Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
            run_id,
            message_id: MessageId::new(),
            content: MessageContent::Text(assistant_text.to_owned()),
            tool_uses: Vec::new(),
            usage: UsageSnapshot::default(),
            pricing_snapshot_id: None,
            stop_reason: StopReason::EndTurn,
            at: harness_contracts::now(),
        }),
        Event::ToolUseCompleted(ToolUseCompletedEvent {
            tool_use_id,
            result: ToolResult::Text("tool output".to_owned()),
            usage: None,
            duration_ms: 1,
            at: harness_contracts::now(),
        }),
        Event::UserMessageAppended(UserMessageAppendedEvent {
            run_id,
            message_id: MessageId::new(),
            content: MessageContent::Text(latest_user.to_owned()),
            metadata: Default::default(),
            attachments: Vec::new(),
            at: harness_contracts::now(),
        }),
        Event::RunEnded(harness_contracts::RunEndedEvent {
            run_id,
            reason: EndReason::Completed,
            usage: Some(UsageSnapshot::default()),
            ended_at: harness_contracts::now(),
        }),
    ]
}

fn test_run_model_snapshot() -> RunModelSnapshot {
    RunModelSnapshot {
        model_config_id: None,
        provider_id: "test".to_owned(),
        model_id: "test-model".to_owned(),
        display_name: "Test Model".to_owned(),
        protocol: ModelProtocol::Messages,
        context_window: 128_000,
        max_output_tokens: 8_192,
        conversation_capability: ConversationModelCapability::default(),
    }
}

fn message_roles(messages: &[Message]) -> Vec<MessageRole> {
    messages.iter().map(|message| message.role).collect()
}

fn message_texts(messages: &[Message]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| match message.parts.as_slice() {
            [MessagePart::Text(text)] => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn fake_transcript_ref() -> TranscriptRef {
    TranscriptRef {
        blob: BlobRef {
            id: BlobId::new(),
            size: 2,
            content_hash: blake3::hash(b"[]").into(),
            content_type: Some("application/json".to_owned()),
        },
        from_offset: JournalOffset(1),
        to_offset: JournalOffset(2),
    }
}

fn fake_context_report() -> SubagentContextReport {
    SubagentContextReport {
        parent_system_hash: Some(ContentHash([1; 32])),
        child_system_hash: ContentHash([2; 32]),
        shared_system_prefix_hash: Some(ContentHash([1; 32])),
        prompt_cache_prefix_reused: true,
        bootstrap_files_inherited: vec!["AGENTS.md".to_owned()],
        system_header_extra_applied: true,
    }
}
