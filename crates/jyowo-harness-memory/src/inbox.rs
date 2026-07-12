//! Memory candidate inbox.
//!
//! Stores proposed memory candidates pending user review.
//! Candidates flow through states: Proposed → Approved/Rejected/Promoted/Merged/Expired.
//! No unapproved candidate enters model context.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use harness_contracts::{
    MemoryActor, MemoryActorContext, MemoryCandidate, MemoryCandidateId, MemoryCandidateOperation,
    MemoryCandidateState, MemoryEvidence, MemoryEvidenceOrigin, MemoryId, MemoryKind,
    MemoryPermissionContext, MemoryPolicyDecision, MemoryRecordDraft, MemorySource,
    MemoryVisibility, SessionId, TenantId,
};
use rusqlite::{Connection, TransactionBehavior};
use thiserror::Error;

use crate::local::{schema, schema_init};
use crate::settings::{read_global_settings, read_thread_settings};
use crate::{
    default_thread_settings, visibility_matches, MemoryMetadata, MemoryPolicyEngine, MemoryRecord,
};

const MAX_MEMORY_CONTENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum MemoryCandidateMutationError {
    #[error("candidate mutation denied by policy: {0:?}")]
    PolicyDenied(MemoryPolicyDecision),
    #[error("invalid candidate mutation: {0}")]
    Invalid(String),
    #[error("memory not found: {0}")]
    NotFound(MemoryId),
    #[error("{0}")]
    Store(String),
}

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
    pub fn promote_into_memory_for_actor(
        &self,
        id: MemoryCandidateId,
        actor: &MemoryActorContext,
        policy_actor: &MemoryActor,
        permission: &MemoryPermissionContext,
    ) -> Result<(MemoryCandidate, MemoryId), MemoryCandidateMutationError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| MemoryCandidateMutationError::Store(format!("inbox lock: {e}")))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| {
                MemoryCandidateMutationError::Store(format!("begin candidate promotion: {e}"))
            })?;
        let mut candidate =
            get_candidate(&tx, self.tenant_id, id).map_err(MemoryCandidateMutationError::Store)?;
        ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])
            .map_err(MemoryCandidateMutationError::Store)?;
        validate_candidate_content(&candidate)?;
        let target_visibility = candidate_target_visibility_in_transaction(&tx, &candidate, actor)?;
        authorize_candidate_in_transaction(
            &tx,
            &candidate,
            policy_actor,
            permission,
            target_visibility.as_ref(),
        )?;
        let memory_id = apply_candidate_in_transaction(&tx, &candidate)?;
        candidate.state = MemoryCandidateState::Promoted;
        candidate.updated_at = Utc::now();
        upsert_candidate(&tx, &candidate).map_err(MemoryCandidateMutationError::Store)?;
        tx.commit().map_err(|e| {
            MemoryCandidateMutationError::Store(format!("commit candidate promotion: {e}"))
        })?;
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
        policy_actor: &MemoryActor,
        permission: &MemoryPermissionContext,
    ) -> Result<MemoryId, MemoryCandidateMutationError> {
        if record.tenant_id != self.tenant_id {
            return Err(MemoryCandidateMutationError::Invalid(format!(
                "tenant mismatch: inbox={} record={}",
                self.tenant_id, record.tenant_id
            )));
        }
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| MemoryCandidateMutationError::Store(format!("inbox lock: {e}")))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| {
                MemoryCandidateMutationError::Store(format!("begin candidate merge: {e}"))
            })?;
        let mut candidates = Vec::with_capacity(ids.len());
        for id in ids {
            let candidate = get_candidate(&tx, self.tenant_id, *id)
                .map_err(MemoryCandidateMutationError::Store)?;
            ensure_candidate_state(&candidate, &[MemoryCandidateState::Proposed])
                .map_err(MemoryCandidateMutationError::Store)?;
            candidates.push(candidate);
        }
        let evidence = derive_merged_candidate_evidence(&candidates, &record.content)
            .map_err(MemoryCandidateMutationError::Invalid)?;
        if record.metadata.evidence.as_ref() != Some(&evidence)
            || record.metadata.source != evidence.source
        {
            return Err(MemoryCandidateMutationError::Invalid(
                "merged record evidence does not match authoritative candidates".to_owned(),
            ));
        }
        authorize_write_in_transaction(
            &tx,
            self.tenant_id,
            evidence.session_id,
            policy_actor,
            &evidence,
            permission,
            &record.visibility,
        )?;
        upsert_memory_record(&tx, record).map_err(MemoryCandidateMutationError::Store)?;
        for candidate in &mut candidates {
            candidate.state = MemoryCandidateState::Merged;
            candidate.updated_at = Utc::now();
            upsert_candidate(&tx, candidate).map_err(MemoryCandidateMutationError::Store)?;
        }
        tx.commit().map_err(|e| {
            MemoryCandidateMutationError::Store(format!("commit candidate merge: {e}"))
        })?;
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
) -> Result<MemoryId, MemoryCandidateMutationError> {
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
            upsert_memory_record(conn, &record).map_err(MemoryCandidateMutationError::Store)?;
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
                .map_err(|e| {
                    MemoryCandidateMutationError::Store(format!("update candidate memory: {e}"))
                })?;
            if affected == 0 {
                return Err(MemoryCandidateMutationError::NotFound(memory_id));
            }
            mark_embedding_missing(conn, memory_id).map_err(MemoryCandidateMutationError::Store)?;
            Ok(memory_id)
        }
        MemoryCandidateOperation::Delete { memory_id } => {
            forget_memory_record(conn, candidate.tenant_id, memory_id)
                .map_err(MemoryCandidateMutationError::Store)?;
            Ok(memory_id)
        }
    }
}

fn authorize_candidate_in_transaction(
    conn: &Connection,
    candidate: &MemoryCandidate,
    actor: &MemoryActor,
    permission: &MemoryPermissionContext,
    target_visibility: Option<&MemoryVisibility>,
) -> Result<(), MemoryCandidateMutationError> {
    let decision = match candidate.operation {
        MemoryCandidateOperation::Create | MemoryCandidateOperation::Update { .. } => {
            return authorize_write_in_transaction(
                conn,
                candidate.tenant_id,
                candidate.evidence.session_id,
                actor,
                &candidate.evidence,
                permission,
                target_visibility.unwrap_or(&candidate.proposed_record.visibility),
            );
        }
        MemoryCandidateOperation::Delete { .. } => {
            let (engine, thread) =
                policy_in_transaction(conn, candidate.tenant_id, candidate.evidence.session_id)?;
            engine.evaluate_delete(&thread, actor, permission)
        }
    };
    match decision {
        MemoryPolicyDecision::Allow => Ok(()),
        denied => Err(MemoryCandidateMutationError::PolicyDenied(denied)),
    }
}

fn validate_candidate_content(
    candidate: &MemoryCandidate,
) -> Result<(), MemoryCandidateMutationError> {
    if matches!(candidate.operation, MemoryCandidateOperation::Delete { .. }) {
        return Ok(());
    }
    let content = &candidate.proposed_record.content;
    if content.trim().is_empty() {
        return Err(MemoryCandidateMutationError::Invalid(
            "memory content must not be empty".to_owned(),
        ));
    }
    if content.len() > MAX_MEMORY_CONTENT_BYTES {
        return Err(MemoryCandidateMutationError::Invalid(format!(
            "memory content must not exceed {MAX_MEMORY_CONTENT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn candidate_target_visibility_in_transaction(
    conn: &Connection,
    candidate: &MemoryCandidate,
    actor: &MemoryActorContext,
) -> Result<Option<MemoryVisibility>, MemoryCandidateMutationError> {
    match candidate.operation {
        MemoryCandidateOperation::Create => Ok(None),
        MemoryCandidateOperation::Update { memory_id }
        | MemoryCandidateOperation::Delete { memory_id } => {
            read_visible_memory_visibility(conn, candidate.tenant_id, memory_id, actor).map(Some)
        }
    }
}

fn authorize_write_in_transaction(
    conn: &Connection,
    tenant_id: TenantId,
    session_id: Option<SessionId>,
    actor: &MemoryActor,
    evidence: &MemoryEvidence,
    permission: &MemoryPermissionContext,
    visibility: &MemoryVisibility,
) -> Result<(), MemoryCandidateMutationError> {
    let (engine, thread) = policy_in_transaction(conn, tenant_id, session_id)?;
    match engine.evaluate_write(&thread, actor, evidence, permission, visibility) {
        MemoryPolicyDecision::Allow => Ok(()),
        denied => Err(MemoryCandidateMutationError::PolicyDenied(denied)),
    }
}

fn policy_in_transaction(
    conn: &Connection,
    tenant_id: TenantId,
    session_id: Option<SessionId>,
) -> Result<
    (MemoryPolicyEngine, harness_contracts::MemoryThreadSettings),
    MemoryCandidateMutationError,
> {
    let global =
        read_global_settings(conn, tenant_id).map_err(MemoryCandidateMutationError::Store)?;
    let thread = match session_id {
        Some(session_id) => read_thread_settings(conn, tenant_id, session_id)
            .map_err(MemoryCandidateMutationError::Store)?,
        None => default_thread_settings(SessionId::new()),
    };
    Ok((MemoryPolicyEngine::new(global), thread))
}

pub fn derive_merged_candidate_evidence(
    candidates: &[MemoryCandidate],
    merged_content: &str,
) -> Result<MemoryEvidence, String> {
    let evidence = candidates
        .first()
        .map(|candidate| candidate.evidence.clone())
        .ok_or_else(|| "merge candidates are missing".to_owned())?;
    let tenant_id = candidates[0].tenant_id;
    if candidates.iter().any(|candidate| {
        candidate.tenant_id != tenant_id
            || !evidence_provenance_compatible(&evidence, &candidate.evidence)
    }) {
        return Err("merge candidates have incompatible authoritative provenance".to_owned());
    }
    let mut merged = evidence;
    merged.content_hash =
        harness_contracts::ContentHash(*blake3::hash(merged_content.as_bytes()).as_bytes());
    Ok(merged)
}

fn evidence_provenance_compatible(left: &MemoryEvidence, right: &MemoryEvidence) -> bool {
    left.source == right.source
        && left.session_id == right.session_id
        && left.run_id == right.run_id
        && origin_provenance_compatible(&left.origin, &right.origin)
}

fn origin_provenance_compatible(left: &MemoryEvidenceOrigin, right: &MemoryEvidenceOrigin) -> bool {
    match (left, right) {
        (
            MemoryEvidenceOrigin::UserMessage {
                session_id: left_session,
                run_id: left_run,
                ..
            },
            MemoryEvidenceOrigin::UserMessage {
                session_id: right_session,
                run_id: right_run,
                ..
            },
        )
        | (
            MemoryEvidenceOrigin::AssistantMessage {
                session_id: left_session,
                run_id: left_run,
                ..
            },
            MemoryEvidenceOrigin::AssistantMessage {
                session_id: right_session,
                run_id: right_run,
                ..
            },
        ) => left_session == right_session && left_run == right_run,
        (
            MemoryEvidenceOrigin::SubagentOutput {
                parent_session_id: left_parent,
                child_session_id: left_child,
                run_id: left_run,
                agent_id: left_agent,
            },
            MemoryEvidenceOrigin::SubagentOutput {
                parent_session_id: right_parent,
                child_session_id: right_child,
                run_id: right_run,
                agent_id: right_agent,
            },
        ) => {
            left_parent == right_parent
                && left_child == right_child
                && left_run == right_run
                && left_agent == right_agent
        }
        (
            MemoryEvidenceOrigin::BuiltinToolOutput {
                tool_name: left_tool,
                ..
            },
            MemoryEvidenceOrigin::BuiltinToolOutput {
                tool_name: right_tool,
                ..
            },
        ) => left_tool == right_tool,
        (
            MemoryEvidenceOrigin::McpToolOutput {
                server_id: left_server,
                tool_name: left_tool,
                ..
            },
            MemoryEvidenceOrigin::McpToolOutput {
                server_id: right_server,
                tool_name: right_tool,
                ..
            },
        ) => left_server == right_server && left_tool == right_tool,
        (
            MemoryEvidenceOrigin::PluginOutput {
                plugin_id: left_plugin,
                tool_name: left_tool,
                ..
            },
            MemoryEvidenceOrigin::PluginOutput {
                plugin_id: right_plugin,
                tool_name: right_tool,
                ..
            },
        ) => left_plugin == right_plugin && left_tool == right_tool,
        (
            MemoryEvidenceOrigin::WebRetrieval {
                url_hash: left_url, ..
            },
            MemoryEvidenceOrigin::WebRetrieval {
                url_hash: right_url,
                ..
            },
        ) => left_url == right_url,
        (
            MemoryEvidenceOrigin::WorkspaceFile {
                workspace_id: left_workspace,
                path_hash: left_path,
                snapshot_id: left_snapshot,
            },
            MemoryEvidenceOrigin::WorkspaceFile {
                workspace_id: right_workspace,
                path_hash: right_path,
                snapshot_id: right_snapshot,
            },
        ) => {
            left_workspace == right_workspace
                && left_path == right_path
                && left_snapshot == right_snapshot
        }
        (
            MemoryEvidenceOrigin::Imported {
                importer: left_importer,
                import_id: left_id,
            },
            MemoryEvidenceOrigin::Imported {
                importer: right_importer,
                import_id: right_id,
            },
        ) => left_importer == right_importer && left_id == right_id,
        (
            MemoryEvidenceOrigin::Consolidated { from: left_from },
            MemoryEvidenceOrigin::Consolidated { from: right_from },
        ) => left_from == right_from,
        _ => false,
    }
}

fn read_visible_memory_visibility(
    conn: &Connection,
    tenant_id: TenantId,
    memory_id: MemoryId,
    actor: &MemoryActorContext,
) -> Result<MemoryVisibility, MemoryCandidateMutationError> {
    if actor.tenant_id != tenant_id {
        return Err(MemoryCandidateMutationError::NotFound(memory_id));
    }
    let now = Utc::now().to_rfc3339();
    let visibility_json: String = match conn.query_row(
        &format!(
            "SELECT visibility FROM {} WHERE id = ?1 AND tenant_id = ?2 \
             AND deleted_at IS NULL AND (expires_at IS NULL OR expires_at > ?3)",
            schema::TABLE_MEMORY_RECORDS
        ),
        rusqlite::params![memory_id.to_string(), tenant_id.to_string(), now],
        |row| row.get(0),
    ) {
        Ok(visibility) => visibility,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(MemoryCandidateMutationError::NotFound(memory_id));
        }
        Err(error) => {
            return Err(MemoryCandidateMutationError::Store(format!(
                "read memory visibility: {error}"
            )));
        }
    };
    let visibility: MemoryVisibility = serde_json::from_str(&visibility_json).map_err(|e| {
        MemoryCandidateMutationError::Store(format!("decode memory visibility: {e}"))
    })?;
    if visibility_matches(&visibility, actor) {
        Ok(visibility)
    } else {
        Err(MemoryCandidateMutationError::NotFound(memory_id))
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
