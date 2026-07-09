use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    AssistantMessageCompletedEvent, BlobId, BlobRef, ConfigHash, ContentHash, EndReason, Event,
    JournalOffset, Message, MessageContent, MessageId, MessagePart, MessageRole, ModelError,
    ModelProtocol, NoopRedactor, PermissionMode, RunId, RunModelSnapshot, RunStartedEvent,
    SnapshotId, StopReason, SubagentContextReport, SubagentStatus, SubagentTerminationReason,
    ToolResult, ToolUseCompletedEvent, ToolUseId, TranscriptRef, UsageSnapshot,
    UserMessageAppendedEvent,
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

#[tokio::test]
async fn default_runner_creates_child_session_runs_child_and_journals_lifecycle() {
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

    let spec = SubagentSpec::minimal("reviewer", "inspect");
    let announcement = runner
        .spawn(spec.clone(), test_input("inspect"), parent.clone())
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    assert_eq!(announcement.summary, "child completed");
    let request = child.request.lock().await.clone().unwrap();
    assert_ne!(request.child_session_id, parent.parent_session_id);
    assert_eq!(request.parent_session_id, parent.parent_session_id);
    assert_eq!(request.spec, spec);
    assert_eq!(request.child_depth, parent.depth + 1);
    assert_eq!(request.correlation_id, parent.correlation_id);
    assert!(request.context_seed.is_empty());

    let parent_envelopes: Vec<_> = store
        .read_envelopes(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_envelopes
        .iter()
        .all(|envelope| envelope.correlation_id == parent.correlation_id));
    assert!(parent_envelopes
        .iter()
        .any(|envelope| matches!(envelope.payload, Event::SubagentSpawned(ref event) if event.spec_snapshot_id != harness_contracts::SnapshotId::from_u128(0))));
    assert!(parent_envelopes
        .iter()
        .any(|envelope| matches!(envelope.payload, Event::SubagentAnnounced(_))));
    let announced_index = parent_envelopes
        .iter()
        .position(|envelope| matches!(envelope.payload, Event::SubagentAnnounced(_)))
        .expect("subagent announcement should be journaled");
    let injected_index = parent_envelopes
        .iter()
        .position(|envelope| {
            matches!(
                &envelope.payload,
                Event::UserMessageAppended(appended)
                    if appended.metadata.source.as_deref() == Some("subagent")
                        && appended.metadata.labels.get("renderer_id")
                            == Some(&"xml-task-notification".to_owned())
                        && matches!(&appended.content, MessageContent::Text(text)
                            if text.contains("<task-notification>")
                                && text.contains("<rewrite-hint>"))
            )
        })
        .expect("subagent announcement should be injected as a parent user message");
    assert!(
        announced_index < injected_index,
        "SubagentAnnounced must precede UserMessageAppended"
    );
    assert!(parent_envelopes.iter().any(|envelope| {
        matches!(
            envelope.payload,
            Event::SubagentTerminated(ref event)
                if event.reason == SubagentTerminationReason::NaturalCompletion
        )
    }));

    let child_events: Vec<_> = store
        .read(
            parent.tenant_id,
            request.child_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(child_events
        .iter()
        .any(|event| matches!(event, Event::SessionCreated(_))));
}

#[tokio::test]
async fn default_runner_uses_aux_summarizer_for_announcement_summary() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let aux = Arc::new(RecordingAuxProvider::new(Ok(
        "aux rewritten summary".to_owned()
    )));
    let runner = DefaultSubagentRunner::new(
        child,
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_announcement_summarizer(Arc::new(AuxAnnouncementSummarizer::new(aux.clone())));

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            ParentContext::for_test(0),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.summary, "aux rewritten summary");
    assert_eq!(aux.tasks().await, vec![AuxTask::Summarize]);
}

#[tokio::test]
async fn default_runner_accepts_engine_factory_as_production_path() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let factory = Arc::new(RecordingEngineFactory::default());
    let runner = DefaultSubagentRunner::new_with_engine_factory(
        factory.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    assert_eq!(
        factory
            .request
            .lock()
            .await
            .as_ref()
            .unwrap()
            .correlation_id,
        parent.correlation_id
    );
}

#[tokio::test]
async fn default_runner_keeps_original_summary_when_aux_summarizer_fails_open() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let aux = Arc::new(RecordingAuxProvider::new(Err(
        ModelError::ProviderUnavailable("aux down".to_owned()),
    )));
    let runner = DefaultSubagentRunner::new(
        child,
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_announcement_summarizer(Arc::new(AuxAnnouncementSummarizer::new(aux)));

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            ParentContext::for_test(0),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.summary, "child completed");
}

#[tokio::test]
async fn structured_announcement_drops_child_transcript_ref() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(TranscriptChildRunner);
    let runner = DefaultSubagentRunner::new(
        child,
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let spec = SubagentSpec::minimal("reviewer", "inspect");

    let announcement = runner
        .spawn(spec, test_input("inspect"), parent.clone())
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.transcript_ref, None);
    let parent_events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced) if announced.transcript_ref.is_none()
        )
    }));
}

#[tokio::test]
async fn full_transcript_announcement_keeps_child_transcript_ref() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(TranscriptChildRunner);
    let runner = DefaultSubagentRunner::new(
        child,
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.announce_mode = AnnounceMode::FullTranscript;

    let announcement = runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let transcript_ref = announcement
        .transcript_ref
        .expect("full transcript mode should retain transcript ref");
    assert_eq!(transcript_ref.from_offset, JournalOffset(1));
    assert_eq!(transcript_ref.to_offset, JournalOffset(2));
    assert_eq!(
        transcript_ref.blob.content_hash,
        *blake3::hash(b"[]").as_bytes()
    );
}

#[tokio::test]
async fn default_runner_persists_child_context_report_on_announcement() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(ContextReportChildRunner);
    let runner = DefaultSubagentRunner::new(
        child,
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let expected = fake_context_report();
    assert_eq!(announcement.context_report, Some(expected.clone()));
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
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.context_report == Some(expected.clone())
        )
    }));
}

#[tokio::test]
async fn fork_latest_user_seeds_only_latest_parent_user_and_writes_session_forked() {
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
    let parent_events = parent_transcript_events("old user", "assistant context", "latest user");
    let last_parent_offset = store
        .append(parent.tenant_id, parent.parent_session_id, &parent_events)
        .await
        .unwrap();

    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::LatestUserOnly;
    let announcement = runner
        .spawn(spec, test_input("inspect"), parent.clone())
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    let request = child.request.lock().await.clone().unwrap();
    assert_eq!(message_texts(&request.context_seed), vec!["latest user"]);

    let parent_envelopes: Vec<_> = store
        .read_envelopes(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::SessionForked(forked)
                if forked.parent_session_id == parent.parent_session_id
                    && forked.child_session_id == request.child_session_id
                    && forked.from_offset == last_parent_offset
                    && !forked.cache_impact.prompt_cache_invalidated
        )
    }));
}

#[tokio::test]
async fn fork_inherit_all_can_exclude_tool_results() {
    let request = spawn_with_context_seed(
        SubagentContextMode::ForkFromParent {
            include_tool_results: false,
        },
        SubagentInputStrategy::InheritAll,
    )
    .await;

    assert_eq!(
        message_roles(&request.context_seed),
        vec![MessageRole::User, MessageRole::Assistant, MessageRole::User]
    );
    assert_eq!(
        message_texts(&request.context_seed),
        vec!["old user", "assistant context", "latest user"]
    );
}

#[tokio::test]
async fn fork_inherit_all_can_include_tool_results() {
    let request = spawn_with_context_seed(
        SubagentContextMode::ForkFromParent {
            include_tool_results: true,
        },
        SubagentInputStrategy::InheritAll,
    )
    .await;

    assert_eq!(
        message_roles(&request.context_seed),
        vec![
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::Tool,
            MessageRole::User
        ]
    );
    assert!(matches!(
        request.context_seed[2].parts.as_slice(),
        [MessagePart::ToolResult { content: ToolResult::Text(text), .. }] if text == "tool output"
    ));
}

#[tokio::test]
async fn custom_input_strategy_fails_closed_before_child_run() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::Custom {
        selector_id: "missing-selector".to_owned(),
    };

    let error = runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("missing-selector"),
        "unexpected error: {error}"
    );
    assert!(child.request.lock().await.is_none());
}

#[tokio::test]
async fn custom_input_strategy_uses_registered_selector() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_input_selector(Arc::new(AssistantOnlySelector));
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
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: true,
    };
    spec.input_strategy = SubagentInputStrategy::Custom {
        selector_id: "assistant-only".to_owned(),
    };

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    assert_eq!(
        message_roles(&request.context_seed),
        vec![MessageRole::Assistant]
    );
    assert_eq!(
        message_texts(&request.context_seed),
        vec!["assistant context"]
    );
}

#[tokio::test]
async fn memory_scope_subset_uses_registered_resolver() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_memory_scope_resolver(Arc::new(TagMemoryResolver));
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.memory_scope = SubagentMemoryScope::Subset {
        selectors: vec![MemorySelector::Tag("safe".to_owned())],
    };

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    assert!(request.memory_scope_resolved);
    assert_eq!(
        message_texts(&request.context_seed),
        vec!["memory tag: safe"]
    );
}

#[tokio::test]
async fn memory_scope_empty_resolves_without_resolver() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.memory_scope = SubagentMemoryScope::Empty;

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    assert!(request.memory_scope_resolved);
    assert!(request.context_seed.is_empty());
}

#[tokio::test]
async fn default_runner_terminates_failed_child_runs() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let runner = DefaultSubagentRunner::new(
        Arc::new(FailingChildRunner),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);

    let err = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, harness_subagent::SubagentError::Engine(_)));
    let parent_events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(terminated.reason, SubagentTerminationReason::Failed { .. })
        )
    }));
}

#[tokio::test]
async fn runner_watchdog_tick_cancels_stalled_child_and_writes_termination() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(CancellableChildRunner::default());
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 1,
        global_limit: 128,
        acquire_timeout: Duration::from_millis(10),
        activity_timeout: Duration::ZERO,
    });
    let runner = Arc::new(
        DefaultSubagentRunner::new(
            child.clone(),
            store.clone(),
            workspace.path(),
            harness_subagent::DelegationPolicy::default(),
        )
        .with_pool(pool),
    );
    let parent = ParentContext::for_test(0);
    let spawn = {
        let runner = runner.clone();
        let parent = parent.clone();
        tokio::spawn(async move {
            runner
                .spawn(
                    SubagentSpec::minimal("reviewer", "inspect"),
                    test_input("inspect"),
                    parent,
                )
                .await
        })
    };

    child.started.notified().await;
    let cancelled = runner.watchdog_tick().await.unwrap();
    assert_eq!(cancelled.len(), 1);
    let result = spawn.await.unwrap();
    assert!(matches!(
        result,
        Err(harness_subagent::SubagentError::Cancelled)
    ));

    let parent_events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(
        parent_events
            .iter()
            .filter(|event| matches!(event, Event::SubagentTerminated(_)))
            .count(),
        1
    );
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(terminated.reason, SubagentTerminationReason::Stalled { .. })
        )
    }));
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentStalled(stalled) if stalled.subagent_id == cancelled[0].subagent_id
        )
    }));
}

#[test]
fn runner_starts_watchdog_lazily_when_constructed_outside_runtime() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(CancellableChildRunner::default());
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 1,
        global_limit: 128,
        acquire_timeout: Duration::from_millis(10),
        activity_timeout: Duration::ZERO,
    });
    let runner = Arc::new(
        DefaultSubagentRunner::new(
            child.clone(),
            store.clone(),
            workspace.path(),
            harness_subagent::DelegationPolicy::default(),
        )
        .with_pool(pool)
        .with_watchdog_interval(Duration::from_millis(10)),
    );
    let parent = ParentContext::for_test(0);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async move {
        let spawn = {
            let runner = runner.clone();
            let parent = parent.clone();
            tokio::spawn(async move {
                runner
                    .spawn(
                        SubagentSpec::minimal("reviewer", "inspect"),
                        test_input("inspect"),
                        parent,
                    )
                    .await
            })
        };

        child.started.notified().await;
        let result = tokio::time::timeout(Duration::from_secs(1), spawn)
            .await
            .expect("lazy watchdog should cancel stalled child")
            .unwrap();
        assert!(matches!(
            result,
            Err(harness_subagent::SubagentError::Cancelled)
        ));

        let parent_events = wait_for_terminated_events(store.as_ref(), &parent, 1).await;
        assert_eq!(
            parent_events
                .iter()
                .filter(|event| matches!(event, Event::SubagentTerminated(_)))
                .count(),
            1
        );
        assert!(parent_events.iter().any(|event| {
            matches!(
                event,
                Event::SubagentTerminated(terminated)
                    if matches!(terminated.reason, SubagentTerminationReason::Stalled { .. })
            )
        }));
    });
}

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
