use rusqlite::{Connection, OptionalExtension};

pub const CURRENT_SCHEMA_VERSION: i64 = 4;

pub fn initialize(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    if let Err(error) = initialize_locked(conn) {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(error);
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

fn initialize_locked(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
    )?;

    let current_version: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .optional()?;

    match current_version {
        None | Some(0) => create_current_schema(conn),
        Some(CURRENT_SCHEMA_VERSION) => Ok(()),
        Some(existing) => Err(rusqlite::Error::InvalidParameterName(format!(
            "unsupported memory schema version {existing}"
        ))),
    }
}

fn create_current_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE memory_records (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            visibility TEXT NOT NULL,
            content TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            content_hash TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            evidence_json TEXT NOT NULL DEFAULT '{}',
            confidence REAL NOT NULL DEFAULT 1.0,
            access_count INTEGER NOT NULL DEFAULT 0,
            last_accessed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            expires_at TEXT,
            deleted_at TEXT
        );

        CREATE INDEX idx_memory_records_tenant_visibility
            ON memory_records(tenant_id, visibility);

        CREATE INDEX idx_memory_records_tenant_content_hash
            ON memory_records(tenant_id, content_hash);

        CREATE INDEX idx_memory_records_tenant_expires
            ON memory_records(tenant_id, expires_at);

        CREATE INDEX idx_memory_records_tenant_deleted
            ON memory_records(tenant_id, deleted_at);

        CREATE INDEX idx_memory_records_tenant_last_accessed
            ON memory_records(tenant_id, last_accessed_at);

        CREATE VIRTUAL TABLE memory_records_fts USING fts5(
            content,
            metadata_text,
            memory_id UNINDEXED,
            tenant_id UNINDEXED,
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE TRIGGER memory_records_fts_ai
        AFTER INSERT ON memory_records
        BEGIN
            INSERT INTO memory_records_fts(content, metadata_text, memory_id, tenant_id)
            VALUES (new.content, new.metadata_json, new.id, new.tenant_id);
        END;

        CREATE TRIGGER memory_records_fts_au
        AFTER UPDATE OF content, metadata_json, id, tenant_id ON memory_records
        BEGIN
            DELETE FROM memory_records_fts WHERE memory_id = old.id;
            INSERT INTO memory_records_fts(content, metadata_text, memory_id, tenant_id)
            VALUES (new.content, new.metadata_json, new.id, new.tenant_id);
        END;

        CREATE TRIGGER memory_records_fts_ad
        AFTER DELETE ON memory_records
        BEGIN
            DELETE FROM memory_records_fts WHERE memory_id = old.id;
        END;

        CREATE TABLE memory_embeddings (
            memory_id TEXT PRIMARY KEY REFERENCES memory_records(id) ON DELETE CASCADE,
            embedding_state TEXT NOT NULL CHECK (embedding_state IN ('missing', 'ready', 'failed', 'disabled')),
            dimension INTEGER,
            vector_le_f32 BLOB,
            model_id TEXT,
            updated_at TEXT NOT NULL,
            error_kind TEXT
        );

        CREATE TABLE memory_tombstones (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            memory_id TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            reason TEXT NOT NULL,
            evidence_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );

        CREATE INDEX idx_memory_tombstones_tenant
            ON memory_tombstones(tenant_id, memory_id);

        CREATE TABLE memory_candidates (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            state TEXT NOT NULL,
            candidate_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            expires_at TEXT
        );

        CREATE INDEX idx_memory_candidates_tenant_state
            ON memory_candidates(tenant_id, state, updated_at);

        CREATE TABLE memory_extraction_jobs (
            job_id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            evidence_hash BLOB NOT NULL,
            job_kind TEXT NOT NULL,
            state TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            lease_owner TEXT,
            lease_expires_at TEXT,
            next_attempt_at TEXT,
            blocked_reason TEXT,
            job_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(tenant_id, session_id, run_id, evidence_hash, job_kind)
        );

        CREATE INDEX idx_memory_extraction_jobs_available
            ON memory_extraction_jobs(state, next_attempt_at, lease_expires_at, created_at);

        CREATE TABLE memory_recall_traces (
            trace_id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            trace_json TEXT NOT NULL,
            at TEXT NOT NULL
        );

        CREATE INDEX idx_memory_recall_traces_session_run
            ON memory_recall_traces(tenant_id, session_id, run_id, at);

        CREATE TABLE memory_global_settings (
            tenant_id TEXT PRIMARY KEY,
            settings_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE memory_thread_settings (
            tenant_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            settings_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (tenant_id, session_id)
        );

        CREATE TABLE memory_model_request_previews (
            tenant_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            trace_id TEXT NOT NULL DEFAULT '',
            preview_json TEXT NOT NULL,
            at TEXT NOT NULL,
            PRIMARY KEY (tenant_id, session_id, run_id, trace_id)
        );

        CREATE INDEX idx_memory_model_request_previews_session_run
            ON memory_model_request_previews(tenant_id, session_id, run_id, at);

        INSERT INTO schema_version (version, applied_at)
        VALUES (4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
        ",
    )
}
