//! Recall trace generation and collection.
//!
//! Traces store metadata about memory recall (IDs, scores, drop reasons,
//! provider latency, budget usage) without storing raw memory content.
//! Traces are linked to `MemoryRecalledEvent` via `trace_id`.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use harness_contracts::{
    ContentHash, MemoryCandidateTrace, MemoryDropReason, MemoryDroppedTrace, MemoryId,
    MemoryInjectedTrace, MemoryModelRequestPreview, MemoryProviderTrace, MemoryRecallTrace,
    MemoryTraceId, RunId, SessionId, TenantId,
};
use rusqlite::Connection;

use crate::local::{migrations, schema};

/// Builder for constructing a `MemoryRecallTrace` incrementally during recall.
#[derive(Debug)]
pub struct MemoryRecallTraceBuilder {
    trace_id: MemoryTraceId,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    turn: u32,
    query_text_hash: ContentHash,
    provider_results: Vec<MemoryProviderTrace>,
    candidates: Vec<MemoryCandidateTrace>,
    injected: Vec<MemoryInjectedTrace>,
    dropped: Vec<MemoryDroppedTrace>,
    redacted_count: u32,
    injected_chars: u32,
    deadline_used_ms: u32,
}

impl MemoryRecallTraceBuilder {
    #[must_use]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        turn: u32,
        query_text_hash: ContentHash,
    ) -> Self {
        Self::new_for_tenant(TenantId::SINGLE, session_id, run_id, turn, query_text_hash)
    }

    #[must_use]
    pub fn new_for_tenant(
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        turn: u32,
        query_text_hash: ContentHash,
    ) -> Self {
        Self {
            trace_id: MemoryTraceId::new(),
            tenant_id,
            session_id,
            run_id,
            turn,
            query_text_hash,
            provider_results: Vec::new(),
            candidates: Vec::new(),
            injected: Vec::new(),
            dropped: Vec::new(),
            redacted_count: 0,
            injected_chars: 0,
            deadline_used_ms: 0,
        }
    }

    pub fn trace_id(&self) -> MemoryTraceId {
        self.trace_id
    }

    pub fn add_provider_result(mut self, result: MemoryProviderTrace) -> Self {
        self.provider_results.push(result);
        self
    }

    pub fn add_candidate(mut self, candidate: MemoryCandidateTrace) -> Self {
        self.candidates.push(candidate);
        self
    }

    pub fn add_injected(
        mut self,
        memory_id: MemoryId,
        provider_id: &str,
        content_hash: ContentHash,
        injected_chars: u32,
        fence_id: &str,
    ) -> Self {
        self.injected.push(MemoryInjectedTrace {
            memory_id,
            provider_id: provider_id.to_owned(),
            content_hash,
            injected_chars,
            fence_id: fence_id.to_owned(),
        });
        self
    }

    pub fn add_dropped(
        mut self,
        reason: MemoryDropReason,
        memory_id: Option<MemoryId>,
        provider_id: Option<&str>,
    ) -> Self {
        self.dropped.push(MemoryDroppedTrace {
            memory_id,
            provider_id: provider_id.map(|s| s.to_owned()),
            content_hash: None,
            reason,
        });
        self
    }

    pub fn set_redacted(mut self, count: u32) -> Self {
        self.redacted_count = count;
        self
    }

    pub fn set_injected_chars(mut self, chars: u32) -> Self {
        self.injected_chars = chars;
        self
    }

    pub fn set_deadline_ms(mut self, ms: u32) -> Self {
        self.deadline_used_ms = ms;
        self
    }

    #[must_use]
    pub fn build(self) -> MemoryRecallTrace {
        MemoryRecallTrace {
            trace_id: self.trace_id,
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            run_id: self.run_id,
            turn: self.turn,
            query_text_hash: self.query_text_hash,
            provider_results: self.provider_results,
            candidates: self.candidates,
            injected: self.injected,
            dropped: self.dropped,
            redacted_count: self.redacted_count,
            injected_chars: self.injected_chars,
            deadline_used_ms: self.deadline_used_ms,
            at: Utc::now(),
        }
    }
}

/// SQLite-backed collector of recall traces.
#[derive(Debug)]
pub struct MemoryRecallTraceCollector {
    conn: Mutex<Connection>,
}

impl MemoryRecallTraceCollector {
    #[must_use]
    pub fn new() -> Self {
        let conn = open_memory_connection().expect("open in-memory recall trace collector");
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub fn open(db_path: &str) -> Result<Self, String> {
        let conn = open_file_connection(db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn add(&self, trace: MemoryRecallTrace) {
        if let Ok(conn) = self.conn.lock() {
            let _ = insert_trace(&conn, &trace);
        }
    }

    pub fn add_model_request_preview(
        &self,
        tenant_id: TenantId,
        preview: MemoryModelRequestPreview,
    ) {
        if let Ok(conn) = self.conn.lock() {
            let _ = insert_model_request_preview(&conn, tenant_id, &preview);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.conn
            .lock()
            .ok()
            .and_then(|conn| {
                conn.query_row("SELECT COUNT(*) FROM memory_recall_traces", [], |row| {
                    row.get::<_, i64>(0)
                })
                .ok()
            })
            .unwrap_or(0) as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn for_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Vec<MemoryRecallTrace> {
        self.conn
            .lock()
            .ok()
            .and_then(|conn| list_traces(&conn, tenant_id, Some(session_id), None).ok())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn for_run(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
    ) -> Vec<MemoryRecallTrace> {
        self.conn
            .lock()
            .ok()
            .and_then(|conn| list_traces(&conn, tenant_id, Some(session_id), Some(run_id)).ok())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn get(&self, tenant_id: TenantId, trace_id: MemoryTraceId) -> Option<MemoryRecallTrace> {
        self.conn
            .lock()
            .ok()
            .and_then(|conn| get_trace(&conn, tenant_id, trace_id).ok().flatten())
    }

    #[must_use]
    pub fn get_model_request_preview(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        trace_id: Option<MemoryTraceId>,
    ) -> Option<MemoryModelRequestPreview> {
        self.conn.lock().ok().and_then(|conn| {
            get_model_request_preview(&conn, tenant_id, session_id, run_id, trace_id)
                .ok()
                .flatten()
        })
    }

    /// List trace summaries without full detail (for IPC listing).
    #[must_use]
    pub fn list_summaries(
        &self,
        tenant_id: TenantId,
        session_id: Option<SessionId>,
        run_id: Option<RunId>,
    ) -> Vec<harness_contracts::MemoryRecallTraceSummary> {
        self.conn
            .lock()
            .ok()
            .and_then(|conn| list_traces(&conn, tenant_id, session_id, run_id).ok())
            .map(|traces| {
                traces
                    .iter()
                    .map(|t| harness_contracts::MemoryRecallTraceSummary {
                        trace_id: t.trace_id,
                        tenant_id: t.tenant_id,
                        session_id: t.session_id,
                        run_id: t.run_id,
                        injected_count: t.injected.len() as u32,
                        dropped_count: t.dropped.len() as u32,
                        redacted_count: t.redacted_count,
                        at: t.at,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn open_memory_connection() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|e| format!("open sqlite: {e}"))?;
    initialize_connection(&conn)?;
    Ok(conn)
}

fn open_file_connection(db_path: &str) -> Result<Connection, String> {
    if let Some(parent) = Path::new(db_path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create db directory: {e}"))?;
    }
    let conn = Connection::open(db_path).map_err(|e| format!("open sqlite: {e}"))?;
    initialize_connection(&conn)?;
    Ok(conn)
}

fn initialize_connection(conn: &Connection) -> Result<(), String> {
    for pragma in schema::CONNECTION_PRAGMAS {
        conn.execute_batch(pragma)
            .map_err(|e| format!("set sqlite pragma: {e}"))?;
    }
    migrations::run(conn).map_err(|e| format!("run migrations: {e}"))
}

fn insert_trace(conn: &Connection, trace: &MemoryRecallTrace) -> Result<(), String> {
    let trace_json = serde_json::to_string(trace).map_err(|e| format!("serialize trace: {e}"))?;
    conn.execute(
        "INSERT INTO memory_recall_traces (trace_id, tenant_id, session_id, run_id, trace_json, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(trace_id) DO UPDATE SET
           tenant_id = excluded.tenant_id,
           session_id = excluded.session_id,
           run_id = excluded.run_id,
           trace_json = excluded.trace_json,
           at = excluded.at",
        rusqlite::params![
            trace.trace_id.to_string(),
            trace.tenant_id.to_string(),
            trace.session_id.to_string(),
            trace.run_id.to_string(),
            trace_json,
            trace.at.to_rfc3339(),
        ],
    )
    .map_err(|e| format!("write trace: {e}"))?;
    Ok(())
}

fn insert_model_request_preview(
    conn: &Connection,
    tenant_id: TenantId,
    preview: &MemoryModelRequestPreview,
) -> Result<(), String> {
    let preview_json =
        serde_json::to_string(preview).map_err(|e| format!("serialize request preview: {e}"))?;
    let trace_id = preview
        .trace_id
        .map(|trace_id| trace_id.to_string())
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO memory_model_request_previews
           (tenant_id, session_id, run_id, trace_id, preview_json, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(tenant_id, session_id, run_id, trace_id) DO UPDATE SET
           preview_json = excluded.preview_json,
           at = excluded.at",
        rusqlite::params![
            tenant_id.to_string(),
            preview.session_id.to_string(),
            preview.run_id.to_string(),
            trace_id,
            preview_json,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| format!("write request preview: {e}"))?;
    Ok(())
}

fn get_model_request_preview(
    conn: &Connection,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    trace_id: Option<MemoryTraceId>,
) -> Result<Option<MemoryModelRequestPreview>, String> {
    let result = if let Some(trace_id) = trace_id {
        conn.query_row(
            "SELECT preview_json FROM memory_model_request_previews
             WHERE tenant_id = ?1 AND session_id = ?2 AND run_id = ?3 AND trace_id = ?4",
            rusqlite::params![
                tenant_id.to_string(),
                session_id.to_string(),
                run_id.to_string(),
                trace_id.to_string(),
            ],
            decode_model_request_preview_row,
        )
    } else {
        conn.query_row(
            "SELECT preview_json FROM memory_model_request_previews
             WHERE tenant_id = ?1 AND session_id = ?2 AND run_id = ?3
             ORDER BY at DESC LIMIT 1",
            rusqlite::params![
                tenant_id.to_string(),
                session_id.to_string(),
                run_id.to_string(),
            ],
            decode_model_request_preview_row,
        )
    };

    match result {
        Ok(preview) => Ok(Some(preview)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(format!("read request preview: {error}")),
    }
}

fn get_trace(
    conn: &Connection,
    tenant_id: TenantId,
    trace_id: MemoryTraceId,
) -> Result<Option<MemoryRecallTrace>, String> {
    let result = conn.query_row(
        "SELECT trace_json FROM memory_recall_traces WHERE tenant_id = ?1 AND trace_id = ?2",
        rusqlite::params![tenant_id.to_string(), trace_id.to_string()],
        decode_trace_row,
    );

    match result {
        Ok(trace) => Ok(Some(trace)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(format!("read trace: {error}")),
    }
}

fn list_traces(
    conn: &Connection,
    tenant_id: TenantId,
    session_id: Option<SessionId>,
    run_id: Option<RunId>,
) -> Result<Vec<MemoryRecallTrace>, String> {
    let mut sql = "SELECT trace_json FROM memory_recall_traces WHERE tenant_id = ?1".to_owned();
    match (session_id, run_id) {
        (Some(_), Some(_)) => sql.push_str(" AND session_id = ?2 AND run_id = ?3"),
        (Some(_), None) => sql.push_str(" AND session_id = ?2"),
        (None, Some(_)) => sql.push_str(" AND run_id = ?2"),
        (None, None) => {}
    }
    sql.push_str(" ORDER BY at ASC");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("prepare list traces: {e}"))?;
    let rows = match (session_id, run_id) {
        (Some(session_id), Some(run_id)) => stmt.query_map(
            rusqlite::params![
                tenant_id.to_string(),
                session_id.to_string(),
                run_id.to_string()
            ],
            decode_trace_row,
        ),
        (Some(session_id), None) => stmt.query_map(
            rusqlite::params![tenant_id.to_string(), session_id.to_string()],
            decode_trace_row,
        ),
        (None, Some(run_id)) => stmt.query_map(
            rusqlite::params![tenant_id.to_string(), run_id.to_string()],
            decode_trace_row,
        ),
        (None, None) => stmt.query_map(rusqlite::params![tenant_id.to_string()], decode_trace_row),
    }
    .map_err(|e| format!("query traces: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("decode traces: {e}"))
}

fn decode_trace_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecallTrace> {
    let json: String = row.get(0)?;
    serde_json::from_str::<MemoryRecallTrace>(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn decode_model_request_preview_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MemoryModelRequestPreview> {
    let json: String = row.get(0)?;
    serde_json::from_str::<MemoryModelRequestPreview>(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
