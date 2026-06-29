use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::*;
use harness_journal::*;

fn temp_root(name: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time is after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jyowo-journal-contract-{name}-{}-{nonce}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn event(text: &str) -> Event {
    Event::UnexpectedError(UnexpectedErrorEvent {
        session_id: None,
        run_id: None,
        error: text.to_owned(),
        at: harness_contracts::now(),
    })
}

fn snapshot(session_id: SessionId) -> SessionSnapshot {
    SessionSnapshot {
        session_id,
        tenant_id: TenantId::SINGLE,
        offset: JournalOffset(0),
        taken_at: harness_contracts::now(),
        body: SnapshotBody::Full(vec![1, 2, 3]),
    }
}

#[tokio::test]
async fn event_store_authorizes_only_current_run_offloaded_blobs() {
    let store: Arc<dyn EventStore> = Arc::new(OffloadedBlobAuthorizerStore::default());
    let authorizer = EventStoreOffloadedBlobAuthorizer::new(store.clone());
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run_id = RunId::new();
    let blob_ref = BlobRef {
        id: BlobId::new(),
        size: 11,
        content_hash: [4; 32],
        content_type: Some("text/plain".to_owned()),
    };
    store
        .append(
            tenant_id,
            session_id,
            &[Event::ToolResultOffloaded(ToolResultOffloadedEvent {
                tool_use_id: ToolUseId::new(),
                run_id,
                blob_ref: blob_ref.clone(),
                original_metric: BudgetMetric::Chars,
                original_size: 11,
                effective_limit: 5,
                head_chars: 2,
                tail_chars: 2,
                at: harness_contracts::now(),
            })],
        )
        .await
        .expect("append succeeds");

    authorizer
        .authorize_offloaded_blob(tenant_id, session_id, run_id, blob_ref.clone())
        .await
        .expect("current run offloaded blob is allowed");
    let error = authorizer
        .authorize_offloaded_blob(tenant_id, session_id, other_run_id, blob_ref)
        .await
        .expect_err("other run is denied");

    assert!(matches!(error, ToolError::PermissionDenied(_)));
}

#[derive(Default)]
struct OffloadedBlobAuthorizerStore {
    envelopes: Mutex<Vec<EventEnvelope>>,
}

#[async_trait]
impl EventStore for OffloadedBlobAuthorizerStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let mut envelopes = self
            .envelopes
            .lock()
            .map_err(|_| JournalError::Message("event store lock poisoned".to_owned()))?;
        for event in events {
            let offset = JournalOffset(envelopes.len() as u64);
            envelopes.push(EventEnvelope {
                offset,
                event_id: EventId::new(),
                session_id,
                tenant_id: tenant,
                run_id: None,
                correlation_id: CorrelationId::new(),
                causation_id: None,
                schema_version: SchemaVersion::CURRENT,
                recorded_at: harness_contracts::now(),
                payload: event.clone(),
            });
        }

        Ok(envelopes
            .last()
            .map(|envelope| envelope.offset)
            .unwrap_or(JournalOffset(0)))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<futures::stream::BoxStream<'static, EventEnvelope>, JournalError> {
        let envelopes = self
            .envelopes
            .lock()
            .map_err(|_| JournalError::Message("event store lock poisoned".to_owned()))?
            .iter()
            .filter(|envelope| envelope.tenant_id == tenant && envelope.session_id == session_id)
            .filter(|envelope| match cursor {
                ReplayCursor::FromStart => true,
                ReplayCursor::FromOffset(offset) => envelope.offset > offset,
                _ => false,
            })
            .cloned()
            .collect::<Vec<_>>();

        Ok(stream::iter(envelopes).boxed())
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let mut envelopes = self
            .envelopes
            .lock()
            .map_err(|_| JournalError::Message("event store lock poisoned".to_owned()))?
            .iter()
            .filter(|envelope| envelope.tenant_id == tenant)
            .cloned()
            .collect::<Vec<_>>();
        if let Some(after) = after {
            if let Some(index) = envelopes
                .iter()
                .position(|envelope| envelope.event_id == after)
            {
                envelopes.drain(..=index);
            }
        }
        envelopes.truncate(limit);
        Ok(envelopes)
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

async fn run_contract<S: EventStore>(store: &S) {
    let session = SessionId::new();
    let last_offset = store
        .append(
            TenantId::SINGLE,
            session,
            &[event("first"), event("second")],
        )
        .await
        .expect("append succeeds");
    assert_eq!(last_offset, JournalOffset(1));

    let replayed: Vec<_> = store
        .read(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read succeeds")
        .collect()
        .await;
    assert_eq!(replayed.len(), 2);

    let envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read envelopes succeeds")
        .collect()
        .await;
    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[0].offset, JournalOffset(0));
    assert_eq!(envelopes[1].offset, JournalOffset(1));

    let first_page = store
        .page_session_envelopes(TenantId::SINGLE, session, None, 1)
        .await
        .expect("first page succeeds");
    assert_eq!(first_page.envelopes.len(), 1);
    assert_eq!(first_page.envelopes[0].event_id, envelopes[0].event_id);
    assert_eq!(first_page.next_event_id, Some(envelopes[0].event_id));

    let second_page = store
        .page_session_envelopes(
            TenantId::SINGLE,
            session,
            Some(first_page.next_event_id.expect("cursor exists")),
            10,
        )
        .await
        .expect("second page succeeds");
    assert_eq!(second_page.envelopes.len(), 1);
    assert_eq!(second_page.envelopes[0].event_id, envelopes[1].event_id);
    assert_eq!(second_page.next_event_id, Some(envelopes[1].event_id));

    let unknown_cursor_error = store
        .page_session_envelopes(TenantId::SINGLE, session, Some(EventId::new()), 10)
        .await
        .expect_err("unknown cursor fails closed");
    assert!(unknown_cursor_error
        .to_string()
        .contains("cursor is unknown"));

    let queried = store
        .query_after(TenantId::SINGLE, None, 10)
        .await
        .expect("query after start succeeds");
    assert!(queried
        .iter()
        .any(|envelope| envelope.event_id == envelopes[0].event_id));
    assert!(queried
        .iter()
        .any(|envelope| envelope.event_id == envelopes[1].event_id));

    let queried_after_first = store
        .query_after(TenantId::SINGLE, Some(envelopes[0].event_id), 10)
        .await
        .expect("query after event succeeds");
    assert!(queried_after_first
        .iter()
        .all(|envelope| envelope.event_id != envelopes[0].event_id));
    assert!(queried_after_first
        .iter()
        .any(|envelope| envelope.event_id == envelopes[1].event_id));

    let after_first: Vec<_> = store
        .read(
            TenantId::SINGLE,
            session,
            ReplayCursor::FromOffset(JournalOffset(0)),
        )
        .await
        .expect("cursor read succeeds")
        .collect()
        .await;
    assert!(matches!(
        &after_first[..],
        [Event::UnexpectedError(UnexpectedErrorEvent { error, .. })] if error == "second"
    ));

    let saved = snapshot(session);
    store
        .save_snapshot(TenantId::SINGLE, saved.clone())
        .await
        .expect("snapshot saves");
    assert_eq!(
        store
            .snapshot(TenantId::SINGLE, session)
            .await
            .expect("snapshot loads"),
        Some(saved)
    );

    let deleted = SessionId::new();
    store
        .append(TenantId::SINGLE, deleted, &[event("delete me")])
        .await
        .expect("deleted session append succeeds");
    store
        .save_snapshot(TenantId::SINGLE, snapshot(deleted))
        .await
        .expect("deleted session snapshot saves");
    assert!(store
        .delete_session(TenantId::SINGLE, deleted)
        .await
        .expect("delete succeeds"));
    let deleted_replay: Vec<_> = store
        .read(TenantId::SINGLE, deleted, ReplayCursor::FromStart)
        .await
        .expect("deleted session read succeeds")
        .collect()
        .await;
    assert!(deleted_replay.is_empty());
    assert_eq!(
        store
            .snapshot(TenantId::SINGLE, deleted)
            .await
            .expect("deleted snapshot lookup succeeds"),
        None
    );
    assert!(!store
        .list_sessions(
            TenantId::SINGLE,
            SessionFilter {
                since: None,
                end_reason: None,
                project_compression_tips: false,
                limit: 10,
            },
        )
        .await
        .expect("sessions list after delete")
        .iter()
        .any(|summary| summary.session_id == deleted));
    assert!(!store
        .delete_session(TenantId::SINGLE, deleted)
        .await
        .expect("delete is idempotent"));

    let sessions = store
        .list_sessions(
            TenantId::SINGLE,
            SessionFilter {
                since: None,
                end_reason: None,
                project_compression_tips: false,
                limit: 10,
            },
        )
        .await
        .expect("sessions list");
    assert!(sessions.iter().any(|summary| summary.session_id == session));

    let ended = SessionId::new();
    store
        .append(
            TenantId::SINGLE,
            ended,
            &[Event::SessionEnded(SessionEndedEvent {
                session_id: ended,
                tenant_id: TenantId::SINGLE,
                reason: EndReason::Completed,
                final_usage: UsageSnapshot::default(),
                at: harness_contracts::now(),
            })],
        )
        .await
        .expect("ended session append succeeds");
    let ended_sessions = store
        .list_sessions(
            TenantId::SINGLE,
            SessionFilter {
                since: None,
                end_reason: Some(EndReason::Completed),
                project_compression_tips: false,
                limit: 10,
            },
        )
        .await
        .expect("ended sessions list");
    assert_eq!(ended_sessions.len(), 1);
    assert_eq!(ended_sessions[0].session_id, ended);
    assert_eq!(ended_sessions[0].end_reason, Some(EndReason::Completed));

    let parent = SessionId::new();
    let child = SessionId::new();
    store
        .append(TenantId::SINGLE, parent, &[event("parent")])
        .await
        .expect("parent append succeeds");
    store
        .append(TenantId::SINGLE, child, &[event("child")])
        .await
        .expect("child append succeeds");
    store
        .compact_link(parent, child, ForkReason::Compaction)
        .await
        .expect("compact link succeeds");
    let compressed = store
        .list_sessions(
            TenantId::SINGLE,
            SessionFilter {
                since: None,
                end_reason: None,
                project_compression_tips: true,
                limit: 10,
            },
        )
        .await
        .expect("compressed sessions list");
    let child_summary = compressed
        .iter()
        .find(|summary| summary.session_id == child)
        .expect("child tip is listed");
    assert_eq!(child_summary.root_session, Some(parent));
    assert!(!compressed
        .iter()
        .any(|summary| summary.session_id == parent));

    let prune_report = store
        .prune(
            TenantId::SINGLE,
            PrunePolicy {
                older_than: Duration::ZERO,
                keep_snapshots: false,
                keep_latest_n_sessions: Some(1),
                target_size_bytes: None,
            },
        )
        .await
        .expect("prune succeeds");
    assert!(prune_report.events_removed > 0);
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_satisfies_event_store_contract() {
    let store = JsonlEventStore::open(temp_root("jsonl"), Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    run_contract(&store).await;
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_pages_segment_files_by_numeric_offset_after_cursor() {
    let store = JsonlEventStore::open(temp_root("jsonl-pagination"), Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    let session = SessionId::new();

    for offset in 0..14 {
        store
            .append(
                TenantId::SINGLE,
                session,
                &[event(&format!("event {offset}"))],
            )
            .await
            .expect("append succeeds");
    }

    let envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read envelopes succeeds")
        .collect()
        .await;
    assert_eq!(envelopes.len(), 14);
    assert_eq!(envelopes[9].offset, JournalOffset(9));

    let page = store
        .page_session_envelopes(TenantId::SINGLE, session, Some(envelopes[9].event_id), 10)
        .await
        .expect("page after offset 9 succeeds");

    let offsets: Vec<_> = page
        .envelopes
        .iter()
        .map(|envelope| envelope.offset)
        .collect();
    assert_eq!(
        offsets,
        vec![
            JournalOffset(10),
            JournalOffset(11),
            JournalOffset(12),
            JournalOffset(13),
        ]
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_satisfies_event_store_contract() {
    let root = temp_root("sqlite");
    std::fs::create_dir_all(&root).expect("root exists");
    let store = SqliteEventStore::open(root.join("events.db"), Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    run_contract(&store).await;
}

#[cfg(feature = "in-memory")]
#[tokio::test]
async fn memory_store_satisfies_event_store_contract() {
    let store = InMemoryEventStore::new(Arc::new(NoopRedactor));
    run_contract(&store).await;
}

#[cfg(feature = "testing")]
#[tokio::test]
async fn test_store_satisfies_event_store_contract() {
    let store = test_event_store(Arc::new(NoopRedactor));
    run_contract(&store).await;
}
