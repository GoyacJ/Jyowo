//! Unified SQLite event store for daemon tasks.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use harness_contracts::{now, EventId, EventSource, IdParseError, TaskEventEnvelope, TaskId};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;
use thiserror::Error;

use crate::task_schema::initialize_task_schema;

#[derive(Debug, Clone, PartialEq)]
pub struct NewTaskEvent {
    pub event_type: String,
    pub schema_version: u16,
    pub payload: Value,
}

pub struct TaskStore {
    connection: Mutex<Connection>,
}

impl TaskStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path = crate::app_controlled_path(path)?;
        let connection = Connection::open(path)?;
        initialize_task_schema(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn append(
        &self,
        task_id: TaskId,
        expected_version: u64,
        source: EventSource,
        events: Vec<NewTaskEvent>,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual_sql: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(stream_sequence), 0) FROM event_log WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        let actual = nonnegative_integer(actual_sql)?;
        if actual != expected_version {
            return Err(TaskStoreError::WrongExpectedVersion {
                expected: expected_version,
                actual,
            });
        }

        let source_json = serde_json::to_string(&source)?;
        let mut committed = Vec::with_capacity(events.len());
        for (index, event) in events.into_iter().enumerate() {
            let stream_sequence = expected_version
                .checked_add(index as u64 + 1)
                .ok_or(TaskStoreError::IntegerOutOfRange)?;
            let stream_sequence_sql = sqlite_integer(stream_sequence)?;
            let event_id = EventId::new();
            let recorded_at = now();
            let payload_json = serde_json::to_string(&event.payload)?;
            transaction.execute(
                "INSERT INTO event_log (
                    task_id, stream_sequence, event_id, event_type, schema_version,
                    recorded_at, source_json, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    task_id.to_string(),
                    stream_sequence_sql,
                    event_id.to_string(),
                    event.event_type,
                    i64::from(event.schema_version),
                    recorded_at.to_rfc3339(),
                    source_json,
                    payload_json,
                ],
            )?;
            let global_offset = transaction.last_insert_rowid() as u64;
            committed.push(TaskEventEnvelope {
                global_offset,
                task_id,
                stream_sequence,
                event_id,
                event_type: event.event_type,
                schema_version: event.schema_version,
                recorded_at,
                source: source.clone(),
                payload: event.payload,
            });
        }
        transaction.commit()?;
        Ok(committed)
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
        let limit =
            i64::try_from(limit.clamp(1, 1000)).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
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

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
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
