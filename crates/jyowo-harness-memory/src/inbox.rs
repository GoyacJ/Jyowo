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
    MemoryEvidence, MemoryId, MemoryKind, MemoryRecordDraft, MemorySource, TenantId,
};
use rusqlite::{Connection, TransactionBehavior};

use crate::local::{schema, schema_init};
use crate::{MemoryMetadata, MemoryRecord};

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

    /// Apply a proposed candidate and mark it promoted in one SQLite transaction.
    pub fn promote_into_memory(
        &self,
        id: MemoryCandidateId,
    ) -> Result<(MemoryCandidate, MemoryId), String> {
        let mut conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("begin candidate promotion: {e}"))?;
        let mut candidate = get_candidate(&tx, self.tenant_id, id)?;
        ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])?;
        let memory_id = apply_candidate_in_transaction(&tx, &candidate)?;
        candidate.state = MemoryCandidateState::Promoted;
        candidate.updated_at = Utc::now();
        upsert_candidate(&tx, &candidate)?;
        tx.commit()
            .map_err(|e| format!("commit candidate promotion: {e}"))?;
        Ok((candidate, memory_id))
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

    /// Insert a merged record and transition every source candidate atomically.
    pub fn merge_into_memory(
        &self,
        ids: &[MemoryCandidateId],
        record: &MemoryRecord,
    ) -> Result<MemoryId, String> {
        if record.tenant_id != self.tenant_id {
            return Err(format!(
                "tenant mismatch: inbox={} record={}",
                self.tenant_id, record.tenant_id
            ));
        }
        let mut conn = self.conn.lock().map_err(|e| format!("inbox lock: {e}"))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("begin candidate merge: {e}"))?;
        let mut candidates = Vec::with_capacity(ids.len());
        for id in ids {
            let candidate = get_candidate(&tx, self.tenant_id, *id)?;
            ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])?;
            candidates.push(candidate);
        }
        upsert_memory_record(&tx, record)?;
        for candidate in &mut candidates {
            candidate.state = MemoryCandidateState::Merged;
            candidate.updated_at = Utc::now();
            upsert_candidate(&tx, candidate)?;
        }
        tx.commit()
            .map_err(|e| format!("commit candidate merge: {e}"))?;
        Ok(record.id)
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

fn apply_candidate_in_transaction(
    conn: &Connection,
    candidate: &MemoryCandidate,
) -> Result<MemoryId, String> {
    match candidate.operation {
        MemoryCandidateOperation::Create => {
            let now = Utc::now();
            let record = MemoryRecord {
                id: MemoryId::new(),
                tenant_id: candidate.tenant_id,
                kind: candidate.proposed_record.kind.clone(),
                visibility: candidate.proposed_record.visibility.clone(),
                content: candidate.proposed_record.content.clone(),
                metadata: MemoryMetadata {
                    tags: candidate.proposed_record.metadata.tags.clone(),
                    source: candidate.evidence.source.clone(),
                    evidence: Some(candidate.evidence.clone()),
                    confidence: candidate
                        .proposed_record
                        .metadata
                        .source_trust
                        .clamp(0.0, 1.0) as f32,
                    access_count: 0,
                    last_accessed_at: None,
                    recall_score: 0.0,
                    recall_score_breakdown: None,
                    ttl: candidate.proposed_record.metadata.ttl,
                    redacted_segments: 0,
                },
                created_at: now,
                updated_at: now,
            };
            upsert_memory_record(conn, &record)?;
            Ok(record.id)
        }
        MemoryCandidateOperation::Update { memory_id } => {
            let content_hash = blake3::hash(candidate.proposed_record.content.as_bytes())
                .to_hex()
                .to_string();
            let now = Utc::now().to_rfc3339();
            let affected = conn
                .execute(
                    &format!(
                        "UPDATE {} SET content = ?1, content_hash = ?2, updated_at = ?3 \
                         WHERE id = ?4 AND tenant_id = ?5 AND deleted_at IS NULL \
                         AND (expires_at IS NULL OR expires_at > ?3)",
                        schema::TABLE_MEMORY_RECORDS
                    ),
                    rusqlite::params![
                        candidate.proposed_record.content,
                        content_hash,
                        now,
                        memory_id.to_string(),
                        candidate.tenant_id.to_string(),
                    ],
                )
                .map_err(|e| format!("update candidate memory: {e}"))?;
            if affected == 0 {
                return Err(format!("memory not found: {memory_id}"));
            }
            mark_embedding_missing(conn, memory_id)?;
            Ok(memory_id)
        }
        MemoryCandidateOperation::Delete { memory_id } => {
            forget_memory_record(conn, candidate.tenant_id, memory_id)?;
            Ok(memory_id)
        }
    }
}

fn upsert_memory_record(conn: &Connection, record: &MemoryRecord) -> Result<(), String> {
    let content_hash = blake3::hash(record.content.as_bytes()).to_hex().to_string();
    let metadata_json =
        serde_json::to_string(&record.metadata).map_err(|e| format!("serialize metadata: {e}"))?;
    let evidence_json = record
        .metadata
        .evidence
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| format!("serialize evidence: {e}"))?
        .unwrap_or_else(|| "{}".to_owned());
    let tombstone_count: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM {} WHERE tenant_id = ?1 \
                 AND (memory_id = ?2 OR content_hash = ?3 OR (?4 <> '{{}}' AND evidence_json = ?4))",
                schema::TABLE_MEMORY_TOMBSTONES
            ),
            rusqlite::params![
                record.tenant_id.to_string(),
                record.id.to_string(),
                content_hash,
                evidence_json,
            ],
            |row| row.get(0),
        )
        .map_err(|e| format!("candidate tombstone check: {e}"))?;
    if tombstone_count > 0 {
        return Err("memory write denied by tombstone barrier".to_owned());
    }
    let expires_at = record.metadata.ttl.map(|ttl| {
        (record.created_at + chrono::Duration::from_std(ttl).unwrap_or_default()).to_rfc3339()
    });
    conn.execute(
        &format!(
            "INSERT INTO {} (id, tenant_id, kind, visibility, content, metadata_json, content_hash, \
             source_kind, evidence_json, confidence, access_count, created_at, updated_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) \
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, visibility = excluded.visibility, \
             content = excluded.content, metadata_json = excluded.metadata_json, \
             content_hash = excluded.content_hash, source_kind = excluded.source_kind, \
             evidence_json = excluded.evidence_json, confidence = excluded.confidence, \
             updated_at = excluded.updated_at, expires_at = excluded.expires_at",
            schema::TABLE_MEMORY_RECORDS
        ),
        rusqlite::params![
            record.id.to_string(),
            record.tenant_id.to_string(),
            memory_kind_to_db(&record.kind),
            serde_json::to_string(&record.visibility).map_err(|e| format!("serialize visibility: {e}"))?,
            record.content,
            metadata_json,
            content_hash,
            memory_source_to_db(&record.metadata.source),
            evidence_json,
            record.metadata.confidence,
            record.metadata.access_count,
            record.created_at.to_rfc3339(),
            Utc::now().to_rfc3339(),
            expires_at,
        ],
    )
    .map_err(|e| format!("write candidate memory: {e}"))?;
    mark_embedding_missing(conn, record.id)
}

fn forget_memory_record(
    conn: &Connection,
    tenant_id: TenantId,
    memory_id: MemoryId,
) -> Result<(), String> {
    let (content_hash, evidence_json): (String, String) = conn
        .query_row(
            &format!(
                "SELECT content_hash, evidence_json FROM {} WHERE id = ?1 AND tenant_id = ?2",
                schema::TABLE_MEMORY_RECORDS
            ),
            rusqlite::params![memory_id.to_string(), tenant_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("candidate forget lookup: {e}"))?;
    conn.execute(
        &format!(
            "DELETE FROM {} WHERE id = ?1 AND tenant_id = ?2",
            schema::TABLE_MEMORY_RECORDS
        ),
        rusqlite::params![memory_id.to_string(), tenant_id.to_string()],
    )
    .map_err(|e| format!("candidate forget: {e}"))?;
    conn.execute(
        &format!(
            "INSERT INTO {} (id, tenant_id, memory_id, content_hash, reason, evidence_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            schema::TABLE_MEMORY_TOMBSTONES
        ),
        rusqlite::params![
            MemoryId::new().to_string(),
            tenant_id.to_string(),
            memory_id.to_string(),
            content_hash,
            "user_requested",
            evidence_json,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| format!("candidate forget tombstone: {e}"))?;
    Ok(())
}

fn mark_embedding_missing(conn: &Connection, memory_id: MemoryId) -> Result<(), String> {
    conn.execute(
        &format!(
            "INSERT INTO {} (memory_id, embedding_state, dimension, vector_le_f32, model_id, updated_at, error_kind) \
             VALUES (?1, ?2, NULL, NULL, NULL, ?3, NULL) \
             ON CONFLICT(memory_id) DO UPDATE SET embedding_state = excluded.embedding_state, \
             dimension = NULL, vector_le_f32 = NULL, model_id = NULL, \
             updated_at = excluded.updated_at, error_kind = NULL",
            schema::TABLE_MEMORY_EMBEDDINGS
        ),
        rusqlite::params![
            memory_id.to_string(),
            schema::EMBEDDING_STATE_MISSING,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| format!("mark candidate embedding missing: {e}"))?;
    Ok(())
}

fn memory_kind_to_db(kind: &MemoryKind) -> &str {
    match kind {
        MemoryKind::UserPreference => "user_preference",
        MemoryKind::Feedback => "feedback",
        MemoryKind::ProjectFact => "project_fact",
        MemoryKind::Reference => "reference",
        MemoryKind::AgentSelfNote => "agent_self_note",
        MemoryKind::Custom(value) => value.as_str(),
        _ => "unknown",
    }
}

fn memory_source_to_db(source: &MemorySource) -> &str {
    match source {
        MemorySource::UserInput => "user_input",
        MemorySource::AgentDerived => "agent_derived",
        MemorySource::SubagentDerived { .. } => "subagent_derived",
        MemorySource::ToolOutput => "tool_output",
        MemorySource::McpToolOutput => "mcp_tool_output",
        MemorySource::PluginOutput => "plugin_output",
        MemorySource::WebRetrieval => "web_retrieval",
        MemorySource::WorkspaceFile => "workspace_file",
        MemorySource::ExternalRetrieval => "external_retrieval",
        MemorySource::Imported => "imported",
        MemorySource::Consolidated { .. } => "consolidated",
        _ => "unknown",
    }
}
