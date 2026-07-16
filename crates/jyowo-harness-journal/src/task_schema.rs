//! `SQLite` schema for the daemon's unified task store.

use rusqlite::Connection;

use crate::TaskStoreError;

pub(crate) fn initialize_task_schema(connection: &Connection) -> Result<(), TaskStoreError> {
    connection.execute_batch(TASK_SCHEMA)?;
    Ok(())
}

const TASK_SCHEMA: &str = r"
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

CREATE INDEX IF NOT EXISTS idx_event_log_task_type
    ON event_log(task_id, event_type);

CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_run_segment
    ON event_log(task_id, json_extract(payload_json, '$.segmentId'))
    WHERE event_type = 'run.started';

CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_queue_item
    ON event_log(task_id, json_extract(payload_json, '$.queueItemId'))
    WHERE event_type = 'message.queued';

CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_permission_request
    ON event_log(task_id, json_extract(payload_json, '$.requestId'))
    WHERE event_type = 'permission.requested';

CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_subagent_actor
    ON event_log(task_id, json_extract(payload_json, '$.actorId'))
    WHERE event_type = 'subagent.spawned';

CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_workspace_lease
    ON event_log(task_id, json_extract(payload_json, '$.leaseId'))
    WHERE event_type = 'workspace.acquired';

CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_engine_session_offset
    ON event_log(
        task_id,
        json_extract(payload_json, '$.tenantId'),
        json_extract(payload_json, '$.sessionId'),
        CAST(json_extract(payload_json, '$.journalOffset') AS INTEGER)
    )
    WHERE event_type GLOB 'engine.*';

CREATE INDEX IF NOT EXISTS idx_engine_run_history
    ON event_log(
        task_id,
        COALESCE(
            json_extract(payload_json, '$.runId'),
            json_extract(payload_json, '$.event.run_id')
        ),
        global_offset
    )
    WHERE event_type GLOB 'engine.*';

CREATE TABLE IF NOT EXISTS task_store_migrations (
    migration_name TEXT PRIMARY KEY,
    applied INTEGER NOT NULL CHECK(applied = 1)
) STRICT;

CREATE TABLE IF NOT EXISTS command_inbox (
    command_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    command_hash TEXT NOT NULL,
    expected_stream_version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('processing', 'accepted', 'rejected')),
    accepted_at TEXT NOT NULL,
    completed_at TEXT,
    outcome_json TEXT,
    result_stream_version INTEGER,
    committed_offset INTEGER,
    event_count INTEGER,
    UNIQUE(task_id, principal_id, idempotency_key),
    CHECK(
        (status = 'processing'
            AND completed_at IS NULL
            AND outcome_json IS NULL
            AND result_stream_version IS NULL
            AND committed_offset IS NULL
            AND event_count IS NULL)
        OR
        (status IN ('accepted', 'rejected')
            AND completed_at IS NOT NULL
            AND outcome_json IS NOT NULL
            AND result_stream_version IS NOT NULL
            AND committed_offset IS NOT NULL
            AND event_count IS NOT NULL
            AND (
                (status = 'accepted' AND event_count > 0)
                OR (status = 'rejected' AND event_count = 0)
            ))
    )
) STRICT;

CREATE TABLE IF NOT EXISTS segment_start_outbox (
    task_id TEXT NOT NULL,
    run_segment_id TEXT NOT NULL,
    request_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT,
    PRIMARY KEY(task_id, run_segment_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_segment_start_outbox_pending
    ON segment_start_outbox(task_id, delivered_at, created_at);

CREATE TABLE IF NOT EXISTS segment_execution (
    task_id TEXT NOT NULL,
    run_segment_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('in_progress', 'completed')),
    claimed_at TEXT NOT NULL,
    completed_at TEXT,
    terminal_json TEXT,
    PRIMARY KEY(task_id, run_segment_id),
    CHECK(
        (status = 'in_progress' AND completed_at IS NULL AND terminal_json IS NULL)
        OR
        (status = 'completed' AND completed_at IS NOT NULL AND terminal_json IS NOT NULL)
    )
) STRICT;

CREATE TABLE IF NOT EXISTS task_projection (
    task_id TEXT PRIMARY KEY,
    last_global_offset INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    projection_digest TEXT NOT NULL
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

CREATE TABLE IF NOT EXISTS context_summaries (
    summary_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    source_start_global_offset INTEGER NOT NULL,
    source_end_global_offset INTEGER NOT NULL,
    blob_id TEXT NOT NULL,
    active INTEGER NOT NULL CHECK(active IN (0, 1)),
    summary_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CHECK(source_start_global_offset > 0),
    CHECK(source_end_global_offset >= source_start_global_offset)
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_context_summaries_one_active
    ON context_summaries(task_id)
    WHERE active = 1;

CREATE TABLE IF NOT EXISTS blob_metadata (
    blob_id TEXT PRIMARY KEY,
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS blob_ownership (
    task_id TEXT NOT NULL,
    blob_id TEXT NOT NULL,
    media_type TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(task_id, blob_id),
    FOREIGN KEY(blob_id) REFERENCES blob_metadata(blob_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX IF NOT EXISTS idx_blob_ownership_blob
    ON blob_ownership(blob_id, task_id);

CREATE TABLE IF NOT EXISTS blob_staging (
    task_id TEXT NOT NULL,
    blob_id TEXT NOT NULL,
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    staged_at TEXT NOT NULL,
    PRIMARY KEY(task_id, blob_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_blob_staging_blob
    ON blob_staging(blob_id, task_id);

CREATE TABLE IF NOT EXISTS blob_store_config (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    store_id TEXT NOT NULL UNIQUE,
    canonical_root TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS scheduled_task_scope (
    scope_key TEXT PRIMARY KEY,
    workspace_root TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS scheduled_task_schedule_state (
    scope_key TEXT NOT NULL,
    scheduled_task_id TEXT NOT NULL,
    cursor_at TEXT NOT NULL,
    next_due_at TEXT NOT NULL,
    active_task_id TEXT,
    PRIMARY KEY(scope_key, scheduled_task_id),
    FOREIGN KEY(scope_key) REFERENCES scheduled_task_scope(scope_key) ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS scheduled_task_run (
    scope_key TEXT NOT NULL,
    run_id TEXT PRIMARY KEY,
    scheduled_task_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    record_json TEXT NOT NULL,
    FOREIGN KEY(scope_key) REFERENCES scheduled_task_scope(scope_key) ON DELETE CASCADE
) STRICT;

CREATE INDEX IF NOT EXISTS idx_scheduled_task_run_scope_started
    ON scheduled_task_run(scope_key, started_at DESC);

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
";
