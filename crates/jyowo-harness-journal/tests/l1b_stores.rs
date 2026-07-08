use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;
use harness_contracts::*;
use harness_journal::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("jyowo-journal-{name}-{}", std::process::id()));
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

fn blob_meta(bytes: &[u8]) -> BlobMeta {
    BlobMeta {
        content_type: Some("text/plain".to_owned()),
        size: bytes.len() as u64,
        content_hash: *blake3::hash(bytes).as_bytes(),
        created_at: harness_contracts::now(),
        retention: BlobRetention::RetainForever,
    }
}

fn wrong_blob_meta(bytes: &[u8]) -> BlobMeta {
    BlobMeta {
        content_type: Some("text/plain".to_owned()),
        size: bytes.len() as u64,
        content_hash: [0; 32],
        created_at: harness_contracts::now(),
        retention: BlobRetention::RetainForever,
    }
}

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        input.replace("secret", &rules.replacement)
    }
}

async fn assert_event_store_contract<S: EventStore>(store: &S) {
    let session = SessionId::new();
    assert_eq!(
        store
            .append(
                TenantId::SINGLE,
                session,
                &[event("first"), event("second")]
            )
            .await
            .expect("append succeeds"),
        JournalOffset(1)
    );

    let replayed: Vec<_> = store
        .read(
            TenantId::SINGLE,
            session,
            ReplayCursor::FromOffset(JournalOffset(0)),
        )
        .await
        .expect("read succeeds")
        .collect()
        .await;
    assert_eq!(replayed.len(), 1);
    assert!(matches!(
        &replayed[0],
        Event::UnexpectedError(UnexpectedErrorEvent { error, .. }) if error == "second"
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
}

async fn assert_event_store_rejects_stale_expected_next_offset<S: EventStore>(store: &S) {
    let session = SessionId::new();
    store
        .append(TenantId::SINGLE, session, &[event("first")])
        .await
        .expect("initial append succeeds");

    let error = store
        .append_with_metadata_expect_next_offset(
            TenantId::SINGLE,
            session,
            AppendMetadata::default(),
            JournalOffset(0),
            &[event("stale")],
        )
        .await
        .expect_err("stale writer is rejected");
    assert!(
        error.to_string().contains("expected next offset"),
        "unexpected error: {error}"
    );

    let offset = store
        .append_with_metadata_expect_next_offset(
            TenantId::SINGLE,
            session,
            AppendMetadata::default(),
            JournalOffset(1),
            &[event("second")],
        )
        .await
        .expect("fresh writer appends");
    assert_eq!(offset, JournalOffset(1));

    let replayed: Vec<_> = store
        .read(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read succeeds")
        .collect()
        .await;
    assert_eq!(replayed.len(), 2);
    assert!(matches!(
        &replayed[1],
        Event::UnexpectedError(UnexpectedErrorEvent { error, .. }) if error == "second"
    ));
}

#[cfg(feature = "jsonl")]
fn read_session_jsonl(root: &Path, session: SessionId) -> String {
    let dir = root.join(TenantId::SINGLE.to_string());
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries {
            let path = entry.expect("dir entry reads").path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == format!("{session}.jsonl")
                || (name.starts_with(&format!("{session}.")) && name.ends_with(".jsonl"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|path| std::fs::read_to_string(path).expect("jsonl reads"))
        .collect()
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_redacts_and_replays() {
    let root = temp_root("jsonl");
    let store = JsonlEventStore::open(&root, Arc::new(SecretRedactor))
        .await
        .expect("store opens");
    let session = SessionId::new();

    store
        .append(TenantId::SINGLE, session, &[event("token=secret")])
        .await
        .expect("append succeeds");

    let raw = read_session_jsonl(&root, session);
    assert!(!raw.contains("secret"));
    assert!(raw.contains("[REDACTED]"));
    assert_event_store_contract(&store).await;
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_rejects_stale_expected_next_offset() {
    let root = temp_root("jsonl-expected-next-offset");
    let store = JsonlEventStore::open(&root, Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    assert_event_store_rejects_stale_expected_next_offset(&store).await;
}

#[cfg(all(unix, feature = "jsonl"))]
#[tokio::test]
async fn jsonl_store_rejects_symlink_root() {
    let root = temp_root("jsonl-symlink-root");
    let external = temp_root("jsonl-symlink-external");
    std::fs::create_dir_all(&external).expect("external dir");
    std::os::unix::fs::symlink(&external, &root).expect("symlink");

    let error = match JsonlEventStore::open(&root, Arc::new(NoopRedactor)).await {
        Ok(_) => panic!("symlink root should fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("symlink"));
}

#[cfg(all(unix, feature = "jsonl"))]
#[tokio::test]
async fn jsonl_store_rejects_symlink_segment_file() {
    let root = temp_root("jsonl-symlink-segment");
    let store = JsonlEventStore::open(&root, Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    let session = SessionId::new();
    let tenant_dir = root.join(TenantId::SINGLE.to_string());
    std::fs::create_dir_all(&tenant_dir).expect("tenant dir");
    let external = root.join("external.jsonl");
    std::fs::write(&external, "{}\n").expect("external event file");
    std::os::unix::fs::symlink(&external, tenant_dir.join(format!("{session}.0.jsonl")))
        .expect("symlink");

    let error = match store
        .read(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
    {
        Ok(_) => panic!("symlink segment should fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("symlink"));
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_persists_snapshots_across_reopen() {
    let root = temp_root("jsonl-snapshot");
    let session = SessionId::new();
    let saved = snapshot(session);

    JsonlEventStore::open(&root, Arc::new(NoopRedactor))
        .await
        .expect("store opens")
        .save_snapshot(TenantId::SINGLE, saved.clone())
        .await
        .expect("snapshot saves");

    let reopened = JsonlEventStore::open(&root, Arc::new(NoopRedactor))
        .await
        .expect("store reopens");
    assert_eq!(
        reopened
            .snapshot(TenantId::SINGLE, session)
            .await
            .expect("snapshot reloads"),
        Some(saved)
    );
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_replays_rotated_segments() {
    let root = temp_root("jsonl-rotation");
    let session = SessionId::new();
    let store = JsonlEventStore::open_with_options(
        &root,
        Arc::new(NoopRedactor),
        JsonlOptions {
            rotation: JsonlRotationPolicy { max_bytes: 1 },
            ..JsonlOptions::default()
        },
    )
    .await
    .expect("store opens");

    store
        .append(TenantId::SINGLE, session, &[event("first")])
        .await
        .expect("first append succeeds");
    store
        .append(TenantId::SINGLE, session, &[event("second")])
        .await
        .expect("second append succeeds");

    let replayed: Vec<_> = store
        .read(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read succeeds")
        .collect()
        .await;
    assert_eq!(replayed.len(), 2);
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_store_writes_append_batches_as_committed_segments() {
    let root = temp_root("jsonl-batch");
    let session = SessionId::new();
    let store = JsonlEventStore::open(&root, Arc::new(NoopRedactor))
        .await
        .expect("store opens");

    store
        .append(
            TenantId::SINGLE,
            session,
            &[event("first"), event("second")],
        )
        .await
        .expect("batch append succeeds");

    let tenant_dir = root.join(TenantId::SINGLE.to_string());
    let segment = tenant_dir.join(format!("{session}.0.jsonl"));
    assert!(segment.exists());
    assert!(!tenant_dir.join(format!("{session}.0.tmp")).exists());
    let lines = std::fs::read_to_string(segment).expect("segment reads");
    assert_eq!(lines.lines().count(), 2);

    std::fs::write(tenant_dir.join(format!("{session}.99.tmp")), b"{not-json\n")
        .expect("tmp writes");
    let replayed: Vec<_> = store
        .read(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("tmp segment is ignored")
        .collect()
        .await;
    assert_eq!(replayed.len(), 2);
}

#[cfg(feature = "jsonl")]
#[tokio::test]
async fn jsonl_prune_sessions_deletes_only_supplied_sessions() {
    let root = temp_root("jsonl-prune-sessions");
    let store = JsonlEventStore::open(&root, Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    let deleted = SessionId::new();
    let retained = SessionId::new();

    store
        .append(TenantId::SINGLE, deleted, &[event("delete me")])
        .await
        .expect("deleted session appends");
    store
        .append(TenantId::SINGLE, retained, &[event("keep me")])
        .await
        .expect("retained session appends");

    let report = store
        .prune_sessions(TenantId::SINGLE, &[deleted], false)
        .await
        .expect("exact-session prune succeeds");

    assert_eq!(report.events_removed, 1);
    assert!(read_session_jsonl(&root, deleted).is_empty());
    let retained_events: Vec<_> = store
        .read(TenantId::SINGLE, retained, ReplayCursor::FromStart)
        .await
        .expect("retained session reads")
        .collect()
        .await;
    assert_eq!(retained_events.len(), 1);
    assert!(matches!(
        &retained_events[0],
        Event::UnexpectedError(UnexpectedErrorEvent { error, .. }) if error == "keep me"
    ));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_satisfies_contract() {
    let root = temp_root("sqlite");
    std::fs::create_dir_all(&root).expect("root created");
    let store = SqliteEventStore::open(root.join("events.db"), Arc::new(SecretRedactor))
        .await
        .expect("store opens");
    assert_event_store_contract(&store).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_rejects_stale_expected_next_offset() {
    let root = temp_root("sqlite-expected-next-offset");
    std::fs::create_dir_all(&root).expect("root created");
    let store = SqliteEventStore::open(root.join("events.db"), Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    assert_event_store_rejects_stale_expected_next_offset(&store).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_rejects_old_journal_marker_column() {
    let root = temp_root("sqlite-old-journal-marker");
    std::fs::create_dir_all(&root).expect("root created");
    let db = root.join("events.db");
    {
        let connection = rusqlite::Connection::open(&db).expect("old database opens");
        let stale_marker_column = ["schema", "version"].join("_");
        let create_sql = format!(
            "CREATE TABLE events (
                tenant_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                offset INTEGER NOT NULL,
                event_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                recorded_at TEXT NOT NULL,
                correlation_id TEXT,
                causation_id TEXT,
                {stale_marker_column} INTEGER NOT NULL DEFAULT 1,
                body TEXT NOT NULL,
                PRIMARY KEY (tenant_id, session_id, offset)
             ) STRICT;"
        );
        connection
            .execute_batch(&create_sql)
            .expect("old events table is created");
    }

    let error = match SqliteEventStore::open(&db, Arc::new(NoopRedactor)).await {
        Ok(_) => panic!("old journal marker column is rejected"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("unsupported journal store shape"),
        "unexpected error: {error}"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_initializes_fts_and_filters_end_reason() {
    let root = temp_root("sqlite-fts");
    std::fs::create_dir_all(&root).expect("root created");
    let db = root.join("events.db");
    let store = SqliteEventStore::open(&db, Arc::new(NoopRedactor))
        .await
        .expect("store opens");
    let completed = SessionId::new();
    let errored = SessionId::new();

    store
        .append(
            TenantId::SINGLE,
            completed,
            &[
                event("completed"),
                Event::SessionEnded(SessionEndedEvent {
                    session_id: completed,
                    tenant_id: TenantId::SINGLE,
                    reason: EndReason::Completed,
                    final_usage: UsageSnapshot::default(),
                    at: harness_contracts::now(),
                }),
            ],
        )
        .await
        .expect("completed session append succeeds");
    store
        .append(
            TenantId::SINGLE,
            errored,
            &[Event::SessionEnded(SessionEndedEvent {
                session_id: errored,
                tenant_id: TenantId::SINGLE,
                reason: EndReason::Error("boom".to_owned()),
                final_usage: UsageSnapshot::default(),
                at: harness_contracts::now(),
            })],
        )
        .await
        .expect("errored session append succeeds");

    let fts_count: i64 = rusqlite::Connection::open(&db)
        .expect("db opens")
        .query_row("SELECT COUNT(*) FROM events_fts", [], |row| row.get(0))
        .expect("fts count loads");
    assert!(fts_count >= 3);

    let completed_sessions = store
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
        .expect("sessions list");
    assert_eq!(completed_sessions.len(), 1);
    assert_eq!(completed_sessions[0].session_id, completed);
    assert_eq!(completed_sessions[0].end_reason, Some(EndReason::Completed));
}

#[cfg(feature = "in-memory")]
#[tokio::test]
async fn memory_store_satisfies_contract_and_is_not_persistent() {
    let store = InMemoryEventStore::new(Arc::new(SecretRedactor));
    assert_event_store_contract(&store).await;
    let fresh = InMemoryEventStore::new(Arc::new(SecretRedactor));
    assert!(fresh
        .list_sessions(
            TenantId::SINGLE,
            SessionFilter {
                since: None,
                end_reason: None,
                project_compression_tips: false,
                limit: 10,
            }
        )
        .await
        .expect("list succeeds")
        .is_empty());
}

#[cfg(feature = "in-memory")]
#[tokio::test]
async fn memory_store_rejects_stale_expected_next_offset() {
    let store = InMemoryEventStore::new(Arc::new(NoopRedactor));
    assert_event_store_rejects_stale_expected_next_offset(&store).await;
}

#[tokio::test]
async fn file_and_memory_blob_stores_round_trip_bytes() {
    let bytes = Bytes::from_static(b"hello blob");
    let meta = blob_meta(&bytes);
    let tenant = TenantId::SINGLE;
    let file_store = FileBlobStore::open(temp_root("blob-file")).expect("file store opens");
    let memory_store = InMemoryBlobStore::default();

    for store in [
        &file_store as &dyn BlobStore,
        &memory_store as &dyn BlobStore,
    ] {
        let blob = store
            .put(tenant, bytes.clone(), meta.clone())
            .await
            .expect("put succeeds");
        assert_eq!(
            store.head(tenant, &blob).await.expect("head succeeds"),
            Some(meta.clone())
        );
        let collected: Vec<_> = store
            .get(tenant, &blob)
            .await
            .expect("get succeeds")
            .collect()
            .await;
        assert_eq!(collected, vec![bytes.clone()]);
        store.delete(tenant, &blob).await.expect("delete succeeds");
        assert_eq!(
            store.head(tenant, &blob).await.expect("head succeeds"),
            None
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn file_blob_store_rejects_symlink_root() {
    let root = temp_root("blob-symlink-root");
    let external = temp_root("blob-symlink-external");
    std::fs::create_dir_all(&external).expect("external dir");
    std::os::unix::fs::symlink(&external, &root).expect("symlink");

    let error = FileBlobStore::open(&root).expect_err("symlink root should fail");

    assert!(error.to_string().contains("symlink"));
}

#[cfg(unix)]
#[tokio::test]
async fn file_blob_store_rejects_symlink_metadata_file() {
    let root = temp_root("blob-symlink-metadata");
    let store = FileBlobStore::open(&root).expect("store opens");
    let id = BlobId::new();
    let id_text = id.to_string();
    let meta_dir = root.join(TenantId::SINGLE.to_string()).join(&id_text[..2]);
    std::fs::create_dir_all(&meta_dir).expect("meta dir");
    let external = root.join("external.meta.json");
    std::fs::write(&external, "{}").expect("external meta");
    std::os::unix::fs::symlink(&external, meta_dir.join(format!("{id}.meta.json")))
        .expect("symlink");

    let error = store
        .inventory(TenantId::SINGLE)
        .expect_err("symlink meta should fail");

    assert!(error.to_string().contains("symlink"));
}

#[tokio::test]
async fn file_blob_store_rejects_hash_mismatch_and_uses_prefixed_paths() {
    let bytes = Bytes::from_static(b"hello blob");
    let tenant = TenantId::SINGLE;
    let root = temp_root("blob-prefixed");
    let file_store = FileBlobStore::open(&root).expect("file store opens");

    let error = file_store
        .put(tenant, bytes.clone(), wrong_blob_meta(&bytes))
        .await
        .expect_err("hash mismatch fails");
    assert!(matches!(error, BlobError::HashMismatch { .. }));

    let blob = file_store
        .put(tenant, bytes.clone(), blob_meta(&bytes))
        .await
        .expect("put succeeds");
    let prefix = blob.id.to_string()[..2].to_owned();
    assert!(root
        .join(tenant.to_string())
        .join(prefix)
        .join(format!("{}.bin", blob.id))
        .exists());
}

#[tokio::test]
async fn retention_enforcer_collects_unreferenced_expired_file_blobs() {
    let tenant = TenantId::SINGLE;
    let root = temp_root("blob-gc");
    let store = FileBlobStore::open(&root).expect("file store opens");
    let evidence_blob_store = FileBlobStore::open(&root).expect("evidence blob store handle opens");
    let expired = Bytes::from_static(b"expired");
    let live = Bytes::from_static(b"live");

    let mut expired_meta = blob_meta(&expired);
    expired_meta.retention = BlobRetention::TtlDays(0);
    let expired_ref = store
        .put(tenant, expired.clone(), expired_meta)
        .await
        .expect("expired blob writes");

    let mut live_meta = blob_meta(&live);
    live_meta.retention = BlobRetention::TtlDays(0);
    let live_ref = store
        .put(tenant, live.clone(), live_meta)
        .await
        .expect("live blob writes");

    let evidence_store = EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::default()),
        Arc::new(evidence_blob_store),
    );
    evidence_store
        .store_existing_blob_evidence(
            tenant,
            EvidenceRefRecord {
                id: EvidenceRefId::new("live-ref"),
                kind: EvidenceRefKind::CommandOutput,
                conversation_id: "conversation-live".to_owned(),
                run_id: "run-live".to_owned(),
                source_event_refs: Vec::new(),
                artifact_id: None,
                revision_id: None,
                content_type: "application/octet-stream".to_owned(),
                byte_length: live.len() as u64,
                content_hash: live_ref.content_hash.to_vec(),
                redaction_state: EvidenceRedactionState::Clean,
                redaction_provenance: RedactionProvenance {
                    redactor_version: "test".to_owned(),
                },
                retention: BlobRetention::TtlDays(0),
                source: EvidenceRefSource::Blob {
                    blob_ref: live_ref.clone(),
                },
            },
        )
        .await
        .expect("live evidence registers");
    let report = RetentionEnforcer::default()
        .collect_garbage(tenant, &store, &evidence_store)
        .await
        .expect("gc succeeds");

    assert_eq!(report.scanned, 2);
    assert_eq!(report.deleted, 1);
    assert!(report.freed_bytes >= expired.len() as u64);
    assert_eq!(
        store
            .head(tenant, &expired_ref)
            .await
            .expect("expired head succeeds"),
        None
    );
    assert!(store
        .head(tenant, &live_ref)
        .await
        .expect("live head succeeds")
        .is_some());
}

#[cfg(all(unix, feature = "sqlite"))]
#[tokio::test]
async fn sqlite_evidence_registry_rejects_symlink_database_file() {
    let root = temp_root("evidence-symlink-file");
    std::fs::create_dir_all(&root).expect("root created");
    let external = root.join("external.sqlite");
    std::fs::write(&external, "").expect("external file");
    let database_path = root.join("evidence.sqlite");
    std::os::unix::fs::symlink(&external, &database_path).expect("symlink");

    let error = match SqliteEvidenceRefRegistry::open(&database_path).await {
        Ok(_) => panic!("symlink sqlite file should fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("symlink"));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn hybrid_blob_store_routes_by_size() {
    let root = temp_root("hybrid-blob");
    std::fs::create_dir_all(&root).expect("root created");
    let sqlite = SqliteBlobStore::open(root.join("blobs.db")).expect("sqlite blob opens");
    let file = FileBlobStore::open(root.join("files")).expect("file blob opens");
    let store = HybridBlobStore::new(sqlite, file, 4);
    let tenant = TenantId::SINGLE;
    let small = Bytes::from_static(b"mini");
    let large = Bytes::from_static(b"larger");

    let small_ref = store
        .put(tenant, small.clone(), blob_meta(&small))
        .await
        .expect("small put succeeds");
    let large_ref = store
        .put(tenant, large.clone(), blob_meta(&large))
        .await
        .expect("large put succeeds");

    assert!(root.join("blobs.db").exists());
    let large_prefix = large_ref.id.to_string()[..2].to_owned();
    assert!(root
        .join("files")
        .join(tenant.to_string())
        .join(large_prefix)
        .join(format!("{}.bin", large_ref.id))
        .exists());
    assert_eq!(
        store
            .get(tenant, &small_ref)
            .await
            .expect("small get succeeds")
            .collect::<Vec<_>>()
            .await,
        vec![small]
    );
    assert_eq!(
        store
            .get(tenant, &large_ref)
            .await
            .expect("large get succeeds")
            .collect::<Vec<_>>()
            .await,
        vec![large]
    );
}
