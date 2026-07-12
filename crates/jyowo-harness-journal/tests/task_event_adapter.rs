#![cfg(all(feature = "sqlite", feature = "blob-file"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    now, BlobId, BlobRef, BudgetMetric, CausationId, ClientId, CommandId, ConfigHash,
    ConversationAttachmentReference, ConversationModelCapability, CorrelationId, EndReason, Event,
    ExecuteCodeStepInvokedEvent, JournalOffset, Message, MessageId, MessagePart, MessageRole,
    ModelProtocol, NoopRedactor, OverflowMetadata, PermissionMode, RedactRules, Redactor,
    RunEndedEvent, RunId, RunModelSnapshot, RunStartedEvent, SessionId, SnapshotId, TaskId,
    TenantId, ToolResultOffloadedEvent, ToolUseId, TurnInput, UnexpectedErrorEvent,
};
use harness_journal::{
    AcceptedCommand, AppendMetadata, EventStore, NewTaskEvent, ReplayCursor, TaskBlobStore,
    TaskEventStoreAdapter, TaskStore,
};
use serde_json::json;

#[tokio::test]
async fn engine_events_share_the_task_log_and_preserve_run_metadata() {
    let database_path = temp_path("adapter");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let session_id = SessionId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );
    let runs = [RunId::new(), RunId::new(), RunId::new()];
    let events = runs
        .into_iter()
        .map(|run_id| {
            Event::RunEnded(RunEndedEvent {
                run_id,
                reason: EndReason::Completed,
                usage: None,
                ended_at: now(),
            })
        })
        .collect::<Vec<_>>();
    let metadata = AppendMetadata {
        run_id: Some(RunId::new()),
        correlation_id: CorrelationId::new(),
        causation_id: Some(CausationId::new()),
    };

    let last_offset = adapter
        .append_with_metadata(TenantId::SINGLE, session_id, metadata, &events)
        .await
        .unwrap();

    assert_eq!(last_offset.0, 2);
    let committed = store.events_after(0, 16).unwrap();
    assert_eq!(
        committed
            .iter()
            .map(|event| event.global_offset)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    for (index, event) in committed.iter().skip(1).enumerate() {
        assert_eq!(event.task_id, task_id);
        assert_eq!(event.event_type, "engine.run_ended");
        assert_eq!(event.payload["sessionId"], session_id.to_string());
        assert_eq!(event.payload["tenantId"], TenantId::SINGLE.to_string());
        assert_eq!(event.payload["journalOffset"], index as u64);
        assert_eq!(event.payload["runId"], metadata.run_id.unwrap().to_string());
        assert_eq!(
            event.payload["correlationId"],
            metadata.correlation_id.to_string()
        );
        assert_eq!(
            event.payload["causationId"],
            metadata.causation_id.unwrap().to_string()
        );
        assert_eq!(event.payload["event"]["run_id"], runs[index].to_string());
    }

    drop((adapter, store));
    cleanup(&database_path);
}

#[tokio::test]
async fn adapter_replays_engine_envelopes_from_the_unified_task_log() {
    let database_path = temp_path("adapter-replay");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let session_id = SessionId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );
    let events = [RunId::new(), RunId::new()].map(|run_id| {
        Event::RunEnded(RunEndedEvent {
            run_id,
            reason: EndReason::Completed,
            usage: None,
            ended_at: now(),
        })
    });
    adapter
        .append(TenantId::SINGLE, session_id, &events)
        .await
        .unwrap();

    let replayed = adapter
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].offset, JournalOffset(0));
    assert_eq!(replayed[1].offset, JournalOffset(1));
    assert_eq!(replayed[0].payload, events[0]);
    assert_eq!(replayed[1].payload, events[1]);

    let after_first = adapter
        .read_envelopes(
            TenantId::SINGLE,
            session_id,
            ReplayCursor::FromOffset(JournalOffset(0)),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(after_first.len(), 1);
    assert_eq!(after_first[0].payload, events[1]);

    let queried = adapter
        .query_after(TenantId::SINGLE, Some(replayed[0].event_id), 10)
        .await
        .unwrap();
    assert_eq!(queried.len(), 1);
    assert_eq!(queried[0].payload, events[1]);

    drop((adapter, store));
    cleanup(&database_path);
}

#[tokio::test]
async fn adapter_enforces_scope_offset_redaction_and_reopen_continuity() {
    let database_path = temp_path("adapter-contract");
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    create_task(&store, task_id);
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(SecretRedactor),
    );
    let event = Event::UnexpectedError(UnexpectedErrorEvent {
        session_id: Some(session_id),
        run_id: None,
        error: "secret value".into(),
        at: now(),
    });

    assert!(adapter
        .append(
            TenantId::SINGLE,
            SessionId::new(),
            std::slice::from_ref(&event)
        )
        .await
        .is_err());
    assert_eq!(store.stream_version(task_id).unwrap(), 1);

    let offset = adapter
        .append_with_metadata_expect_next_offset(
            TenantId::SINGLE,
            session_id,
            AppendMetadata::default(),
            JournalOffset(0),
            std::slice::from_ref(&event),
        )
        .await
        .unwrap();
    assert_eq!(offset, JournalOffset(0));
    assert_eq!(
        store.events_after(0, 8).unwrap()[1].payload["event"]["error"],
        "[REDACTED] value"
    );
    assert!(adapter
        .append_with_metadata_expect_next_offset(
            TenantId::SINGLE,
            session_id,
            AppendMetadata::default(),
            JournalOffset(0),
            std::slice::from_ref(&event),
        )
        .await
        .is_err());

    drop((adapter, store));
    let reopened = Arc::new(TaskStore::open(&database_path).unwrap());
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&reopened),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(SecretRedactor),
    );
    let offset = adapter
        .append_with_metadata_expect_next_offset(
            TenantId::SINGLE,
            session_id,
            AppendMetadata::default(),
            JournalOffset(1),
            &[event],
        )
        .await
        .unwrap();
    assert_eq!(offset, JournalOffset(1));

    drop((adapter, reopened));
    cleanup(&database_path);
}

#[tokio::test]
async fn oversized_engine_batches_are_rejected_before_redaction() {
    let database_path = temp_path("batch-limit");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let redaction_calls = Arc::new(AtomicUsize::new(0));
    let session_id = SessionId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(CountingRedactor(Arc::clone(&redaction_calls))),
    );
    let event = Event::RunEnded(RunEndedEvent {
        run_id: RunId::new(),
        reason: EndReason::Completed,
        usage: None,
        ended_at: now(),
    });

    let result = adapter
        .append(TenantId::SINGLE, session_id, &vec![event; 257])
        .await;

    assert!(result.is_err());
    assert_eq!(redaction_calls.load(Ordering::Relaxed), 0);
    assert_eq!(store.stream_version(task_id).unwrap(), 1);

    drop((adapter, store));
    cleanup(&database_path);
}

struct CountingRedactor(Arc<AtomicUsize>);

impl Redactor for CountingRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        self.0.fetch_add(1, Ordering::Relaxed);
        input.to_owned()
    }
}

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        input.replace("secret", &rules.replacement)
    }
}

#[tokio::test]
async fn engine_blob_references_require_metadata_and_task_ownership() {
    let database_path = temp_path("blob-ownership");
    let blob_root = database_path.with_extension("blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let owner_task = TaskId::new();
    let engine_task = TaskId::new();
    create_task(&store, owner_task);
    create_task(&store, engine_task);
    let owner_blobs = TaskBlobStore::open(Arc::clone(&store), owner_task, &blob_root).unwrap();
    let foreign = owner_blobs.put("text/plain", b"foreign").unwrap();
    let session_id = SessionId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        engine_task,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );

    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[offloaded_event(foreign.clone())],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[execute_code_event(foreign.clone())],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[run_started_event(session_id, foreign.clone())],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[run_started_metadata_event(session_id, foreign.clone())],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let unknown = BlobRef {
        id: BlobId::new(),
        size: 7,
        content_hash: [7; 32],
        content_type: Some("text/plain".into()),
    };
    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[run_started_event(session_id, unknown.clone())],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[run_started_metadata_event(session_id, unknown.clone())],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let result = adapter
        .append(TenantId::SINGLE, session_id, &[offloaded_event(unknown)])
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    let result = adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[run_started_with_input(
                session_id,
                vec![MessagePart::Text("invalid metadata".into())],
                json!({ "attachments": { "not": "a list" } }),
            )],
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.stream_version(engine_task).unwrap(), 1);

    drop((adapter, owner_blobs, store));
    cleanup(&database_path);
    let _ = std::fs::remove_dir_all(blob_root);
}

#[tokio::test]
async fn engine_blob_reference_metadata_must_match_the_task_blob() {
    let database_path = temp_path("blob-reference-metadata");
    let blob_root = database_path.with_extension("blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();
    let reference = blobs.put("text/plain", b"owned").unwrap();
    let session_id = SessionId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );
    let mut tampered = reference.clone();
    tampered.size += 1;

    let result = adapter
        .append(TenantId::SINGLE, session_id, &[offloaded_event(tampered)])
        .await;

    assert!(result.is_err());
    assert_eq!(store.stream_version(task_id).unwrap(), 1);
    assert_eq!(
        adapter
            .append(TenantId::SINGLE, session_id, &[offloaded_event(reference)],)
            .await
            .unwrap(),
        JournalOffset(0)
    );
    assert_eq!(store.stream_version(task_id).unwrap(), 2);

    drop((adapter, blobs, store));
    cleanup(&database_path);
    let _ = std::fs::remove_dir_all(blob_root);
}

fn offloaded_event(blob_ref: BlobRef) -> Event {
    Event::ToolResultOffloaded(ToolResultOffloadedEvent {
        tool_use_id: ToolUseId::new(),
        run_id: RunId::new(),
        blob_ref,
        original_metric: BudgetMetric::Bytes,
        original_size: 7,
        effective_limit: 7,
        head_chars: 0,
        tail_chars: 0,
        at: now(),
    })
}

fn execute_code_event(blob_ref: BlobRef) -> Event {
    Event::ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent {
        parent_tool_use_id: ToolUseId::new(),
        run_id: RunId::new(),
        session_id: SessionId::new(),
        embedded_tool: "read_file".into(),
        args_hash: [1; 32],
        step_seq: 1,
        duration_ms: 1,
        overflow: Some(OverflowMetadata {
            blob_ref,
            head_chars: 0,
            tail_chars: 0,
            original_size: 7,
            original_metric: BudgetMetric::Bytes,
            effective_limit: 7,
        }),
        refused_reason: None,
        at: now(),
    })
}

fn run_started_event(session_id: SessionId, blob_ref: BlobRef) -> Event {
    run_started_with_input(
        session_id,
        vec![MessagePart::Image {
            mime_type: "image/png".into(),
            blob_ref,
        }],
        serde_json::Value::Null,
    )
}

fn run_started_metadata_event(session_id: SessionId, blob_ref: BlobRef) -> Event {
    let attachment = ConversationAttachmentReference {
        id: "attachment-1".into(),
        name: "attachment.txt".into(),
        mime_type: "text/plain".into(),
        size_bytes: blob_ref.size,
        blob_ref,
    };
    run_started_with_input(
        session_id,
        vec![MessagePart::Text("attachment".into())],
        json!({ "attachments": [attachment] }),
    )
}

fn run_started_with_input(
    session_id: SessionId,
    parts: Vec<MessagePart>,
    metadata: serde_json::Value,
) -> Event {
    Event::RunStarted(RunStartedEvent {
        run_id: RunId::new(),
        session_id,
        tenant_id: TenantId::SINGLE,
        parent_run_id: None,
        model: RunModelSnapshot {
            model_config_id: None,
            provider_id: "test-provider".into(),
            model_id: "test-model".into(),
            display_name: "Test Model".into(),
            protocol: ModelProtocol::Messages,
            context_window: 128_000,
            max_output_tokens: 8_192,
            conversation_capability: ConversationModelCapability::default(),
        },
        input: TurnInput {
            message: Message {
                id: MessageId::new(),
                role: MessageRole::User,
                parts,
                created_at: now(),
            },
            metadata,
        },
        snapshot_id: SnapshotId::new(),
        effective_config_hash: ConfigHash([0; 32]),
        started_at: now(),
        correlation_id: CorrelationId::new(),
        permission_mode: PermissionMode::Default,
    })
}

fn create_task(store: &TaskStore, task_id: TaskId) {
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("create-{}", CommandId::new()),
                expected_stream_version: 0,
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "create": true }),
            },
            |_| Ok(vec![NewTaskEvent::task_created("Adapter")]),
        )
        .unwrap();
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-event-{name}-{}-{}.db",
        std::process::id(),
        TaskId::new()
    ))
}

fn cleanup(database_path: &std::path::Path) {
    for suffix in ["", "-shm", "-wal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", database_path.display()));
    }
}
