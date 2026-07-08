use rusqlite::Connection;

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
    if has_table(conn, &old_marker_table())? {
        return Err(unsupported_store_shape("old memory marker table"));
    }
    if has_user_tables(conn)? {
        validate_current_schema(conn)
    } else {
        create_current_schema(conn)
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

        ",
    )
}

fn validate_current_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    for table in [
        "memory_records",
        "memory_records_fts",
        "memory_embeddings",
        "memory_tombstones",
        "memory_candidates",
        "memory_extraction_jobs",
        "memory_recall_traces",
        "memory_global_settings",
        "memory_thread_settings",
        "memory_model_request_previews",
    ] {
        if !has_table(conn, table)? {
            return Err(unsupported_store_shape(format!("missing table {table}")));
        }
    }

    for (table, columns) in [
        (
            "memory_records",
            &[
                "id",
                "tenant_id",
                "kind",
                "visibility",
                "content",
                "metadata_json",
                "content_hash",
                "source_kind",
                "evidence_json",
                "confidence",
                "access_count",
                "last_accessed_at",
                "created_at",
                "updated_at",
                "expires_at",
                "deleted_at",
            ][..],
        ),
        (
            "memory_records_fts",
            &["content", "metadata_text", "memory_id", "tenant_id"][..],
        ),
        (
            "memory_embeddings",
            &[
                "memory_id",
                "embedding_state",
                "dimension",
                "vector_le_f32",
                "model_id",
                "updated_at",
                "error_kind",
            ][..],
        ),
        (
            "memory_tombstones",
            &[
                "id",
                "tenant_id",
                "memory_id",
                "content_hash",
                "reason",
                "evidence_json",
                "created_at",
            ][..],
        ),
        (
            "memory_candidates",
            &[
                "id",
                "tenant_id",
                "state",
                "candidate_json",
                "created_at",
                "updated_at",
                "expires_at",
            ][..],
        ),
        (
            "memory_extraction_jobs",
            &[
                "job_id",
                "tenant_id",
                "session_id",
                "run_id",
                "evidence_hash",
                "job_kind",
                "state",
                "attempt_count",
                "lease_owner",
                "lease_expires_at",
                "next_attempt_at",
                "blocked_reason",
                "job_json",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "memory_recall_traces",
            &[
                "trace_id",
                "tenant_id",
                "session_id",
                "run_id",
                "trace_json",
                "at",
            ][..],
        ),
        (
            "memory_global_settings",
            &["tenant_id", "settings_json", "updated_at"][..],
        ),
        (
            "memory_thread_settings",
            &["tenant_id", "session_id", "settings_json", "updated_at"][..],
        ),
        (
            "memory_model_request_previews",
            &[
                "tenant_id",
                "session_id",
                "run_id",
                "trace_id",
                "preview_json",
                "at",
            ][..],
        ),
    ] {
        for column in columns {
            if !has_column(conn, table, column)? {
                return Err(unsupported_store_shape(format!(
                    "missing column {table}.{column}"
                )));
            }
        }
    }

    Ok(())
}

fn has_user_tables(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table'
          AND name NOT LIKE 'sqlite_%'
        ",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn has_table(conn: &Connection, table: &str) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, rusqlite::Error> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn old_marker_table() -> String {
    ["schema", "version"].join("_")
}

fn unsupported_store_shape(details: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(format!(
        "unsupported memory store shape: {}",
        details.into()
    ))
}
