//! Memory candidate inbox.
//!
//! Stores proposed memory candidates pending user review.
//! Candidates flow through states: Proposed → Approved/Rejected/Promoted/Merged/Expired.
//! No unapproved candidate enters model context.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use harness_contracts::{
    MemoryCandidate, MemoryCandidateId, MemoryCandidateOperation, MemoryCandidateState,
    MemoryEvidence, MemoryRecordDraft, TenantId,
};
use rusqlite::Connection;

use crate::local::{schema, schema_init};

/// SQLite-backed candidate inbox for a single tenant.
#[derive(Debug)]
pub struct MemoryInbox {
    tenant_id: TenantId,
    conn: Mutex<Connection>,
}

impl MemoryInbox {
    #[must_use]
    pub fn new(tenant_id: TenantId) -> Self {
        let conn = open_memory_connection().expect("open in-memory memory inbox");
        Self {
            tenant_id,
            conn: Mutex::new(conn),
        }
    }

    pub fn open(db_path: &str, tenant_id: TenantId) -> Result<Self, String> {
        let conn = open_file_connection(db_path)?;
        Ok(Self {
            tenant_id,
            conn: Mutex::new(conn),
        })
    }

    /// Propose a new memory candidate.
    pub fn propose(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        self.propose_with_operation(MemoryCandidateOperation::Create, draft, evidence)
    }

    pub fn propose_with_operation(
        &self,
        operation: MemoryCandidateOperation,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        let now = Utc::now();
        let candidate = MemoryCandidate {
            id: MemoryCandidateId::new(),
            tenant_id: self.tenant_id,
            state: MemoryCandidateState::Proposed,
            operation,
            proposed_record: draft,
            evidence,
            created_at: now,
            updated_at: now,
            expires_at: None,
        };

        upsert_candidate(&conn, &candidate)?;
        Ok(candidate)
    }

    /// Approve a candidate (move to Approved state).
    pub fn approve(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        let mut candidate = get_candidate(&conn, self.tenant_id, id)?;

        ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])?;
        candidate.state = MemoryCandidateState::Approved;
        candidate.updated_at = Utc::now();
        upsert_candidate(&conn, &candidate)?;
        Ok(candidate.clone())
    }

    /// Reject a candidate (move to Rejected state).
    pub fn reject(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        let mut candidate = get_candidate(&conn, self.tenant_id, id)?;

        ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])?;
        candidate.state = MemoryCandidateState::Rejected;
        candidate.updated_at = Utc::now();
        upsert_candidate(&conn, &candidate)?;
        Ok(candidate.clone())
    }

    /// Mark a reviewed candidate as promoted into long-term memory.
    pub fn promote(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        let mut candidate = get_candidate(&conn, self.tenant_id, id)?;

        ensure_candidate_state(
            &candidate,
            &[
                MemoryCandidateState::Proposed,
                MemoryCandidateState::Approved,
            ],
        )?;
        candidate.state = MemoryCandidateState::Promoted;
        candidate.updated_at = Utc::now();
        upsert_candidate(&conn, &candidate)?;
        Ok(candidate.clone())
    }

    /// Mark a candidate as merged into a new long-term memory record.
    pub fn merge(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        let mut candidate = get_candidate(&conn, self.tenant_id, id)?;

        ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])?;
        candidate.state = MemoryCandidateState::Merged;
        candidate.updated_at = Utc::now();
        upsert_candidate(&conn, &candidate)?;
        Ok(candidate.clone())
    }

    /// List candidates, optionally filtered by state.
    pub fn list(
        &self,
        state: Option<MemoryCandidateState>,
    ) -> Result<Vec<MemoryCandidate>, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        list_candidates(&conn, self.tenant_id, state)
    }

    /// Import a candidate as a proposed inbox item.
    pub fn import(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        let conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;

        let now = Utc::now();
        let candidate = MemoryCandidate {
            id: MemoryCandidateId::new(),
            tenant_id: self.tenant_id,
            state: MemoryCandidateState::Proposed,
            operation: MemoryCandidateOperation::Create,
            proposed_record: draft,
            evidence,
            created_at: now,
            updated_at: now,
            expires_at: None,
        };

        upsert_candidate(&conn, &candidate)?;
        Ok(candidate)
    }
}

/// Marker trait for inbox storage backends.
pub trait InboxStore: Send + Sync + 'static {
    fn propose(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String>;

    fn approve(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String>;

    fn reject(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String>;

    fn merge(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String>;

    fn list(&self, state: Option<MemoryCandidateState>) -> Result<Vec<MemoryCandidate>, String>;
}

impl InboxStore for MemoryInbox {
    fn propose(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        self.propose(draft, evidence)
    }

    fn approve(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        self.approve(id)
    }

    fn reject(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        self.reject(id)
    }

    fn merge(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        self.merge(id)
    }

    fn list(&self, state: Option<MemoryCandidateState>) -> Result<Vec<MemoryCandidate>, String> {
        self.list(state)
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
    schema_init::initialize(conn).map_err(|e| format!("initialize schema: {e}"))
}

fn upsert_candidate(conn: &Connection, candidate: &MemoryCandidate) -> Result<(), String> {
    let candidate_json =
        serde_json::to_string(candidate).map_err(|e| format!("serialize candidate: {e}"))?;
    conn.execute(
        "INSERT INTO memory_candidates (id, tenant_id, state, candidate_json, created_at, updated_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           state = excluded.state,
           candidate_json = excluded.candidate_json,
           updated_at = excluded.updated_at,
           expires_at = excluded.expires_at",
        rusqlite::params![
            candidate.id.to_string(),
            candidate.tenant_id.to_string(),
            state_to_db(candidate.state),
            candidate_json,
            candidate.created_at.to_rfc3339(),
            candidate.updated_at.to_rfc3339(),
            candidate.expires_at.map(|at| at.to_rfc3339()),
        ],
    )
    .map_err(|e| format!("write candidate: {e}"))?;
    Ok(())
}

fn get_candidate(
    conn: &Connection,
    tenant_id: TenantId,
    id: MemoryCandidateId,
) -> Result<MemoryCandidate, String> {
    conn.query_row(
        "SELECT candidate_json FROM memory_candidates WHERE id = ?1 AND tenant_id = ?2",
        rusqlite::params![id.to_string(), tenant_id.to_string()],
        |row| {
            let json: String = row.get(0)?;
            serde_json::from_str::<MemoryCandidate>(&json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => format!("candidate not found: {id}"),
        other => format!("read candidate: {other}"),
    })
}

fn ensure_candidate_state(
    candidate: &MemoryCandidate,
    allowed: &[MemoryCandidateState],
) -> Result<(), String> {
    if allowed.iter().any(|state| *state == candidate.state) {
        return Ok(());
    }

    Err(format!("candidate is not proposed: {}", candidate.id))
}

fn list_candidates(
    conn: &Connection,
    tenant_id: TenantId,
    state: Option<MemoryCandidateState>,
) -> Result<Vec<MemoryCandidate>, String> {
    let mut sql = "SELECT candidate_json FROM memory_candidates WHERE tenant_id = ?1".to_owned();
    if state.is_some() {
        sql.push_str(" AND state = ?2");
    }
    sql.push_str(" ORDER BY created_at ASC");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("prepare list candidates: {e}"))?;

    let map_row = |row: &rusqlite::Row<'_>| {
        let json: String = row.get(0)?;
        serde_json::from_str::<MemoryCandidate>(&json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    };

    let rows = if let Some(state) = state {
        stmt.query_map(
            rusqlite::params![tenant_id.to_string(), state_to_db(state)],
            map_row,
        )
    } else {
        stmt.query_map(rusqlite::params![tenant_id.to_string()], map_row)
    }
    .map_err(|e| format!("query candidates: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("decode candidates: {e}"))
}

fn state_to_db(state: MemoryCandidateState) -> String {
    serde_json::to_string(&state)
        .unwrap_or_else(|_| "\"proposed\"".to_owned())
        .trim_matches('"')
        .to_owned()
}
