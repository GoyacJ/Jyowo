#![cfg(feature = "testing")]

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::stream::BoxStream;
use harness_contracts::{
    AssistantMessageCompletedEvent, Event, EventId, ForkReason, JournalError, JournalOffset,
    MessageContent, MessageId, MessageMetadata, NoopRedactor, RunId, SessionId, StopReason,
    TenantId, UsageSnapshot, UserMessageAppendedEvent,
};
use harness_journal::{
    AppendMetadata, EventEnvelope, EventEnvelopePage, EventStore, InMemoryEventStore, PrunePolicy,
    PruneReport, ReplayCursor, SessionFilter, SessionSnapshot, SessionSummary,
};
use jyowo_harness_sdk::{
    testing, ConversationEventsPageRequest, Harness, HarnessOptions, SessionOptions,
};

fn temp_workspace(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("jyowo-sdk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("workspace should be created");
    root
}

fn harness_options(workspace: PathBuf) -> HarnessOptions {
    let mut options = HarnessOptions::default();
    options.workspace_root = workspace;
    options.model_id = "mock-model".to_owned();
    options
}

fn user_message(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    Event::UserMessageAppended(UserMessageAppendedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        metadata: MessageMetadata::default(),
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
    })
}

fn assistant_message(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        tool_uses: Vec::new(),
        usage: UsageSnapshot::default(),
        pricing_snapshot_id: None,
        stop_reason: StopReason::EndTurn,
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
    })
}

struct CountingEventStore {
    inner: InMemoryEventStore,
    read_envelopes_calls: AtomicUsize,
    page_session_envelopes_calls: AtomicUsize,
}

impl CountingEventStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(Arc::new(NoopRedactor)),
            read_envelopes_calls: AtomicUsize::new(0),
            page_session_envelopes_calls: AtomicUsize::new(0),
        }
    }

    fn reset_counts(&self) {
        self.read_envelopes_calls.store(0, Ordering::SeqCst);
        self.page_session_envelopes_calls.store(0, Ordering::SeqCst);
    }

    fn read_envelopes_calls(&self) -> usize {
        self.read_envelopes_calls.load(Ordering::SeqCst)
    }

    fn page_session_envelopes_calls(&self) -> usize {
        self.page_session_envelopes_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl EventStore for CountingEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.inner.append(tenant, session_id, events).await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.inner
            .append_with_metadata(tenant, session_id, metadata, events)
            .await
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        self.read_envelopes_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.read_envelopes(tenant, session_id, cursor).await
    }

    async fn page_session_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        after_event_id: Option<EventId>,
        limit: usize,
    ) -> Result<EventEnvelopePage, JournalError> {
        self.page_session_envelopes_calls
            .fetch_add(1, Ordering::SeqCst);
        self.inner
            .page_session_envelopes(tenant, session_id, after_event_id, limit)
            .await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        self.inner.query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        self.inner.delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        self.inner.prune(tenant, policy).await
    }
}

#[tokio::test]
async fn page_conversation_events_uses_journal_cursor_pushdown() {
    let store = Arc::new(CountingEventStore::new());
    let workspace = temp_workspace("conversation-page-pushdown");
    let harness = Harness::builder()
        .with_options(harness_options(workspace.clone()))
        .with_model(testing::MockProvider::default())
        .with_store_arc(store.clone())
        .with_sandbox(testing::NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");
    let session_id = SessionId::new();
    let options = SessionOptions::new(workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should open");

    store.reset_counts();
    let page = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options,
            after_event_id: None,
            limit: 1,
        })
        .await
        .expect("page should load");

    assert_eq!(page.events.len(), 1);
    assert_eq!(store.page_session_envelopes_calls(), 1);
    assert_eq!(store.read_envelopes_calls(), 0);
}

#[tokio::test]
async fn conversation_read_model_facade_returns_safe_snapshot_and_timeline() {
    let store = Arc::new(CountingEventStore::new());
    let workspace = temp_workspace("conversation-read-model-facade");
    let harness = Harness::builder()
        .with_options(harness_options(workspace.clone()))
        .with_model(testing::MockProvider::default())
        .with_store_arc(store.clone())
        .with_sandbox(testing::NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");
    let session_id = SessionId::new();
    let options = SessionOptions::new(workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options)
        .await
        .expect("session should open");
    let run_id = RunId::new();
    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                user_message(run_id, MessageId::new(), "read /Users/goya/.ssh/config"),
                assistant_message(run_id, MessageId::new(), "Done"),
            ],
        )
        .await
        .expect("messages should append");

    let summaries = harness
        .list_conversation_summaries(TenantId::SINGLE, 10)
        .await
        .expect("summaries should load");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].title.as_str(), "[REDACTED]");
    assert_eq!(
        summaries[0].last_message_preview.as_ref().unwrap().as_str(),
        "Done"
    );

    let snapshot = harness
        .get_conversation_snapshot(&session_id.to_string(), 200)
        .await
        .expect("snapshot should load")
        .expect("snapshot should exist");
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].body.as_str(), "[REDACTED]");
    assert_eq!(snapshot.messages[1].body.as_str(), "Done");

    let first_page = harness
        .page_conversation_timeline(&session_id.to_string(), None, 1)
        .await
        .expect("timeline first page should load");
    assert_eq!(first_page.events.len(), 1);
    let second_page = harness
        .page_conversation_timeline(&session_id.to_string(), first_page.cursor, 10)
        .await
        .expect("timeline second page should load");
    assert_eq!(second_page.events.len(), 1);
    assert_eq!(second_page.events[0].cursor.conversation_sequence, 2);
}
