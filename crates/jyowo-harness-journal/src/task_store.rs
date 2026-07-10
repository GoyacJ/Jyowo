//! Unified `SQLite` event store for daemon tasks.

use std::collections::{HashMap, HashSet};
#[cfg(feature = "blob-file")]
use std::fs::File;
use std::path::Path;
#[cfg(feature = "blob-file")]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
#[cfg(feature = "blob-file")]
use fs2::FileExt;
use harness_contracts::{
    now, BlobId, ClientId, CommandId, Event, EventId, EventSource, EventSourceKind, IdParseError,
    QueueItemProjection, SessionId, TaskEventEnvelope, TaskId, TaskProjection, TenantId,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::task_event::{NewTaskEvent, TaskBlobReference, TaskEvent, MAX_EVENT_PAYLOAD_BYTES};
use crate::task_projection::{
    empty_task_projection, load_task_projection_row, projection_counts, projection_snapshot,
    ProjectionCounts, SynchronousTaskProjector, TaskProjector, PROJECTION_TABLES,
};
use crate::task_schema::initialize_task_schema;

const MAX_COMMAND_PAYLOAD_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_EVENTS_PER_TRANSACTION: usize = 256;
pub(crate) const MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION: usize = 8 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_EVENT_TYPE_BYTES: usize = 128;
const MAX_SOURCE_JSON_BYTES: usize = 4096;
const MAX_READ_PAGE_SIZE: usize = 16;
const REBUILD_PAGE_SIZE: usize = 16;
#[cfg(any(test, feature = "blob-file"))]
const MAX_TASK_BLOB_BYTES: u64 = 1024 * 1024 * 1024;
#[cfg(any(test, feature = "blob-file"))]
const MAX_GLOBAL_BLOB_BYTES: u64 = 8 * 1024 * 1024 * 1024;
#[cfg(feature = "blob-file")]
const BLOB_OPERATION_LOCK_COUNT: usize = 64;
#[cfg(feature = "blob-file")]
const BLOB_ROOT_CLAIM_DIRECTORY: &str = ".jyowo-task-store";
#[cfg(feature = "blob-file")]
const BLOB_ROOT_LOCK_FILE: &str = ".jyowo-task-store.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAuthority {
    source: EventSource,
    principal_id: String,
}

impl EventAuthority {
    fn source(&self) -> &EventSource {
        &self.source
    }

    fn principal_id(&self) -> &str {
        &self.principal_id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcceptedCommand {
    pub command_id: CommandId,
    pub task_id: TaskId,
    pub idempotency_key: String,
    pub expected_stream_version: u64,
    pub authority: EventAuthority,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandOutcome {
    Accepted {
        command_id: CommandId,
        task_id: TaskId,
        stream_version: u64,
        committed_offset: u64,
    },
    Rejected {
        command_id: CommandId,
        task_id: TaskId,
        rejection: CommandRejection,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandRejection {
    WrongExpectedVersion { expected: u64, actual: u64 },
    StaleQueueRevision { latest: Box<QueueItemProjection> },
    InvalidCommand { message: String },
}

pub struct TaskStore {
    connection: Mutex<Connection>,
    projector: Arc<dyn TaskProjector>,
    #[cfg(feature = "blob-file")]
    blob_operation_locks: [Mutex<()>; BLOB_OPERATION_LOCK_COUNT],
    #[cfg(feature = "blob-file")]
    database_identity: String,
    #[cfg(feature = "blob-file")]
    blob_root_lock: Mutex<Option<std::fs::File>>,
}

#[cfg(feature = "blob-file")]
pub(crate) struct StoredBlobMetadata {
    pub(crate) media_type: String,
    pub(crate) byte_size: u64,
    pub(crate) content_hash: [u8; 32],
    pub(crate) relative_path: String,
}

struct StagedBlobMetadata {
    media_type: String,
    byte_size: u64,
    content_hash: String,
    relative_path: String,
}

impl TaskStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        Self::open_with_projector(path, Arc::new(SynchronousTaskProjector))
    }

    pub(crate) fn open_with_projector(
        path: impl AsRef<Path>,
        projector: Arc<dyn TaskProjector>,
    ) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path = crate::app_controlled_path(path)?;
        let connection = Connection::open(path)?;
        initialize_task_schema(&connection)?;
        #[cfg(feature = "blob-file")]
        let database_identity = load_or_create_blob_store_identity(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            projector,
            #[cfg(feature = "blob-file")]
            blob_operation_locks: std::array::from_fn(|_| Mutex::new(())),
            #[cfg(feature = "blob-file")]
            database_identity,
            #[cfg(feature = "blob-file")]
            blob_root_lock: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn user_authority(client_id: ClientId) -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::User,
                actor_id: None,
                client_id: Some(client_id),
            },
            principal_id: format!("user:{client_id}"),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn supervisor_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Supervisor,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:supervisor".into(),
        }
    }

    #[must_use]
    pub fn recovery_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Recovery,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:recovery".into(),
        }
    }

    #[must_use]
    pub fn supervisor_command_authority(authority: &EventAuthority) -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Supervisor,
                actor_id: None,
                client_id: authority.source.client_id,
            },
            principal_id: authority.principal_id.clone(),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn permission_broker_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::PermissionBroker,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:permission_broker".into(),
        }
    }

    #[must_use]
    pub(crate) fn engine_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Engine,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:engine".into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn append(
        &self,
        task_id: TaskId,
        expected_version: u64,
        authority: &EventAuthority,
        events: Vec<NewTaskEvent>,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        validate_events(authority.source(), &events)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_blob_references_in_transaction(&transaction, task_id, &events)?;
        let committed = append_in_transaction(
            &transaction,
            task_id,
            expected_version,
            authority.source(),
            events,
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        transaction.commit()?;
        Ok(committed)
    }

    pub fn transact_command<F>(
        &self,
        command: AcceptedCommand,
        decide: F,
    ) -> Result<CommandOutcome, TaskStoreError>
    where
        F: FnOnce(&TaskProjection) -> Result<Vec<NewTaskEvent>, CommandRejection>,
    {
        validate_command(&command)?;
        let command_hash = command_hash(&command)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = stored_command_outcome(&transaction, &command, &command_hash)? {
            return Ok(outcome);
        }

        transaction.execute(
            "INSERT INTO command_inbox (
                command_id, task_id, principal_id, idempotency_key, command_hash,
                expected_stream_version, status, accepted_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'processing', ?7)",
            params![
                command.command_id.to_string(),
                command.task_id.to_string(),
                command.authority.principal_id(),
                command.idempotency_key,
                command_hash,
                sqlite_integer(command.expected_stream_version)?,
                now().to_rfc3339(),
            ],
        )?;

        let actual = stream_version_in_transaction(&transaction, command.task_id)?;
        let projection = projection_for_decision(&transaction, command.task_id, actual)?;
        if actual != command.expected_stream_version {
            let outcome = CommandOutcome::Rejected {
                command_id: command.command_id,
                task_id: command.task_id,
                rejection: CommandRejection::WrongExpectedVersion {
                    expected: command.expected_stream_version,
                    actual,
                },
            };
            finish_command(
                &transaction,
                command.command_id,
                "rejected",
                &outcome,
                actual,
                latest_global_offset_in_transaction(&transaction)?,
                0,
            )?;
            transaction.commit()?;
            return Ok(outcome);
        }

        let events = match decide(&projection) {
            Ok(events) => events,
            Err(rejection) => {
                let outcome = CommandOutcome::Rejected {
                    command_id: command.command_id,
                    task_id: command.task_id,
                    rejection,
                };
                finish_command(
                    &transaction,
                    command.command_id,
                    "rejected",
                    &outcome,
                    actual,
                    latest_global_offset_in_transaction(&transaction)?,
                    0,
                )?;
                transaction.commit()?;
                return Ok(outcome);
            }
        };
        if events.is_empty() {
            let outcome = CommandOutcome::Rejected {
                command_id: command.command_id,
                task_id: command.task_id,
                rejection: CommandRejection::InvalidCommand {
                    message: "accepted commands must emit at least one task event".into(),
                },
            };
            finish_command(
                &transaction,
                command.command_id,
                "rejected",
                &outcome,
                actual,
                latest_global_offset_in_transaction(&transaction)?,
                0,
            )?;
            transaction.commit()?;
            return Ok(outcome);
        }
        validate_queue_consumption_events(&events)?;
        validate_events(command.authority.source(), &events)?;
        if let Err(error) = prepare_command_blob_references(&transaction, command.task_id, &events)
        {
            let Some(rejection) = blob_command_rejection(&error) else {
                return Err(error);
            };
            let outcome = CommandOutcome::Rejected {
                command_id: command.command_id,
                task_id: command.task_id,
                rejection,
            };
            finish_command(
                &transaction,
                command.command_id,
                "rejected",
                &outcome,
                actual,
                latest_global_offset_in_transaction(&transaction)?,
                0,
            )?;
            transaction.commit()?;
            return Ok(outcome);
        }
        let committed = append_in_transaction(
            &transaction,
            command.task_id,
            command.expected_stream_version,
            command.authority.source(),
            events,
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        let stream_version = committed
            .last()
            .map_or(command.expected_stream_version, |event| {
                event.stream_sequence
            });
        let committed_offset = committed
            .last()
            .map_or(latest_global_offset_in_transaction(&transaction), |event| {
                Ok(event.global_offset)
            })?;
        let outcome = CommandOutcome::Accepted {
            command_id: command.command_id,
            task_id: command.task_id,
            stream_version,
            committed_offset,
        };
        finish_command(
            &transaction,
            command.command_id,
            "accepted",
            &outcome,
            stream_version,
            committed_offset,
            committed.len(),
        )?;
        transaction.commit()?;
        Ok(outcome)
    }

    pub fn stream_version(&self, task_id: TaskId) -> Result<u64, TaskStoreError> {
        let connection = self.lock()?;
        let version: i64 = connection.query_row(
            "SELECT COALESCE(MAX(stream_sequence), 0) FROM event_log WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        nonnegative_integer(version)
    }

    pub fn events_after(
        &self,
        after_global_offset: u64,
        limit: usize,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        let after = i64::try_from(after_global_offset).unwrap_or(i64::MAX);
        let limit = i64::try_from(limit.clamp(1, MAX_READ_PAGE_SIZE))
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT global_offset, task_id, stream_sequence, event_id, event_type,
                    schema_version, recorded_at, source_json, payload_json
             FROM event_log
             WHERE global_offset > ?1
             ORDER BY global_offset ASC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after, limit], |row| {
            Ok(StoredTaskEvent {
                global_offset: row.get(0)?,
                task_id: row.get(1)?,
                stream_sequence: row.get(2)?,
                event_id: row.get(3)?,
                event_type: row.get(4)?,
                schema_version: row.get(5)?,
                recorded_at: row.get(6)?,
                source_json: row.get(7)?,
                payload_json: row.get(8)?,
            })
        })?;

        rows.map(|row| {
            row.map_err(TaskStoreError::from)
                .and_then(StoredTaskEvent::decode)
        })
        .collect()
    }

    pub fn latest_global_offset(&self) -> Result<u64, TaskStoreError> {
        let connection = self.lock()?;
        let offset: i64 = connection.query_row(
            "SELECT COALESCE(MAX(global_offset), 0) FROM event_log",
            [],
            |row| row.get(0),
        )?;
        nonnegative_integer(offset)
    }

    pub fn task_projection(
        &self,
        task_id: TaskId,
    ) -> Result<Option<TaskProjection>, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let actual = stream_version_in_transaction(&transaction, task_id)?;
        let projection = projection_for_decision(&transaction, task_id, actual)?;
        transaction.commit()?;
        Ok((actual > 0).then_some(projection))
    }

    pub fn queue_item_projection(
        &self,
        task_id: TaskId,
        queue_item_id: harness_contracts::QueueItemId,
    ) -> Result<Option<QueueItemProjection>, TaskStoreError> {
        let connection = self.lock()?;
        let body = connection
            .query_row(
                "SELECT projection_json FROM queue_projection
                 WHERE task_id = ?1 AND queue_item_id = ?2",
                params![task_id.to_string(), queue_item_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        body.map(|body| {
            let projection: QueueItemProjection = serde_json::from_str(&body)?;
            if projection.queue_item_id != queue_item_id {
                return Err(TaskStoreError::ProjectionIntegrity(format!(
                    "queue item {queue_item_id} projection has another identity"
                )));
            }
            Ok(projection)
        })
        .transpose()
    }

    pub fn projection_counts(&self) -> Result<ProjectionCounts, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let counts = projection_counts(&transaction)?;
        transaction.commit()?;
        Ok(counts)
    }

    pub fn rebuild_projections(&self) -> Result<(), TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = projection_snapshot(&transaction)?;
        for table in PROJECTION_TABLES {
            transaction.execute(&format!("DELETE FROM {table}"), [])?;
        }
        let mut after_offset = 0;
        let mut previous_global_offset = None;
        let mut task_versions = HashMap::<TaskId, u64>::new();
        loop {
            let events = load_events_page(&transaction, after_offset, REBUILD_PAGE_SIZE)?;
            if events.is_empty() {
                break;
            }
            for event in &events {
                if previous_global_offset.is_some_and(|previous| event.global_offset <= previous) {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "event log global offsets are not strictly increasing".into(),
                    ));
                }
                let expected = task_versions.get(&event.task_id).copied().unwrap_or(0) + 1;
                if event.stream_sequence != expected {
                    return Err(TaskStoreError::ProjectionIntegrity(format!(
                        "task {} stream sequence is {}, expected {expected}",
                        event.task_id, event.stream_sequence
                    )));
                }
                self.projector.apply(&transaction, event)?;
                task_versions.insert(event.task_id, event.stream_sequence);
                previous_global_offset = Some(event.global_offset);
                after_offset = event.global_offset;
            }
        }
        let _repaired = before != projection_snapshot(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn append_engine_events(
        &self,
        task_id: TaskId,
        tenant_id: TenantId,
        session_id: SessionId,
        metadata: crate::AppendMetadata,
        expected_next_offset: Option<u64>,
        events: &[Event],
    ) -> Result<u64, TaskStoreError> {
        let authority = Self::engine_authority();
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next_engine_offset: i64 = transaction.query_row(
            "SELECT COALESCE(
                MAX(CAST(json_extract(payload_json, '$.journalOffset') AS INTEGER)), -1
             ) + 1
             FROM event_log
             WHERE task_id = ?1
               AND event_type GLOB 'engine.*'
               AND json_extract(payload_json, '$.tenantId') = ?2
               AND json_extract(payload_json, '$.sessionId') = ?3",
            params![
                task_id.to_string(),
                tenant_id.to_string(),
                session_id.to_string()
            ],
            |row| row.get(0),
        )?;
        let next_engine_offset = nonnegative_integer(next_engine_offset)?;
        if let Some(expected) = expected_next_offset {
            if expected != next_engine_offset {
                return Err(TaskStoreError::EngineOffsetMismatch {
                    expected,
                    actual: next_engine_offset,
                });
            }
        }
        if events.is_empty() {
            transaction.commit()?;
            return Ok(next_engine_offset.saturating_sub(1));
        }

        let events = events
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, event)| {
                let index = u64::try_from(index).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
                let journal_offset = next_engine_offset
                    .checked_add(index)
                    .ok_or(TaskStoreError::IntegerOutOfRange)?;
                NewTaskEvent::engine(
                    tenant_id,
                    session_id,
                    journal_offset,
                    metadata.run_id,
                    metadata.correlation_id,
                    metadata.causation_id,
                    event,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_events(authority.source(), &events)?;
        promote_blob_references_in_transaction(&transaction, task_id, &events)?;
        validate_blob_references_in_transaction(&transaction, task_id, &events)?;

        let stream_version = stream_version_in_transaction(&transaction, task_id)?;
        let committed = append_in_transaction(
            &transaction,
            task_id,
            stream_version,
            authority.source(),
            events,
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        let committed_count =
            u64::try_from(committed.len()).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let last_offset = next_engine_offset
            .checked_add(committed_count)
            .and_then(|count| count.checked_sub(1))
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        transaction.commit()?;
        Ok(last_offset)
    }

    #[cfg(any(test, feature = "blob-file"))]
    pub(crate) fn stage_blob(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
        media_type: &str,
        byte_size: u64,
        content_hash: [u8; 32],
        relative_path: &str,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stream_version_in_transaction(&transaction, task_id)? == 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "task {task_id} must exist before staging blobs"
            )));
        }
        let content_hash = blake3::Hash::from_bytes(content_hash).to_hex().to_string();
        let existing_metadata = blob_metadata_in_transaction(&transaction, blob_id)?;
        if let Some(existing) = &existing_metadata {
            validate_staged_blob_identity(
                blob_id,
                existing.byte_size,
                &existing.content_hash,
                &existing.relative_path,
                byte_size,
                &content_hash,
                relative_path,
            )?;
        }
        let owned_media_type = transaction
            .query_row(
                "SELECT media_type FROM blob_ownership WHERE task_id = ?1 AND blob_id = ?2",
                params![task_id.to_string(), blob_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(owned_media_type) = owned_media_type {
            if owned_media_type != media_type {
                return Err(TaskStoreError::BlobIntegrity(format!(
                    "blob {blob_id} is already owned with another media type"
                )));
            }
            transaction.commit()?;
            return Ok(());
        }
        let existing_stage = staged_blob_for_task(&transaction, task_id, blob_id)?;
        if let Some(existing) = &existing_stage {
            validate_staged_blob_identity(
                blob_id,
                existing.byte_size,
                &existing.content_hash,
                &existing.relative_path,
                byte_size,
                &content_hash,
                relative_path,
            )?;
            if existing.media_type != media_type {
                return Err(TaskStoreError::BlobIntegrity(format!(
                    "blob {blob_id} is already staged with another media type"
                )));
            }
        }
        transaction.execute(
            "INSERT INTO blob_staging (
                task_id, blob_id, media_type, byte_size, content_hash, relative_path, staged_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(task_id, blob_id) DO UPDATE SET staged_at = excluded.staged_at",
            params![
                task_id.to_string(),
                blob_id.to_string(),
                media_type,
                sqlite_integer(byte_size)?,
                content_hash,
                relative_path,
                now().to_rfc3339(),
            ],
        )?;
        enforce_blob_quotas(&transaction, task_id)?;
        transaction.commit()?;
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn discard_staged_blob_with<F>(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
        cleanup_file: F,
    ) -> Result<(), TaskStoreError>
    where
        F: FnOnce() -> Result<(), TaskStoreError>,
    {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM blob_staging WHERE task_id = ?1 AND blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
        )?;
        let referenced: i64 = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM blob_metadata WHERE blob_id = ?1)
                 OR EXISTS(SELECT 1 FROM blob_staging WHERE blob_id = ?1)",
            [blob_id.to_string()],
            |row| row.get(0),
        )?;
        if referenced == 0 {
            cleanup_file()?;
        }
        transaction.commit()?;
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn cleanup_blob_if_unreferenced<F>(
        &self,
        blob_id: BlobId,
        byte_size: u64,
        content_hash: [u8; 32],
        relative_path: &str,
        cleanup_file: F,
    ) -> Result<(), TaskStoreError>
    where
        F: FnOnce() -> Result<(), TaskStoreError>,
    {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let referenced: i64 = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM blob_metadata
                WHERE blob_id = ?1 AND byte_size = ?2 AND content_hash = ?3 AND relative_path = ?4
             ) OR EXISTS(
                SELECT 1 FROM blob_staging
                WHERE blob_id = ?1 AND byte_size = ?2 AND content_hash = ?3 AND relative_path = ?4
             )",
            params![
                blob_id.to_string(),
                sqlite_integer(byte_size)?,
                blake3::Hash::from_bytes(content_hash).to_hex().to_string(),
                relative_path,
            ],
            |row| row.get(0),
        )?;
        if referenced == 0 {
            cleanup_file()?;
        }
        transaction.commit()?;
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn bind_blob_root(&self, root: &Path) -> Result<(), TaskStoreError> {
        let root_text = root.to_str().ok_or_else(|| {
            TaskStoreError::InvalidInput("task blob root is not valid UTF-8".into())
        })?;
        let mut root_lock = self
            .blob_root_lock
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        let new_lock = if root_lock.is_none() {
            let lock_path = root.join(BLOB_ROOT_LOCK_FILE);
            let file = open_blob_root_lock(&lock_path)?;
            harness_fs::set_owner_only_file_if_unix(&file)?;
            file.try_lock_exclusive().map_err(|_| {
                TaskStoreError::BlobIntegrity(
                    "task blob root is already open by another store instance".into(),
                )
            })?;
            Some(file)
        } else {
            None
        };
        let mut claim = None;
        let result = (|| {
            let mut connection = self.lock()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction.query_row(
                "SELECT canonical_root FROM blob_store_config WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )?;
            if let Some(existing) = existing.as_deref() {
                if existing != root_text {
                    return Err(TaskStoreError::BlobIntegrity(format!(
                        "task database is already bound to blob root {existing}"
                    )));
                }
            }
            claim = Some(claim_blob_root(root, &self.database_identity)?);
            if existing.is_none() {
                transaction.execute(
                    "UPDATE blob_store_config SET canonical_root = ?1 WHERE singleton = 1",
                    [root_text],
                )?;
            }
            if new_lock.is_some() {
                reconcile_blob_root_in_transaction(&transaction, root)?;
            }
            transaction.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            if let Some(claim) = claim {
                claim.rollback();
            }
            return Err(error);
        }
        if let Some(file) = new_lock {
            *root_lock = Some(file);
        }
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn blob_metadata_for_task(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
    ) -> Result<StoredBlobMetadata, TaskStoreError> {
        let connection = self.lock()?;
        let metadata = connection
            .query_row(
                "SELECT ownership.media_type, metadata.byte_size,
                        metadata.content_hash, metadata.relative_path
                 FROM blob_metadata AS metadata
                 JOIN blob_ownership AS ownership ON ownership.blob_id = metadata.blob_id
                 WHERE ownership.task_id = ?1 AND metadata.blob_id = ?2",
                params![task_id.to_string(), blob_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let metadata = match metadata {
            Some(metadata) => metadata,
            None => {
                let exists: i64 = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM blob_metadata WHERE blob_id = ?1)",
                    [blob_id.to_string()],
                    |row| row.get(0),
                )?;
                return if exists == 1 {
                    Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
                } else {
                    Err(TaskStoreError::BlobNotFound { blob_id })
                };
            }
        };
        let hash = blake3::Hash::from_hex(&metadata.2).map_err(|error| {
            TaskStoreError::BlobIntegrity(format!(
                "blob {blob_id} has invalid hash metadata: {error}"
            ))
        })?;
        Ok(StoredBlobMetadata {
            media_type: metadata.0,
            byte_size: nonnegative_integer(metadata.1)?,
            content_hash: *hash.as_bytes(),
            relative_path: metadata.3,
        })
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn lock_blob_operation(
        &self,
        blob_id: BlobId,
    ) -> Result<std::sync::MutexGuard<'_, ()>, TaskStoreError> {
        let mut prefix = [0_u8; 8];
        prefix.copy_from_slice(&blob_id.as_bytes()[..8]);
        let index = usize::try_from(u64::from_be_bytes(prefix) % BLOB_OPERATION_LOCK_COUNT as u64)
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        self.blob_operation_locks[index]
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
}

#[cfg(feature = "blob-file")]
fn load_or_create_blob_store_identity(connection: &Connection) -> Result<String, TaskStoreError> {
    let candidate = TaskId::new().to_string();
    connection.execute(
        "INSERT INTO blob_store_config (singleton, store_id, canonical_root)
         VALUES (1, ?1, NULL)
         ON CONFLICT(singleton) DO NOTHING",
        [candidate],
    )?;
    connection
        .query_row(
            "SELECT store_id FROM blob_store_config WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(TaskStoreError::from)
}

#[cfg(feature = "blob-file")]
fn open_blob_root_lock(path: &Path) -> Result<File, TaskStoreError> {
    #[cfg(unix)]
    {
        let parent = harness_fs::open_parent_dir_no_symlink_for_read(path)?
            .ok_or_else(|| TaskStoreError::BlobIntegrity("task blob root does not exist".into()))?;
        let file = parent.open_or_create_read_write_file(parent.file_name())?;
        if !file.metadata()?.is_file() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root lock is not a regular file".into(),
            ));
        }
        parent.sync_all()?;
        return Ok(file);
    }

    #[cfg(not(unix))]
    {
        harness_fs::ensure_no_symlink_components(path)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root lock is not a regular file".into(),
            ));
        }
        Ok(file)
    }
}

#[cfg(feature = "blob-file")]
struct BlobRootClaim {
    directory: PathBuf,
    owner: PathBuf,
    owner_created: bool,
}

#[cfg(feature = "blob-file")]
impl BlobRootClaim {
    fn rollback(self) {
        if !self.owner_created {
            return;
        }
        let _ = std::fs::remove_dir(self.owner);
        let _ = std::fs::remove_dir(self.directory);
    }
}

#[cfg(feature = "blob-file")]
fn claim_blob_root(root: &Path, database_identity: &str) -> Result<BlobRootClaim, TaskStoreError> {
    let claim_directory = root.join(BLOB_ROOT_CLAIM_DIRECTORY);
    let owner = claim_directory.join(database_identity);
    let claim_directory_created = match std::fs::create_dir(&claim_directory) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error.into()),
    };
    let mut owner_created = false;
    let result = (|| {
        harness_fs::ensure_owner_only_app_dir(&claim_directory)?;
        owner_created = match std::fs::create_dir(&owner) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(TaskStoreError::from(error)),
        };
        harness_fs::ensure_owner_only_app_dir(&owner)?;
        let mut entries = std::fs::read_dir(&claim_directory)?;
        let Some(entry) = entries.next().transpose()? else {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root has an incomplete database claim".into(),
            ));
        };
        if entry.path() != owner || entries.next().transpose()?.is_some() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root is already claimed by another database".into(),
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        BlobRootClaim {
            directory: claim_directory.clone(),
            owner,
            owner_created,
        }
        .rollback();
        if claim_directory_created {
            let _ = std::fs::remove_dir(&claim_directory);
        }
        return Err(error);
    }
    Ok(BlobRootClaim {
        directory: claim_directory,
        owner,
        owner_created,
    })
}

#[cfg(feature = "blob-file")]
fn reconcile_blob_root_in_transaction(
    transaction: &Transaction<'_>,
    root: &Path,
) -> Result<(), TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT relative_path FROM blob_metadata
         UNION
         SELECT relative_path FROM blob_staging",
    )?;
    let referenced = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    drop(statement);

    for prefix_entry in std::fs::read_dir(root)? {
        let prefix_entry = prefix_entry?;
        if prefix_entry.file_name() == BLOB_ROOT_CLAIM_DIRECTORY {
            continue;
        }
        let prefix_path = prefix_entry.path();
        let prefix_metadata = std::fs::symlink_metadata(&prefix_path)?;
        if prefix_metadata.file_type().is_symlink() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root contains a symlink".into(),
            ));
        }
        let prefix = prefix_entry.file_name();
        if !prefix_metadata.is_dir() || prefix.as_encoded_bytes().len() != 2 {
            continue;
        }
        for body_entry in std::fs::read_dir(&prefix_path)? {
            let body_entry = body_entry?;
            let body_path = body_entry.path();
            let body_metadata = std::fs::symlink_metadata(&body_path)?;
            if body_metadata.file_type().is_symlink() || !body_metadata.is_file() {
                return Err(TaskStoreError::BlobIntegrity(
                    "task blob directory contains a non-regular file".into(),
                ));
            }
            let relative_path = body_path
                .strip_prefix(root)
                .map_err(|_| {
                    TaskStoreError::BlobIntegrity(
                        "task blob file is outside its configured root".into(),
                    )
                })?
                .to_str()
                .ok_or_else(|| {
                    TaskStoreError::BlobIntegrity(
                        "task blob relative path is not valid UTF-8".into(),
                    )
                })?;
            if !referenced.contains(relative_path) {
                harness_fs::remove_file_no_follow(&body_path)?;
            }
        }
    }
    Ok(())
}

fn append_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    expected_version: u64,
    source: &EventSource,
    events: Vec<NewTaskEvent>,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
    let actual = stream_version_in_transaction(transaction, task_id)?;
    if actual != expected_version {
        return Err(TaskStoreError::WrongExpectedVersion {
            expected: expected_version,
            actual,
        });
    }

    let source_json = serde_json::to_string(source)?;
    let mut committed = Vec::with_capacity(events.len());
    for (index, event) in events.into_iter().enumerate() {
        let index = u64::try_from(index).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let stream_sequence = expected_version
            .checked_add(index + 1)
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        let event_id = EventId::new();
        let recorded_at = now();
        let (event_type, schema_version, payload) = event.encode()?;
        let payload_json = serde_json::to_string(&payload)?;
        let inserted = transaction.execute(
            "INSERT INTO event_log (
                task_id, stream_sequence, event_id, event_type, schema_version,
                recorded_at, source_json, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task_id.to_string(),
                sqlite_integer(stream_sequence)?,
                event_id.to_string(),
                event_type,
                i64::from(schema_version),
                recorded_at.to_rfc3339(),
                source_json,
                payload_json,
            ],
        );
        if let Err(error) = inserted {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                return Err(TaskStoreError::Projector(
                    "task event violates an identity or stream uniqueness constraint".into(),
                ));
            }
            return Err(error.into());
        }
        let global_offset = nonnegative_integer(transaction.last_insert_rowid())?;
        committed.push(TaskEventEnvelope {
            global_offset,
            task_id,
            stream_sequence,
            event_id,
            event_type: event_type.into(),
            schema_version,
            recorded_at,
            source: source.clone(),
            payload,
        });
    }
    Ok(committed)
}

fn validate_blob_references_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    events: &[NewTaskEvent],
) -> Result<(), TaskStoreError> {
    let mut references = Vec::new();
    for event in events {
        references.extend(event.blob_references()?);
    }
    let references = merge_blob_references(references)?;
    for reference in references {
        validate_blob_reference_in_transaction(transaction, task_id, &reference)?;
    }
    Ok(())
}

fn merge_blob_references(
    references: impl IntoIterator<Item = TaskBlobReference>,
) -> Result<Vec<TaskBlobReference>, TaskStoreError> {
    let mut merged = HashMap::<BlobId, TaskBlobReference>::new();
    for reference in references {
        match merged.get_mut(&reference.blob_id) {
            Some(existing) => match (&existing.expected, reference.expected) {
                (Some(current), Some(incoming)) if current != &incoming => {
                    return Err(TaskStoreError::BlobIntegrity(format!(
                        "blob {} has conflicting references in one event batch",
                        reference.blob_id
                    )));
                }
                (None, Some(incoming)) => existing.expected = Some(incoming),
                _ => {}
            },
            None => {
                merged.insert(reference.blob_id, reference);
            }
        }
    }
    Ok(merged.into_values().collect())
}

fn validate_blob_reference_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    reference: &TaskBlobReference,
) -> Result<(), TaskStoreError> {
    let blob_id = reference.blob_id;
    let owned = transaction
        .query_row(
            "SELECT ownership.media_type, metadata.byte_size, metadata.content_hash
             FROM blob_ownership AS ownership
             JOIN blob_metadata AS metadata ON metadata.blob_id = ownership.blob_id
             WHERE ownership.task_id = ?1 AND ownership.blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((media_type, byte_size, content_hash)) = owned else {
        return if blob_metadata_in_transaction(transaction, blob_id)?.is_some() {
            Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
        } else {
            Err(TaskStoreError::BlobNotFound { blob_id })
        };
    };
    if let Some(expected) = &reference.expected {
        let expected_media_type = expected.content_type.as_deref();
        if expected.id != blob_id
            || expected.size != nonnegative_integer(byte_size)?
            || blake3::Hash::from_bytes(expected.content_hash)
                .to_hex()
                .as_str()
                != content_hash
            || expected_media_type != Some(media_type.as_str())
        {
            return Err(TaskStoreError::BlobIntegrity(format!(
                "blob reference {blob_id} does not match task-owned metadata"
            )));
        }
    }
    Ok(())
}

fn promote_blob_references_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    events: &[NewTaskEvent],
) -> Result<(), TaskStoreError> {
    let mut checked = HashSet::new();
    let mut references = Vec::new();
    for event in events {
        references.extend(event.blob_references()?);
    }
    for blob_id in references.into_iter().map(|reference| reference.blob_id) {
        if !checked.insert(blob_id) {
            continue;
        }
        let owned: i64 = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM blob_ownership WHERE task_id = ?1 AND blob_id = ?2
             )",
            params![task_id.to_string(), blob_id.to_string()],
            |row| row.get(0),
        )?;
        if owned == 1 {
            continue;
        }
        let staged = staged_blob_for_task(transaction, task_id, blob_id)?;
        let Some(staged) = staged else {
            return if blob_metadata_in_transaction(transaction, blob_id)?.is_some() {
                Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
            } else {
                Err(TaskStoreError::BlobNotFound { blob_id })
            };
        };
        if let Some(existing) = blob_metadata_in_transaction(transaction, blob_id)? {
            validate_staged_blob_identity(
                blob_id,
                existing.byte_size,
                &existing.content_hash,
                &existing.relative_path,
                staged.byte_size,
                &staged.content_hash,
                &staged.relative_path,
            )?;
        } else {
            transaction.execute(
                "INSERT INTO blob_metadata (
                    blob_id, media_type, byte_size, content_hash, relative_path, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    blob_id.to_string(),
                    staged.media_type,
                    sqlite_integer(staged.byte_size)?,
                    staged.content_hash,
                    staged.relative_path,
                    now().to_rfc3339(),
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO blob_ownership (task_id, blob_id, media_type, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                task_id.to_string(),
                blob_id.to_string(),
                staged.media_type,
                now().to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM blob_staging WHERE task_id = ?1 AND blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
        )?;
    }
    Ok(())
}

fn prepare_command_blob_references(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    events: &[NewTaskEvent],
) -> Result<(), TaskStoreError> {
    transaction.execute_batch("SAVEPOINT command_blob_references")?;
    let result = promote_blob_references_in_transaction(transaction, task_id, events)
        .and_then(|()| validate_blob_references_in_transaction(transaction, task_id, events));
    match result {
        Ok(()) => {
            transaction.execute_batch("RELEASE command_blob_references")?;
            Ok(())
        }
        Err(error) => {
            transaction.execute_batch(
                "ROLLBACK TO command_blob_references; RELEASE command_blob_references",
            )?;
            Err(error)
        }
    }
}

fn blob_command_rejection(error: &TaskStoreError) -> Option<CommandRejection> {
    match error {
        TaskStoreError::BlobNotFound { .. }
        | TaskStoreError::BlobOwnershipDenied { .. }
        | TaskStoreError::BlobIntegrity(_) => Some(CommandRejection::InvalidCommand {
            message: "command references an unavailable or invalid blob".into(),
        }),
        _ => None,
    }
}

fn blob_metadata_in_transaction(
    transaction: &Transaction<'_>,
    blob_id: BlobId,
) -> Result<Option<StagedBlobMetadata>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT media_type, byte_size, content_hash, relative_path
             FROM blob_metadata WHERE blob_id = ?1",
            [blob_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(media_type, byte_size, content_hash, relative_path)| {
            Ok(StagedBlobMetadata {
                media_type,
                byte_size: nonnegative_integer(byte_size)?,
                content_hash,
                relative_path,
            })
        })
        .transpose()
}

fn staged_blob_for_task(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    blob_id: BlobId,
) -> Result<Option<StagedBlobMetadata>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT media_type, byte_size, content_hash, relative_path
             FROM blob_staging WHERE task_id = ?1 AND blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(media_type, byte_size, content_hash, relative_path)| {
            Ok(StagedBlobMetadata {
                media_type,
                byte_size: nonnegative_integer(byte_size)?,
                content_hash,
                relative_path,
            })
        })
        .transpose()
}

#[allow(clippy::too_many_arguments)]
fn validate_staged_blob_identity(
    blob_id: BlobId,
    existing_size: u64,
    existing_hash: &str,
    existing_path: &str,
    staged_size: u64,
    staged_hash: &str,
    staged_path: &str,
) -> Result<(), TaskStoreError> {
    if existing_size != staged_size || existing_hash != staged_hash || existing_path != staged_path
    {
        return Err(TaskStoreError::BlobIntegrity(format!(
            "blob {blob_id} metadata conflicts with its content-addressed identity"
        )));
    }
    Ok(())
}

#[cfg(any(test, feature = "blob-file"))]
fn enforce_blob_quotas(
    transaction: &Transaction<'_>,
    task_id: TaskId,
) -> Result<(), TaskStoreError> {
    let task_bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(byte_size), 0) FROM (
            SELECT metadata.blob_id, metadata.byte_size
            FROM blob_metadata AS metadata
            JOIN blob_ownership AS ownership ON ownership.blob_id = metadata.blob_id
            WHERE ownership.task_id = ?1
            UNION
            SELECT blob_id, byte_size FROM blob_staging WHERE task_id = ?1
         )",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    if nonnegative_integer(task_bytes)? > MAX_TASK_BLOB_BYTES {
        return Err(TaskStoreError::InvalidInput(format!(
            "task blob quota exceeds {MAX_TASK_BLOB_BYTES} bytes"
        )));
    }
    let global_bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(byte_size), 0) FROM (
            SELECT blob_id, byte_size FROM blob_metadata
            UNION
            SELECT blob_id, byte_size FROM blob_staging
         )",
        [],
        |row| row.get(0),
    )?;
    if nonnegative_integer(global_bytes)? > MAX_GLOBAL_BLOB_BYTES {
        return Err(TaskStoreError::InvalidInput(format!(
            "global blob quota exceeds {MAX_GLOBAL_BLOB_BYTES} bytes"
        )));
    }
    Ok(())
}

fn stream_version_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
) -> Result<u64, TaskStoreError> {
    let version: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(stream_sequence), 0) FROM event_log WHERE task_id = ?1",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    nonnegative_integer(version)
}

fn projection_for_decision(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    actual_stream_version: u64,
) -> Result<TaskProjection, TaskStoreError> {
    let canonical_row = load_task_projection_row(transaction, task_id)?;
    if actual_stream_version == 0 {
        if canonical_row.is_some() {
            return Err(TaskStoreError::ProjectionIntegrity(format!(
                "task {task_id} has a projection without an event stream"
            )));
        }
        return Ok(empty_task_projection(task_id));
    }
    let (_, projection) = canonical_row.ok_or_else(|| {
        TaskStoreError::ProjectionIntegrity(format!(
            "task {task_id} has events but no canonical projection"
        ))
    })?;
    if projection.stream_version != actual_stream_version {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "task {task_id} canonical version is {}, event version is {actual_stream_version}",
            projection.stream_version
        )));
    }
    let event_global_offset: i64 = transaction.query_row(
        "SELECT global_offset FROM event_log
         WHERE task_id = ?1
         ORDER BY stream_sequence DESC
         LIMIT 1",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    let event_global_offset = nonnegative_integer(event_global_offset)?;
    if projection.last_global_offset != event_global_offset {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "task {task_id} projection offset is {}, event offset is {event_global_offset}",
            projection.last_global_offset
        )));
    }
    Ok(projection)
}

fn latest_global_offset_in_transaction(
    transaction: &Transaction<'_>,
) -> Result<u64, TaskStoreError> {
    let offset: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(global_offset), 0) FROM event_log",
        [],
        |row| row.get(0),
    )?;
    nonnegative_integer(offset)
}

fn stored_command_outcome(
    transaction: &Transaction<'_>,
    command: &AcceptedCommand,
    command_hash: &str,
) -> Result<Option<CommandOutcome>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT command_id, task_id, principal_id, idempotency_key, command_hash,
                expected_stream_version, status, completed_at, outcome_json,
                result_stream_version, committed_offset, event_count
         FROM command_inbox
         WHERE command_id = ?1
            OR (task_id = ?2 AND principal_id = ?3 AND idempotency_key = ?4)",
    )?;
    let rows = statement.query_map(
        params![
            command.command_id.to_string(),
            command.task_id.to_string(),
            command.authority.principal_id(),
            command.idempotency_key
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<i64>>(10)?,
                row.get::<_, Option<i64>>(11)?,
            ))
        },
    )?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err(TaskStoreError::CommandConflict {
            command_id: command.command_id,
        });
    }
    let (
        stored_command_id,
        stored_task_id,
        stored_principal_id,
        stored_idempotency_key,
        stored_hash,
        stored_expected_version,
        status,
        completed_at,
        outcome_json,
        result_stream_version,
        committed_offset,
        event_count,
    ) = &rows[0];
    let same_command_id = stored_command_id == &command.command_id.to_string();
    let same_idempotency_key = stored_task_id == &command.task_id.to_string()
        && stored_principal_id == command.authority.principal_id()
        && stored_idempotency_key == &command.idempotency_key;
    let command_id_replay = same_command_id && same_idempotency_key;
    let idempotency_replay = !same_command_id && same_idempotency_key;
    if (!command_id_replay && !idempotency_replay)
        || stored_hash != command_hash
        || stored_task_id != &command.task_id.to_string()
        || stored_principal_id != command.authority.principal_id()
        || nonnegative_integer(*stored_expected_version)? != command.expected_stream_version
    {
        return Err(TaskStoreError::CommandConflict {
            command_id: command.command_id,
        });
    }
    if !matches!(status.as_str(), "accepted" | "rejected") || completed_at.is_none() {
        return Err(TaskStoreError::IncompleteCommand {
            command_id: command.command_id,
        });
    }
    let outcome_json = outcome_json
        .as_ref()
        .ok_or(TaskStoreError::IncompleteCommand {
            command_id: command.command_id,
        })?;
    let outcome: CommandOutcome = serde_json::from_str(outcome_json)?;
    let result_stream_version = required_nonnegative_integer(
        *result_stream_version,
        stored_command_id,
        "result stream version",
    )?;
    let committed_offset =
        required_nonnegative_integer(*committed_offset, stored_command_id, "committed offset")?;
    let event_count = required_nonnegative_integer(*event_count, stored_command_id, "event count")?;
    let (outcome_command_id, outcome_task_id, outcome_status) = match &outcome {
        CommandOutcome::Accepted {
            command_id,
            task_id,
            ..
        } => (*command_id, *task_id, "accepted"),
        CommandOutcome::Rejected {
            command_id,
            task_id,
            ..
        } => (*command_id, *task_id, "rejected"),
    };
    if outcome_command_id.to_string() != *stored_command_id
        || outcome_task_id.to_string() != *stored_task_id
        || outcome_status != status
    {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "command {stored_command_id} inbox outcome is inconsistent"
        )));
    }
    validate_stored_command_facts(
        transaction,
        &outcome,
        command.expected_stream_version,
        result_stream_version,
        committed_offset,
        event_count,
    )?;
    Ok(Some(outcome))
}

fn finish_command(
    transaction: &Transaction<'_>,
    command_id: CommandId,
    status: &str,
    outcome: &CommandOutcome,
    result_stream_version: u64,
    committed_offset: u64,
    event_count: usize,
) -> Result<(), TaskStoreError> {
    let changed = transaction.execute(
        "UPDATE command_inbox
         SET status = ?2, completed_at = ?3, outcome_json = ?4,
             result_stream_version = ?5, committed_offset = ?6, event_count = ?7
         WHERE command_id = ?1",
        params![
            command_id.to_string(),
            status,
            now().to_rfc3339(),
            serde_json::to_string(outcome)?,
            sqlite_integer(result_stream_version)?,
            sqlite_integer(committed_offset)?,
            i64::try_from(event_count).map_err(|_| TaskStoreError::IntegerOutOfRange)?,
        ],
    )?;
    if changed != 1 {
        return Err(TaskStoreError::IncompleteCommand { command_id });
    }
    Ok(())
}

fn required_nonnegative_integer(
    value: Option<i64>,
    command_id: &str,
    field: &str,
) -> Result<u64, TaskStoreError> {
    value
        .ok_or_else(|| {
            TaskStoreError::ProjectionIntegrity(format!("command {command_id} is missing {field}"))
        })
        .and_then(nonnegative_integer)
}

fn validate_stored_command_facts(
    transaction: &Transaction<'_>,
    outcome: &CommandOutcome,
    expected_stream_version: u64,
    result_stream_version: u64,
    committed_offset: u64,
    event_count: u64,
) -> Result<(), TaskStoreError> {
    match outcome {
        CommandOutcome::Accepted {
            task_id,
            stream_version,
            committed_offset: outcome_offset,
            ..
        } => {
            if event_count == 0
                || *stream_version != result_stream_version
                || *outcome_offset != committed_offset
                || result_stream_version
                    != expected_stream_version
                        .checked_add(event_count)
                        .ok_or(TaskStoreError::IntegerOutOfRange)?
            {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "accepted command outcome does not match persisted commit facts".into(),
                ));
            }
            if event_count > 0 {
                let (count, max_offset): (i64, Option<i64>) = transaction.query_row(
                    "SELECT COUNT(*), MAX(global_offset)
                     FROM event_log
                     WHERE task_id = ?1 AND stream_sequence > ?2 AND stream_sequence <= ?3",
                    params![
                        task_id.to_string(),
                        sqlite_integer(expected_stream_version)?,
                        sqlite_integer(result_stream_version)?,
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                if nonnegative_integer(count)? != event_count
                    || max_offset.map(nonnegative_integer).transpose()? != Some(committed_offset)
                {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "accepted command commit facts do not match the event log".into(),
                    ));
                }
            }
        }
        CommandOutcome::Rejected { rejection, .. } => {
            if event_count != 0 {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "rejected command persisted a non-zero event count".into(),
                ));
            }
            match rejection {
                CommandRejection::WrongExpectedVersion { expected, actual }
                    if *expected == expected_stream_version && *actual == result_stream_version => {
                }
                CommandRejection::StaleQueueRevision { .. }
                | CommandRejection::InvalidCommand { .. }
                    if result_stream_version == expected_stream_version => {}
                _ => {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "rejected command outcome does not match persisted commit facts".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn command_hash(command: &AcceptedCommand) -> Result<String, TaskStoreError> {
    let canonical = canonicalize_json(json!({
        "taskId": command.task_id,
        "expectedStreamVersion": command.expected_stream_version,
        "principalId": command.authority.principal_id(),
        "source": command.authority.source(),
        "payload": command.payload,
    }));
    Ok(blake3::hash(&serde_json::to_vec(&canonical)?)
        .to_hex()
        .to_string())
}

fn validate_command(command: &AcceptedCommand) -> Result<(), TaskStoreError> {
    if i64::try_from(command.expected_stream_version).is_err() {
        return Err(TaskStoreError::InvalidInput(
            "expected stream version exceeds SQLite's supported range".into(),
        ));
    }
    if command.idempotency_key.is_empty()
        || command.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_BYTES
    {
        return Err(TaskStoreError::InvalidInput(
            "idempotency key must contain 1 to 256 bytes".into(),
        ));
    }
    if serde_json::to_vec(&command.payload)?.len() > MAX_COMMAND_PAYLOAD_BYTES {
        return Err(TaskStoreError::InvalidInput(
            "command payload exceeds 1 MiB".into(),
        ));
    }
    Ok(())
}

fn validate_events(source: &EventSource, events: &[NewTaskEvent]) -> Result<(), TaskStoreError> {
    if events.len() > MAX_EVENTS_PER_TRANSACTION {
        return Err(TaskStoreError::InvalidInput(format!(
            "a transaction may contain at most {MAX_EVENTS_PER_TRANSACTION} events"
        )));
    }
    let source_bytes = serde_json::to_vec(source)?.len();
    let mut total_bytes = 0_usize;
    for event in events {
        event.validate_source(source)?;
        let (event_type, _, payload) = event.encode()?;
        let payload_bytes = serde_json::to_vec(&payload)?.len();
        let event_bytes = source_bytes
            .checked_add(event_type.len())
            .and_then(|bytes| bytes.checked_add(payload_bytes))
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        total_bytes = total_bytes
            .checked_add(event_bytes)
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        if total_bytes > MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION {
            return Err(TaskStoreError::InvalidInput(
                "task event transaction exceeds 8 MiB".into(),
            ));
        }
    }
    Ok(())
}

fn validate_queue_consumption_events(events: &[NewTaskEvent]) -> Result<(), TaskStoreError> {
    let started_segments = events
        .iter()
        .filter_map(|event| match &event.event {
            TaskEvent::RunStarted { segment_id, .. } => Some(*segment_id),
            _ => None,
        })
        .collect::<HashSet<_>>();
    for event in events {
        let TaskEvent::MessageConsumed { run_segment_id, .. } = &event.event else {
            continue;
        };
        if !started_segments.contains(run_segment_id) {
            return Err(TaskStoreError::InvalidInput(
                "message consumption must start its run in the same command".into(),
            ));
        }
    }
    Ok(())
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            Value::Object(Map::from_iter(sorted))
        }
        scalar => scalar,
    }
}

fn load_events_page(
    transaction: &Transaction<'_>,
    after_offset: u64,
    limit: usize,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT global_offset, task_id, stream_sequence, event_id, event_type,
                schema_version, recorded_at, source_json, payload_json
         FROM event_log
         WHERE global_offset > ?1
         ORDER BY global_offset ASC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![
            sqlite_integer(after_offset)?,
            i64::try_from(limit).map_err(|_| TaskStoreError::IntegerOutOfRange)?
        ],
        |row| {
            Ok(StoredTaskEvent {
                global_offset: row.get(0)?,
                task_id: row.get(1)?,
                stream_sequence: row.get(2)?,
                event_id: row.get(3)?,
                event_type: row.get(4)?,
                schema_version: row.get(5)?,
                recorded_at: row.get(6)?,
                source_json: row.get(7)?,
                payload_json: row.get(8)?,
            })
        },
    )?;
    rows.map(|row| {
        row.map_err(TaskStoreError::from)
            .and_then(StoredTaskEvent::decode)
    })
    .collect()
}

struct StoredTaskEvent {
    global_offset: i64,
    task_id: String,
    stream_sequence: i64,
    event_id: String,
    event_type: String,
    schema_version: i64,
    recorded_at: String,
    source_json: String,
    payload_json: String,
}

impl StoredTaskEvent {
    fn decode(self) -> Result<TaskEventEnvelope, TaskStoreError> {
        if self.event_type.len() > MAX_EVENT_TYPE_BYTES
            || self.source_json.len() > MAX_SOURCE_JSON_BYTES
            || self.payload_json.len() > MAX_EVENT_PAYLOAD_BYTES
        {
            return Err(TaskStoreError::InvalidInput(
                "persisted task event exceeds its encoded size limit".into(),
            ));
        }
        Ok(TaskEventEnvelope {
            global_offset: nonnegative_integer(self.global_offset)?,
            task_id: TaskId::parse(&self.task_id)?,
            stream_sequence: nonnegative_integer(self.stream_sequence)?,
            event_id: EventId::parse(&self.event_id)?,
            event_type: self.event_type,
            schema_version: u16::try_from(self.schema_version)
                .map_err(|_| TaskStoreError::IntegerOutOfRange)?,
            recorded_at: DateTime::parse_from_rfc3339(&self.recorded_at)
                .map_err(|error| TaskStoreError::InvalidTimestamp(error.to_string()))?
                .with_timezone(&Utc),
            source: serde_json::from_str(&self.source_json)?,
            payload: serde_json::from_str(&self.payload_json)?,
        })
    }
}

fn sqlite_integer(value: u64) -> Result<i64, TaskStoreError> {
    i64::try_from(value).map_err(|_| TaskStoreError::IntegerOutOfRange)
}

fn nonnegative_integer(value: i64) -> Result<u64, TaskStoreError> {
    u64::try_from(value).map_err(|_| TaskStoreError::IntegerOutOfRange)
}

#[derive(Debug, Error)]
pub enum TaskStoreError {
    #[error("wrong expected task stream version: expected {expected}, actual {actual}")]
    WrongExpectedVersion { expected: u64, actual: u64 },
    #[error("wrong expected engine event offset: expected {expected}, actual {actual}")]
    EngineOffsetMismatch { expected: u64, actual: u64 },
    #[error("command {command_id} conflicts with an existing inbox entry")]
    CommandConflict { command_id: CommandId },
    #[error("command {command_id} has no durable outcome")]
    IncompleteCommand { command_id: CommandId },
    #[error("task projector failed: {0}")]
    Projector(String),
    #[error("task projection integrity check failed: {0}")]
    ProjectionIntegrity(String),
    #[error("rebuilt task projections differ from the committed projections")]
    ProjectionMismatch,
    #[error("unsupported task event {event_type} schema version {schema_version}")]
    UnsupportedEvent {
        event_type: String,
        schema_version: u16,
    },
    #[error("invalid task store input: {0}")]
    InvalidInput(String),
    #[error("task blob not found: {blob_id}")]
    BlobNotFound { blob_id: BlobId },
    #[error("task {task_id} does not own blob {blob_id}")]
    BlobOwnershipDenied { blob_id: BlobId, task_id: TaskId },
    #[error("task blob integrity check failed: {0}")]
    BlobIntegrity(String),
    #[error("task store integer is outside SQLite's signed range")]
    IntegerOutOfRange,
    #[error("task store connection lock was poisoned")]
    LockPoisoned,
    #[error("invalid task timestamp: {0}")]
    InvalidTimestamp(String),
    #[error(transparent)]
    InvalidId(#[from] IdParseError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Filesystem(#[from] harness_fs::FsError),
}

#[cfg(test)]
mod blob_reference_merge_tests {
    use harness_contracts::{BlobId, BlobRef};

    use super::*;

    #[test]
    fn repeated_blob_references_are_merged_and_conflicts_are_rejected() {
        let blob = BlobRef {
            id: BlobId::new(),
            size: 1,
            content_hash: [1; 32],
            content_type: Some("text/plain".into()),
        };
        let merged = merge_blob_references(vec![
            TaskBlobReference {
                blob_id: blob.id,
                expected: None,
            },
            TaskBlobReference {
                blob_id: blob.id,
                expected: Some(blob.clone()),
            },
            TaskBlobReference {
                blob_id: blob.id,
                expected: Some(blob.clone()),
            },
        ])
        .unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].expected, Some(blob.clone()));

        let mut conflicting = blob.clone();
        conflicting.size += 1;
        assert!(matches!(
            merge_blob_references(vec![
                TaskBlobReference {
                    blob_id: blob.id,
                    expected: Some(blob),
                },
                TaskBlobReference {
                    blob_id: conflicting.id,
                    expected: Some(conflicting),
                },
            ]),
            Err(TaskStoreError::BlobIntegrity(_))
        ));
    }
}
