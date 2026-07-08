use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use harness_agent_runtime::{
    AgentRuntimeStore, BackgroundAgentManager, BackgroundAgentRecord, BackgroundAgentStartRequest,
    BackgroundAgentStoreRecord, BackgroundAgentTransitionError,
};
use harness_contracts::{
    BackgroundAgentId, BackgroundAgentState, Event, EventId, ForkReason, JournalError,
    JournalOffset, NoopRedactor, RedactRules, Redactor, RequestId, SessionId, TenantId,
};
use harness_journal::{
    AppendMetadata, EventEnvelope, EventStore, InMemoryEventStore, PrunePolicy, PruneReport,
    ReplayCursor, SessionFilter, SessionSnapshot, SessionSummary,
};
use tempfile::tempdir;

#[derive(Debug)]
struct TokenRedactor;

impl Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("sk-test-secret", "[REDACTED]")
    }
}

struct FailingAppendEventStore;

#[async_trait]
impl EventStore for FailingAppendEventStore {
    async fn append(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        Err(JournalError::Message("forced append failure".to_owned()))
    }

    async fn append_with_metadata(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _metadata: AppendMetadata,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        Err(JournalError::Message("forced append failure".to_owned()))
    }

    async fn read_envelopes(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn query_after(
        &self,
        _tenant: TenantId,
        _after: Option<EventId>,
        _limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Ok(Vec::new())
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(None)
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Ok(false)
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Ok(PruneReport::default())
    }
}

struct FailingInterruptedEventStore;

#[async_trait]
impl EventStore for FailingInterruptedEventStore {
    async fn append(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        if events
            .iter()
            .any(|event| matches!(event, Event::BackgroundAgentInterrupted(_)))
        {
            Err(JournalError::Message(
                "forced interrupted append failure".to_owned(),
            ))
        } else {
            Ok(JournalOffset(0))
        }
    }

    async fn append_with_metadata(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append(_tenant, _session_id, events).await
    }

    async fn read_envelopes(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn query_after(
        &self,
        _tenant: TenantId,
        _after: Option<EventId>,
        _limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Ok(Vec::new())
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(None)
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Ok(false)
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Ok(PruneReport::default())
    }
}

struct FailSecondAppendEventStore {
    calls: std::sync::atomic::AtomicUsize,
}

impl FailSecondAppendEventStore {
    fn new() -> Self {
        Self {
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl EventStore for FailSecondAppendEventStore {
    async fn append(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if call == 1 {
            Err(JournalError::Message(
                "forced second append failure".to_owned(),
            ))
        } else {
            Ok(JournalOffset(0))
        }
    }

    async fn append_with_metadata(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append(_tenant, _session_id, events).await
    }

    async fn read_envelopes(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn query_after(
        &self,
        _tenant: TenantId,
        _after: Option<EventId>,
        _limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Ok(Vec::new())
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(None)
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Ok(false)
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Ok(PruneReport::default())
    }
}

fn manager(
    store: Arc<AgentRuntimeStore>,
    event_store: Arc<InMemoryEventStore>,
    session_id: SessionId,
) -> BackgroundAgentManager {
    let event_store: Arc<dyn EventStore> = event_store;
    BackgroundAgentManager::new(
        store,
        event_store,
        TenantId::SINGLE,
        session_id,
        Arc::new(TokenRedactor),
    )
}

async fn events(event_store: &InMemoryEventStore, session_id: SessionId) -> Vec<Event> {
    let mut stream = event_store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("read events");
    let mut events = Vec::new();
    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        events.push(event);
    }
    events
}

fn title(record: &BackgroundAgentRecord) -> &str {
    &record.title
}

#[tokio::test]
async fn agent_orchestration_background_record_is_durable_before_execution_and_journaled() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store.clone(),
        TenantId::SINGLE,
        session_id,
        Arc::new(TokenRedactor),
    );

    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id: session_id,
            title: "Secret sk-test-secret task".to_owned(),
            payload_json: serde_json::json!({"kind":"test"}).to_string(),
        })
        .await
        .expect("start background agent");

    assert_eq!(record.state, BackgroundAgentState::Running);
    assert_eq!(title(&record), "Secret [REDACTED] task");
    assert_eq!(
        store
            .get_background_agent(record.background_agent_id.as_str())
            .expect("load record")
            .expect("record exists")
            .state,
        BackgroundAgentState::Running
    );
    assert_eq!(
        store
            .list_background_agent_attempts(record.background_agent_id.as_str())
            .expect("attempts")
            .len(),
        1
    );

    let events = events(&event_store, session_id).await;
    assert!(matches!(events[0], Event::BackgroundAgentStarted(_)));
    assert!(matches!(
        events[1],
        Event::BackgroundAgentStateChanged(ref event)
            if event.from == BackgroundAgentState::Queued && event.to == BackgroundAgentState::Running
    ));
}

#[tokio::test]
async fn agent_orchestration_background_lifecycle_operations_follow_transition_table() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = manager(
        Arc::clone(&store),
        Arc::clone(&event_store),
        conversation_id,
    );

    let running = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "Managed task".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");

    let paused = manager
        .pause(running.background_agent_id.as_str(), "operator pause")
        .await
        .expect("pause");
    assert_eq!(paused.state, BackgroundAgentState::Paused);

    let resumed = manager
        .resume(paused.background_agent_id.as_str(), "operator resume")
        .await
        .expect("resume");
    assert_eq!(resumed.state, BackgroundAgentState::Running);
    let events_after_resume = events(&event_store, conversation_id).await;
    assert!(events_after_resume.windows(2).any(|window| {
        matches!(
            (&window[0], &window[1]),
            (
                Event::BackgroundAgentStateChanged(first),
                Event::BackgroundAgentStateChanged(second)
            ) if first.from == BackgroundAgentState::Paused
                && first.to == BackgroundAgentState::Queued
                && second.from == BackgroundAgentState::Queued
                && second.to == BackgroundAgentState::Running
        )
    }));

    let input_request_id = RequestId::new();
    let waiting = manager
        .request_input(
            resumed.background_agent_id.as_str(),
            input_request_id,
            "Need token sk-test-secret",
        )
        .await
        .expect("request input");
    assert_eq!(waiting.state, BackgroundAgentState::WaitingForInput);

    let after_input = manager
        .send_input(
            waiting.background_agent_id.as_str(),
            input_request_id,
            "answer sk-test-secret",
        )
        .await
        .expect("send input");
    assert_eq!(after_input.state, BackgroundAgentState::Running);

    let paused_after_input = manager
        .pause(
            after_input.background_agent_id.as_str(),
            "pause after input",
        )
        .await
        .expect("pause after input");
    let resumed_after_input = manager
        .resume(
            paused_after_input.background_agent_id.as_str(),
            "resume after consumed input",
        )
        .await
        .expect("resume after consumed input");
    assert_eq!(resumed_after_input.state, BackgroundAgentState::Running);

    let cancelled = manager
        .cancel(
            resumed_after_input.background_agent_id.as_str(),
            "user cancelled",
        )
        .await
        .expect("cancel");
    assert_eq!(cancelled.state, BackgroundAgentState::Cancelled);

    let archived = manager
        .archive(cancelled.background_agent_id.as_str())
        .await
        .expect("archive");
    assert_eq!(archived.state, BackgroundAgentState::Archived);

    manager
        .delete_archived(archived.background_agent_id.as_str())
        .await
        .expect("delete archived");
    assert!(store
        .get_background_agent(archived.background_agent_id.as_str())
        .expect("lookup")
        .is_none());

    let rendered_events = serde_json::to_string(&events(&event_store, conversation_id).await)
        .expect("events serialize");
    assert!(!rendered_events.contains("sk-test-secret"));
    assert!(rendered_events.contains("background_agent_archived"));
    assert!(rendered_events.contains("background_agent_deleted"));
}

#[tokio::test]
async fn agent_orchestration_background_start_does_not_create_record_when_journal_append_fails() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let conversation_id = SessionId::new();
    let failing_manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        Arc::new(FailingAppendEventStore),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(TokenRedactor),
    );

    assert!(matches!(
        failing_manager
            .start(BackgroundAgentStartRequest {
                background_agent_id: None,
                conversation_id,
                title: "journal failure".to_owned(),
                payload_json: "{}".to_owned(),
            })
            .await,
        Err(BackgroundAgentTransitionError::Journal(_))
    ));

    assert!(store
        .list_background_agents(true)
        .expect("list backgrounds")
        .is_empty());
}

#[tokio::test]
async fn agent_orchestration_background_transition_does_not_mutate_state_when_journal_append_fails()
{
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let good_event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let good_manager = manager(
        Arc::clone(&store),
        Arc::clone(&good_event_store),
        conversation_id,
    );
    let record = good_manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "journal failure".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    let failing_manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        Arc::new(FailingAppendEventStore),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(TokenRedactor),
    );

    assert!(matches!(
        failing_manager
            .pause(record.background_agent_id.as_str(), "journal down")
            .await,
        Err(BackgroundAgentTransitionError::Journal(_))
    ));

    let stored = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(stored.state, BackgroundAgentState::Running);
}

#[tokio::test]
async fn agent_orchestration_background_delete_does_not_remove_record_when_journal_append_fails() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let good_event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let good_manager = manager(
        Arc::clone(&store),
        Arc::clone(&good_event_store),
        conversation_id,
    );
    let record = good_manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "delete journal failure".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    let cancelled = good_manager
        .cancel(record.background_agent_id.as_str(), "cancel")
        .await
        .expect("cancel");
    let archived = good_manager
        .archive(cancelled.background_agent_id.as_str())
        .await
        .expect("archive");
    let failing_manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        Arc::new(FailingAppendEventStore),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(TokenRedactor),
    );

    assert!(matches!(
        failing_manager
            .delete_archived(archived.background_agent_id.as_str())
            .await,
        Err(BackgroundAgentTransitionError::Journal(_))
    ));

    let stored = store
        .get_background_agent(archived.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background still exists");
    assert_eq!(stored.state, BackgroundAgentState::Archived);
}

#[tokio::test]
async fn agent_orchestration_background_resume_interrupted_does_not_create_attempt_when_journal_append_fails(
) {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let good_event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let good_manager = manager(
        Arc::clone(&store),
        Arc::clone(&good_event_store),
        conversation_id,
    );
    let record = good_manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "resume journal failure".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    good_manager
        .recover_on_startup("process restart")
        .await
        .expect("interrupt running");
    let attempts_before = store
        .list_background_agent_attempts(record.background_agent_id.as_str())
        .expect("attempts before");
    let failing_manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        Arc::new(FailingAppendEventStore),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(TokenRedactor),
    );

    assert!(matches!(
        failing_manager
            .resume(record.background_agent_id.as_str(), "resume")
            .await,
        Err(BackgroundAgentTransitionError::Journal(_))
    ));

    let attempts_after = store
        .list_background_agent_attempts(record.background_agent_id.as_str())
        .expect("attempts after");
    assert_eq!(attempts_after.len(), attempts_before.len());
    let stored = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(stored.state, BackgroundAgentState::Interrupted);
}

#[tokio::test]
async fn agent_orchestration_background_resume_interrupted_uses_single_durable_append_before_sqlite_mutation(
) {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let good_event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let good_manager = manager(
        Arc::clone(&store),
        Arc::clone(&good_event_store),
        conversation_id,
    );
    let record = good_manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "resume interrupted atomic append".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    good_manager
        .recover_on_startup("process restart")
        .await
        .expect("interrupt running");
    let attempts_before = store
        .list_background_agent_attempts(record.background_agent_id.as_str())
        .expect("attempts before");
    let fail_second_append_manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        Arc::new(FailSecondAppendEventStore::new()),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(TokenRedactor),
    );

    let resumed = fail_second_append_manager
        .resume(record.background_agent_id.as_str(), "resume")
        .await
        .expect("resume should use one durable append");

    assert_eq!(resumed.state, BackgroundAgentState::Running);
    let attempts_after = store
        .list_background_agent_attempts(record.background_agent_id.as_str())
        .expect("attempts after");
    assert_eq!(attempts_after.len(), attempts_before.len() + 1);
}

#[tokio::test]
async fn agent_orchestration_background_resume_interrupted_emits_started_for_new_attempt() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = manager(
        Arc::clone(&store),
        Arc::clone(&event_store),
        conversation_id,
    );
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "resume emits started".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    manager
        .recover_on_startup("process restart")
        .await
        .expect("interrupt running");

    let resumed = manager
        .resume(record.background_agent_id.as_str(), "operator resume")
        .await
        .expect("resume interrupted");
    let attempts = store
        .list_background_agent_attempts(record.background_agent_id.as_str())
        .expect("attempts");
    let resumed_attempt_id = attempts
        .last()
        .expect("resumed attempt")
        .attempt_id
        .as_str();

    assert_eq!(resumed.run_id.as_deref(), Some(resumed_attempt_id));
    let events_after_resume = events(&event_store, conversation_id).await;
    assert!(events_after_resume.windows(2).any(|window| {
        matches!(
            (&window[0], &window[1]),
            (
                Event::BackgroundAgentStarted(started),
                Event::BackgroundAgentStateChanged(changed)
            ) if started.background_agent_id.to_string() == record.background_agent_id
                && started.attempt_id.to_string() == resumed_attempt_id
                && changed.attempt_id.map(|attempt_id| attempt_id.to_string()).as_deref()
                    == Some(resumed_attempt_id)
                && changed.to == BackgroundAgentState::Running
        )
    }));
}

#[tokio::test]
async fn agent_orchestration_background_startup_recovery_does_not_mutate_state_when_interrupted_event_append_fails(
) {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let good_event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let good_manager = manager(
        Arc::clone(&store),
        Arc::clone(&good_event_store),
        conversation_id,
    );
    let record = good_manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "startup interrupted event failure".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    let failing_manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        Arc::new(FailingInterruptedEventStore),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(TokenRedactor),
    );

    assert!(matches!(
        failing_manager.recover_on_startup("process restart").await,
        Err(BackgroundAgentTransitionError::Journal(_))
    ));

    let stored = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(stored.state, BackgroundAgentState::Running);
}

#[tokio::test]
async fn agent_orchestration_background_invalid_transitions_fail_closed() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let manager = manager(
        Arc::clone(&store),
        Arc::clone(&event_store),
        SessionId::new(),
    );
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id: SessionId::new(),
            title: "Managed task".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");

    let completed = manager
        .complete(record.background_agent_id.as_str(), "done")
        .await
        .expect("complete");
    assert_eq!(completed.state, BackgroundAgentState::Succeeded);

    assert!(matches!(
        manager
            .pause(completed.background_agent_id.as_str(), "too late")
            .await,
        Err(BackgroundAgentTransitionError::InvalidTransition { .. })
    ));
    assert!(matches!(
        manager
            .delete_archived(completed.background_agent_id.as_str())
            .await,
        Err(BackgroundAgentTransitionError::InvalidTransition { .. })
    ));
}

#[tokio::test]
async fn agent_orchestration_background_startup_recovery_marks_live_process_states_without_deleting_terminal_records(
) {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = manager(
        Arc::clone(&store),
        Arc::clone(&event_store),
        conversation_id,
    );

    let running = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "running".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start running");
    let waiting = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "waiting".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start waiting");
    manager
        .request_input(
            waiting.background_agent_id.as_str(),
            RequestId::new(),
            "Need input",
        )
        .await
        .expect("request input");
    let terminal = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "done".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start terminal");
    manager
        .complete(terminal.background_agent_id.as_str(), "done")
        .await
        .expect("complete");
    let waiting_permission_with_pending = BackgroundAgentId::new().to_string();
    let waiting_permission_bool_only = BackgroundAgentId::new().to_string();
    let waiting_permission_without_pending = BackgroundAgentId::new().to_string();
    let waiting_permission_request_id = RequestId::new();
    for (background_agent_id, payload_json) in [
        (
            waiting_permission_with_pending.as_str(),
            serde_json::json!({
                "backgroundRecovery": {
                    "kind": "permission",
                    "requestId": waiting_permission_request_id.to_string(),
                },
                "pendingPermissionDecision": true,
            })
            .to_string(),
        ),
        (
            waiting_permission_bool_only.as_str(),
            serde_json::json!({"pendingPermissionDecision": true}).to_string(),
        ),
        (waiting_permission_without_pending.as_str(), "{}".to_owned()),
    ] {
        store
            .insert_background_agent(&BackgroundAgentStoreRecord {
                background_agent_id: background_agent_id.to_owned(),
                conversation_id: conversation_id.to_string(),
                run_id: None,
                state: BackgroundAgentState::WaitingForPermission,
                title: "waiting permission".to_owned(),
                created_at: "2026-06-30T00:00:00Z".to_owned(),
                updated_at: "2026-06-30T00:00:00Z".to_owned(),
                payload_json,
            })
            .expect("insert waiting permission");
    }

    let recovered = manager
        .recover_on_startup("process restart")
        .await
        .expect("recover");
    assert_eq!(recovered.len(), 5);
    assert_eq!(
        store
            .get_background_agent(running.background_agent_id.as_str())
            .expect("running lookup")
            .expect("running exists")
            .state,
        BackgroundAgentState::Interrupted
    );
    assert_eq!(
        store
            .get_background_agent(waiting.background_agent_id.as_str())
            .expect("waiting lookup")
            .expect("waiting exists")
            .state,
        BackgroundAgentState::Recoverable
    );
    assert_eq!(
        store
            .get_background_agent(waiting_permission_with_pending.as_str())
            .expect("waiting permission lookup")
            .expect("waiting permission exists")
            .state,
        BackgroundAgentState::Recoverable
    );
    assert_eq!(
        store
            .get_background_agent(waiting_permission_bool_only.as_str())
            .expect("waiting permission lookup")
            .expect("waiting permission exists")
            .state,
        BackgroundAgentState::Interrupted
    );
    assert_eq!(
        store
            .get_background_agent(waiting_permission_without_pending.as_str())
            .expect("waiting permission lookup")
            .expect("waiting permission exists")
            .state,
        BackgroundAgentState::Interrupted
    );
    assert_eq!(
        store
            .get_background_agent(terminal.background_agent_id.as_str())
            .expect("terminal lookup")
            .expect("terminal exists")
            .state,
        BackgroundAgentState::Succeeded
    );

    let attempts_before_resume = store
        .list_background_agent_attempts(running.background_agent_id.as_str())
        .expect("attempts");
    manager
        .resume(running.background_agent_id.as_str(), "resume interrupted")
        .await
        .expect("resume interrupted");
    let attempts_after_resume = store
        .list_background_agent_attempts(running.background_agent_id.as_str())
        .expect("attempts");
    assert_eq!(
        attempts_after_resume.len(),
        attempts_before_resume.len() + 1
    );
    assert_eq!(
        attempts_after_resume
            .last()
            .expect("resumed attempt")
            .prior_attempt_id
            .as_deref(),
        attempts_before_resume
            .last()
            .map(|attempt| attempt.attempt_id.as_str())
    );

    let resumed_waiting = manager
        .resume(
            waiting.background_agent_id.as_str(),
            "resume input recovery",
        )
        .await
        .expect("resume input recovery");
    assert_eq!(resumed_waiting.state, BackgroundAgentState::WaitingForInput);
}
