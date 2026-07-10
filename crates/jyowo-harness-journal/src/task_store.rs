//! Unified `SQLite` event store for daemon tasks.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use harness_contracts::{
    now, ClientId, CommandId, EventId, EventSource, EventSourceKind, IdParseError,
    TaskEventEnvelope, TaskId, TaskProjection,
};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::task_event::{NewTaskEvent, MAX_EVENT_PAYLOAD_BYTES};
use crate::task_projection::{
    empty_task_projection, load_task_projection_row, projection_counts, projection_snapshot,
    ProjectionCounts, SynchronousTaskProjector, TaskProjector, PROJECTION_TABLES,
};
use crate::task_schema::initialize_task_schema;

const MAX_COMMAND_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_EVENTS_PER_TRANSACTION: usize = 256;
const MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION: usize = 8 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_EVENT_TYPE_BYTES: usize = 128;
const MAX_SOURCE_JSON_BYTES: usize = 4096;
const MAX_READ_PAGE_SIZE: usize = 16;
const REBUILD_PAGE_SIZE: usize = 16;

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
    InvalidCommand { message: String },
}

pub struct TaskStore {
    connection: Mutex<Connection>,
    projector: Arc<dyn TaskProjector>,
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
        Ok(Self {
            connection: Mutex::new(connection),
            projector,
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
    pub(crate) fn supervisor_authority() -> EventAuthority {
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
        validate_events(command.authority.source(), &events)?;
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

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
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
                CommandRejection::InvalidCommand { .. }
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
