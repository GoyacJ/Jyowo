//! SQLite schema for the daemon's unified task store.

use rusqlite::Connection;

use crate::TaskStoreError;

pub(crate) fn initialize_task_schema(connection: &Connection) -> Result<(), TaskStoreError> {
    connection.execute_batch(TASK_SCHEMA)?;
    Ok(())
}

const TASK_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE IF NOT EXISTS event_log (
    global_offset INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    stream_sequence INTEGER NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    recorded_at TEXT NOT NULL,
    source_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(task_id, stream_sequence)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_event_log_task_stream
    ON event_log(task_id, stream_sequence);

CREATE TABLE IF NOT EXISTS command_inbox (
    command_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    command_hash TEXT NOT NULL,
    expected_stream_version INTEGER NOT NULL,
    status TEXT NOT NULL,
    accepted_at TEXT NOT NULL,
    completed_at TEXT,
    outcome_json TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS task_projection (
    task_id TEXT PRIMARY KEY,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS run_projection (
    task_id TEXT NOT NULL,
    run_segment_id TEXT NOT NULL,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    PRIMARY KEY(task_id, run_segment_id)
) STRICT;

CREATE TABLE IF NOT EXISTS queue_projection (
    task_id TEXT NOT NULL,
    queue_item_id TEXT NOT NULL,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    PRIMARY KEY(task_id, queue_item_id)
) STRICT;

CREATE TABLE IF NOT EXISTS permission_projection (
    task_id TEXT NOT NULL,
    permission_request_id TEXT NOT NULL,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    PRIMARY KEY(task_id, permission_request_id)
) STRICT;

CREATE TABLE IF NOT EXISTS subagent_projection (
    task_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    PRIMARY KEY(task_id, actor_id)
) STRICT;

CREATE TABLE IF NOT EXISTS workspace_projection (
    task_id TEXT NOT NULL,
    workspace_lease_id TEXT NOT NULL,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    PRIMARY KEY(task_id, workspace_lease_id)
) STRICT;

CREATE TABLE IF NOT EXISTS timeline_projection (
    task_id TEXT NOT NULL,
    global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    PRIMARY KEY(task_id, global_offset)
) STRICT;

CREATE TABLE IF NOT EXISTS checkpoints (
    checkpoint_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    run_segment_id TEXT NOT NULL,
    committed_global_offset INTEGER NOT NULL,
    checkpoint_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS blob_metadata (
    blob_id TEXT PRIMARY KEY,
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS workspace_leases (
    workspace_lease_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    canonical_root TEXT NOT NULL,
    mode TEXT NOT NULL,
    writable INTEGER NOT NULL,
    state TEXT NOT NULL,
    acquired_at TEXT,
    expires_at TEXT,
    lease_json TEXT NOT NULL
) STRICT;
"#;
