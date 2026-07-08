//! Local SQLite memory provider.
//!
//! Production default provider. Uses SQLite with FTS5 for lexical search,
//! enforces TTL, tenant isolation, visibility filtering, tombstone checks,
//! and optional embedding vector storage.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    MemoryError, MemoryId, MemoryKind, MemoryScoreBreakdown, MemoryVisibility, TenantId,
};
use rusqlite::{Connection, Error as SqliteError, ErrorCode, Transaction, TransactionBehavior};
use tokio::sync::Mutex;

use crate::local::embedding::{
    cosine_similarity, deserialize_vector_le, serialize_vector_le, MemoryEmbeddingProvider,
};
use crate::local::ranking::{self, RankScore};
use crate::local::schema;
use crate::local::schema_init;
use crate::{
    content_preview, visibility_matches, MemoryKindFilter, MemoryLifecycle, MemoryListScope,
    MemoryMetadata, MemoryQuery, MemoryRecord, MemoryStore, MemorySummary, MemoryVisibilityFilter,
};

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const SQLITE_INIT_RETRY_ATTEMPTS: usize = 50;
const SQLITE_INIT_RETRY_DELAY: Duration = Duration::from_millis(100);
static SQLITE_INIT_LOCK: StdMutex<()> = StdMutex::new(());

/// Options for opening a local memory provider.
#[derive(Clone)]
pub struct LocalMemoryOptions {
    /// Maximum records to return per recall query.
    pub max_records_per_recall: u32,
    /// Optional embedding provider for semantic search.
    pub embedding_provider: Option<Arc<dyn MemoryEmbeddingProvider>>,
}

impl Default for LocalMemoryOptions {
    fn default() -> Self {
        Self {
            max_records_per_recall: 50,
            embedding_provider: None,
        }
    }
}

/// Local SQLite-backed memory provider.
///
/// Implements `MemoryStore` and `MemoryLifecycle`. This is the production
/// default provider when memory is enabled.
pub struct LocalMemoryProvider {
    conn: Mutex<Connection>,
    tenant_id: TenantId,
    options: LocalMemoryOptions,
}

impl LocalMemoryProvider {
    /// Open a local memory provider at the given SQLite path.
    ///
    /// Creates the current schema on open. If the database file does not exist,
    /// it will be created.
    pub fn open(db_path: &str, tenant_id: TenantId) -> Result<Self, MemoryError> {
        Self::open_with_options(db_path, tenant_id, LocalMemoryOptions::default())
    }

    /// Open with explicit options.
    pub fn open_with_options(
        db_path: &str,
        tenant_id: TenantId,
        options: LocalMemoryOptions,
    ) -> Result<Self, MemoryError> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| MemoryError::Message(format!("failed to create db directory: {e}")))?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| MemoryError::Message(format!("failed to open sqlite database: {e}")))?;
        conn.busy_timeout(SQLITE_BUSY_TIMEOUT)
            .map_err(|e| MemoryError::Message(format!("failed to set sqlite busy timeout: {e}")))?;

        let _init_guard = SQLITE_INIT_LOCK
            .lock()
            .map_err(|_| MemoryError::Message("sqlite init lock poisoned".to_owned()))?;

        // Apply PRAGMAs
        for pragma in schema::CONNECTION_PRAGMAS {
            retry_sqlite_init(|| conn.execute_batch(pragma))
                .map_err(|e| MemoryError::Message(format!("failed to set pragma: {e}")))?;
        }
        ensure_wal_journal_mode(&conn)
            .map_err(|e| MemoryError::Message(format!("failed to set sqlite journal mode: {e}")))?;

        retry_sqlite_init(|| schema_init::initialize(&conn))
            .map_err(|e| MemoryError::Message(format!("schema initialization failed: {e}")))?;

        Ok(Self {
            conn: Mutex::new(conn),
            tenant_id,
            options,
        })
    }
}

fn ensure_wal_journal_mode(conn: &Connection) -> Result<(), SqliteError> {
    let current_mode: String =
        retry_sqlite_init(|| conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)))?;
    if current_mode.eq_ignore_ascii_case("wal") {
        return Ok(());
    }

    let updated_mode: String =
        retry_sqlite_init(|| conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0)))?;
    if updated_mode.eq_ignore_ascii_case("wal") {
        Ok(())
    } else {
        Err(SqliteError::ExecuteReturnedResults)
    }
}

fn retry_sqlite_init<T, F>(mut operation: F) -> Result<T, SqliteError>
where
    F: FnMut() -> Result<T, SqliteError>,
{
    for attempt in 1..=SQLITE_INIT_RETRY_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if sqlite_locked_or_busy(&error) && attempt < SQLITE_INIT_RETRY_ATTEMPTS => {
                thread::sleep(SQLITE_INIT_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("sqlite init retry loop returns on final attempt");
}

fn sqlite_locked_or_busy(error: &SqliteError) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

#[async_trait]
impl MemoryStore for LocalMemoryProvider {
    fn provider_id(&self) -> &str {
        "local"
    }

    async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        if query.tenant_id != self.tenant_id {
            return Err(MemoryError::Message(format!(
                "tenant mismatch: provider={} query={}",
                self.tenant_id, query.tenant_id
            )));
        }

        let conn = self.conn.lock().await;
        let now = Utc::now();

        // Build FTS query — use simple match syntax
        let fts_query = build_fts_query(&query.text);

        let recall_limit = query
            .max_records
            .min(self.options.max_records_per_recall)
            .max(1);
        let query_embedding = self
            .options
            .embedding_provider
            .as_ref()
            .and_then(|provider| provider.embed(&query.text));
        let now_text = now.to_rfc3339();

        let mut candidates: HashMap<String, CandidateRow> = HashMap::new();
        for (row, rank) in fts_rows(&conn, &fts_query, self.tenant_id, &now_text)? {
            if !kind_filter_matches_record(&row, query.kind_filter.as_ref()) {
                continue;
            }
            if !visibility_filter_matches_record(&row, &query.visibility_filter) {
                continue;
            }
            let lexical_score = ranking::normalize_fts_rank(rank);
            candidates.insert(
                row.id.clone(),
                CandidateRow {
                    row,
                    lexical_score,
                    vector_score: None,
                },
            );
        }

        if let Some(query_vector) = query_embedding.as_deref() {
            for row in all_candidate_rows(&conn, self.tenant_id, &now_text)? {
                if !kind_filter_matches_record(&row, query.kind_filter.as_ref()) {
                    continue;
                }
                if !visibility_filter_matches_record(&row, &query.visibility_filter) {
                    continue;
                }
                let Some(vector_score) = embedding_score_for_record(&conn, &row.id, query_vector)?
                else {
                    continue;
                };
                candidates
                    .entry(row.id.clone())
                    .and_modify(|candidate| {
                        candidate.vector_score = Some(vector_score);
                    })
                    .or_insert(CandidateRow {
                        row,
                        lexical_score: 0.0,
                        vector_score: Some(vector_score),
                    });
            }
        }

        let mut results: Vec<MemoryRecord> = candidates
            .into_values()
            .filter(|candidate| recall_match_score(candidate) >= query.min_similarity)
            .map(|candidate| {
                let mut record = row_to_memory_record(&candidate.row)?;
                let mut score = RankScore {
                    lexical_score: candidate.lexical_score,
                    vector_score: candidate.vector_score,
                    confidence_score: record.metadata.confidence,
                    recency_score: ranking::recency_score(record.updated_at, now),
                    access_score: ranking::access_score(record.metadata.access_count),
                    source_trust_score: source_trust_score(&record.metadata.source),
                    explicit_selection_boost: 0.0,
                    final_score: 0.0,
                };
                score.final_score = ranking::compute_final_score(&score);
                record.metadata.recall_score = score.final_score;
                record.metadata.recall_score_breakdown = Some(rank_score_breakdown(&score));
                Ok(record)
            })
            .collect::<Result<Vec<_>, MemoryError>>()?;

        results.sort_by(|a, b| {
            b.metadata
                .recall_score
                .partial_cmp(&a.metadata.recall_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(recall_limit as usize);

        // Update access counters for returned records
        for record in &mut results {
            let _ = conn.execute(
                "UPDATE memory_records SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2",
                rusqlite::params![now.to_rfc3339(), record.id.to_string()],
            );
            record.metadata.access_count = record.metadata.access_count.saturating_add(1);
            record.metadata.last_accessed_at = Some(now);
        }

        Ok(results)
    }

    async fn get(&self, id: MemoryId) -> Result<MemoryRecord, MemoryError> {
        let conn = self.conn.lock().await;
        let now = Utc::now();

        let row: MemoryRecordRow = conn
            .query_row(
                &format!(
                    "SELECT {} FROM {} WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL AND (expires_at IS NULL OR expires_at > ?3)",
                    schema::RECORD_COLUMNS_BARE,
                    schema::TABLE_MEMORY_RECORDS,
                ),
                rusqlite::params![id.to_string(), self.tenant_id.to_string(), now.to_rfc3339()],
                |row| row_to_record(row),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => MemoryError::NotFound(id),
                other => MemoryError::Message(format!("get failed: {other}")),
            })?;

        row_to_memory_record(&row)
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        if record.tenant_id != self.tenant_id {
            return Err(MemoryError::Message(format!(
                "tenant mismatch: provider={} record={}",
                self.tenant_id, record.tenant_id
            )));
        }

        let mut conn = self.conn.lock().await;
        let id = record.id;
        let now = Utc::now().to_rfc3339();
        let content_hash = blake3::hash(record.content.as_bytes()).to_hex().to_string();
        let embedding = match self.options.embedding_provider.as_ref() {
            Some(provider) => match provider.embed(&record.content) {
                Some(vector) if vector.len() == provider.dimension() => {
                    Some(Some((provider.model_id().to_owned(), vector)))
                }
                Some(vector) => {
                    return Err(MemoryError::Message(format!(
                        "embedding dimension mismatch: provider={} expected={} actual={}",
                        provider.model_id(),
                        provider.dimension(),
                        vector.len()
                    )));
                }
                None => Some(None),
            },
            None => None,
        };
        let metadata_json =
            serde_json::to_string(&record.metadata).unwrap_or_else(|_| "{}".to_owned());
        let evidence_json = record
            .metadata
            .evidence
            .as_ref()
            .and_then(|evidence| serde_json::to_string(evidence).ok())
            .unwrap_or_else(|| "{}".to_owned());
        let content = record.content.clone();
        let tenant_id = record.tenant_id;

        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| MemoryError::Message(format!("begin upsert transaction failed: {e}")))?;

        let tombstone_count: i64 = tx
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM {} \
                     WHERE tenant_id = ?1 \
                       AND (memory_id = ?2 OR content_hash = ?3 OR (?4 <> '{{}}' AND evidence_json = ?4))",
                    schema::TABLE_MEMORY_TOMBSTONES,
                ),
                rusqlite::params![
                    tenant_id.to_string(),
                    id.to_string(),
                    &content_hash,
                    &evidence_json
                ],
                |row| row.get(0),
            )
            .map_err(|e| MemoryError::Message(format!("tombstone check failed: {e}")))?;
        if tombstone_count > 0 {
            return Err(MemoryError::Message(
                "memory write denied by tombstone barrier".to_owned(),
            ));
        }

        // Compute expires_at from TTL
        let expires_at = record.metadata.ttl.map(|ttl| {
            (record.created_at + chrono::Duration::from_std(ttl).unwrap_or_default()).to_rfc3339()
        });

        tx.execute(
            &format!(
                r#"INSERT INTO {} (id, tenant_id, kind, visibility, content, metadata_json, content_hash,
                   source_kind, evidence_json, confidence, access_count, created_at, updated_at, expires_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                   ON CONFLICT(id) DO UPDATE SET
                   kind = excluded.kind,
                   visibility = excluded.visibility,
                   content = excluded.content,
                   metadata_json = excluded.metadata_json,
                   content_hash = excluded.content_hash,
                   source_kind = excluded.source_kind,
                   evidence_json = excluded.evidence_json,
                   confidence = excluded.confidence,
                   updated_at = excluded.updated_at,
                   expires_at = excluded.expires_at"#,
                schema::TABLE_MEMORY_RECORDS,
            ),
            rusqlite::params![
                id.to_string(),
                tenant_id.to_string(),
                kind_to_str(&record.kind),
                visibility_to_str(&record.visibility),
                content,
                metadata_json,
                content_hash,
                source_to_str(&record.metadata.source),
                evidence_json,
                record.metadata.confidence,
                record.metadata.access_count,
                record.created_at.to_rfc3339(),
                now,
                expires_at,
            ],
        )
        .map_err(|e| MemoryError::Message(format!("upsert failed: {e}")))?;

        upsert_embedding(&tx, id, embedding)
            .map_err(|e| MemoryError::Message(format!("upsert embedding failed: {e}")))?;

        tx.commit()
            .map_err(|e| MemoryError::Message(format!("commit upsert transaction failed: {e}")))?;

        Ok(id)
    }

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError> {
        let mut conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| MemoryError::Message(format!("begin forget transaction failed: {e}")))?;

        let (content_hash, evidence_json): (String, String) = tx
            .query_row(
                &format!(
                    "SELECT content_hash, evidence_json FROM {} WHERE id = ?1 AND tenant_id = ?2",
                    schema::TABLE_MEMORY_RECORDS,
                ),
                rusqlite::params![id.to_string(), self.tenant_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => MemoryError::NotFound(id),
                other => MemoryError::Message(format!("forget lookup failed: {other}")),
            })?;

        // Hard-delete the memory record in the same transaction as its tombstone.
        let affected = tx
            .execute(
                &format!(
                    "DELETE FROM {} WHERE id = ?1 AND tenant_id = ?2",
                    schema::TABLE_MEMORY_RECORDS,
                ),
                rusqlite::params![id.to_string(), self.tenant_id.to_string()],
            )
            .map_err(|e| MemoryError::Message(format!("forget failed: {e}")))?;

        if affected == 0 {
            return Err(MemoryError::NotFound(id));
        }

        // Record a tombstone
        tx.execute(
            &format!(
                "INSERT INTO {} (id, tenant_id, memory_id, content_hash, reason, evidence_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                schema::TABLE_MEMORY_TOMBSTONES,
            ),
            rusqlite::params![
                MemoryId::new().to_string(),
                self.tenant_id.to_string(),
                id.to_string(),
                content_hash,
                "user_requested",
                evidence_json,
                now,
            ],
        )
        .map_err(|e| MemoryError::Message(format!("forget tombstone failed: {e}")))?;

        tx.commit()
            .map_err(|e| MemoryError::Message(format!("commit forget transaction failed: {e}")))?;

        Ok(())
    }

    async fn rollback_uncommitted_upsert(&self, id: MemoryId) -> Result<(), MemoryError> {
        let mut conn = self.conn.lock().await;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| {
                MemoryError::Message(format!("begin rollback upsert transaction failed: {e}"))
            })?;

        tx.execute(
            &format!(
                "DELETE FROM {} WHERE id = ?1 AND tenant_id = ?2",
                schema::TABLE_MEMORY_RECORDS,
            ),
            rusqlite::params![id.to_string(), self.tenant_id.to_string()],
        )
        .map_err(|e| MemoryError::Message(format!("rollback upsert failed: {e}")))?;

        tx.commit().map_err(|e| {
            MemoryError::Message(format!("commit rollback upsert transaction failed: {e}"))
        })?;
        Ok(())
    }

    async fn rollback_uncommitted_forget(&self, record: MemoryRecord) -> Result<(), MemoryError> {
        if record.tenant_id != self.tenant_id {
            return Err(MemoryError::Message(format!(
                "tenant mismatch: provider={} record={}",
                self.tenant_id, record.tenant_id
            )));
        }

        let content_hash = blake3::hash(record.content.as_bytes()).to_hex().to_string();
        {
            let mut conn = self.conn.lock().await;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e| {
                    MemoryError::Message(format!("begin rollback forget transaction failed: {e}"))
                })?;
            tx.execute(
                &format!(
                    "DELETE FROM {} WHERE tenant_id = ?1 AND (memory_id = ?2 OR content_hash = ?3)",
                    schema::TABLE_MEMORY_TOMBSTONES,
                ),
                rusqlite::params![
                    self.tenant_id.to_string(),
                    record.id.to_string(),
                    content_hash
                ],
            )
            .map_err(|e| {
                MemoryError::Message(format!("rollback forget tombstone cleanup failed: {e}"))
            })?;
            tx.commit().map_err(|e| {
                MemoryError::Message(format!("commit rollback forget transaction failed: {e}"))
            })?;
        }

        self.upsert(record).await.map(|_| ())
    }

    async fn list(&self, scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();

        let base_sql = format!(
            "SELECT {} FROM {} WHERE tenant_id = ?1 AND deleted_at IS NULL AND (expires_at IS NULL OR expires_at > ?2)",
            schema::RECORD_COLUMNS_BARE,
            schema::TABLE_MEMORY_RECORDS,
        );

        let rows: Vec<MemoryRecordRow> = conn
            .prepare(&base_sql)
            .map_err(|e| MemoryError::Message(format!("list prepare failed: {e}")))?
            .query_map(rusqlite::params![self.tenant_id.to_string(), now], |row| {
                row_to_record(row)
            })
            .map_err(|e| MemoryError::Message(format!("list query failed: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Message(format!("list row decode failed: {e}")))?;

        let summaries: Vec<MemorySummary> = rows
            .into_iter()
            .map(|row| {
                let record = row_to_memory_record(&row)?;
                Ok(list_scope_filter(&record, &scope, self.provider_id()))
            })
            .collect::<Result<Vec<_>, MemoryError>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(summaries)
    }
}

impl MemoryLifecycle for LocalMemoryProvider {}

impl crate::MemoryProvider for LocalMemoryProvider {
    fn descriptor(&self) -> crate::MemoryProviderDescriptor {
        crate::MemoryProviderDescriptor {
            provider_id: self.provider_id().to_owned(),
            provider_kind: harness_contracts::MemoryProviderKind::Local,
            priority: 0,
            trust_level: harness_contracts::MemoryProviderTrust::BuiltIn,
            tenant_scope: None,
            workspace_scope: None,
            durability: harness_contracts::MemoryProviderDurability::Durable,
            readable: true,
            writable: true,
            allowed_visibility: vec![
                harness_contracts::MemoryVisibilityClass::Private,
                harness_contracts::MemoryVisibilityClass::User,
                harness_contracts::MemoryVisibilityClass::Team,
                harness_contracts::MemoryVisibilityClass::Tenant,
            ],
            supports_evidence: true,
            supports_raw_content_export: true,
            timeout_ms: 5000,
            max_records_per_recall: self.options.max_records_per_recall,
            max_chars_per_recall: 100_000,
            max_bytes_per_record: 1024 * 1024,
        }
    }
}

// ── SQL row helpers ──

struct CandidateRow {
    row: MemoryRecordRow,
    lexical_score: f32,
    vector_score: Option<f32>,
}

fn recall_match_score(candidate: &CandidateRow) -> f32 {
    // FTS5 rank is a lexical relevance signal, not calibrated semantic
    // similarity. Keep lexical-only matches from satisfying near-exact
    // `min_similarity` gates while still letting normal recall floors work.
    let lexical_similarity = candidate.lexical_score.min(0.95);
    candidate
        .vector_score
        .unwrap_or(0.0)
        .max(lexical_similarity)
}

#[derive(Clone)]
struct MemoryRecordRow {
    id: String,
    tenant_id: String,
    kind: String,
    visibility: String,
    content: String,
    metadata_json: String,
    _content_hash: String,
    _source_kind: String,
    evidence_json: String,
    confidence: f64,
    access_count: i64,
    last_accessed_at: Option<String>,
    created_at: String,
    updated_at: String,
    _expires_at: Option<String>,
    _deleted_at: Option<String>,
}

fn fts_rows(
    conn: &Connection,
    fts_query: &str,
    tenant_id: TenantId,
    now: &str,
) -> Result<Vec<(MemoryRecordRow, f64)>, MemoryError> {
    let sql = format!(
        r#"SELECT {cols}, fts.rank
           FROM {fts_table} fts
           JOIN {records} r ON fts.memory_id = r.id
           WHERE {fts_table} MATCH ?1
             AND r.tenant_id = ?2
             AND r.deleted_at IS NULL
             AND (r.expires_at IS NULL OR r.expires_at > ?3)
           ORDER BY fts.rank"#,
        cols = schema::RECORD_COLUMNS,
        fts_table = schema::TABLE_MEMORY_RECORDS_FTS,
        records = schema::TABLE_MEMORY_RECORDS,
    );

    conn.prepare(&sql)
        .map_err(|e| MemoryError::Message(format!("recall prepare failed: {e}")))?
        .query_map(
            rusqlite::params![fts_query, tenant_id.to_string(), now],
            |row| {
                let record = row_to_record(row)?;
                let rank: f64 = row.get(16)?;
                Ok((record, rank))
            },
        )
        .map_err(|e| MemoryError::Message(format!("recall query failed: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| MemoryError::Message(format!("recall row decode failed: {e}")))
}

fn all_candidate_rows(
    conn: &Connection,
    tenant_id: TenantId,
    now: &str,
) -> Result<Vec<MemoryRecordRow>, MemoryError> {
    let sql = format!(
        "SELECT {} FROM {} WHERE tenant_id = ?1 AND deleted_at IS NULL AND (expires_at IS NULL OR expires_at > ?2)",
        schema::RECORD_COLUMNS_BARE,
        schema::TABLE_MEMORY_RECORDS,
    );

    conn.prepare(&sql)
        .map_err(|e| MemoryError::Message(format!("semantic recall prepare failed: {e}")))?
        .query_map(rusqlite::params![tenant_id.to_string(), now], |row| {
            row_to_record(row)
        })
        .map_err(|e| MemoryError::Message(format!("semantic recall query failed: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| MemoryError::Message(format!("semantic recall row decode failed: {e}")))
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<MemoryRecordRow> {
    Ok(MemoryRecordRow {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        kind: row.get(2)?,
        visibility: row.get(3)?,
        content: row.get(4)?,
        metadata_json: row.get(5)?,
        _content_hash: row.get(6)?,
        _source_kind: row.get(7)?,
        evidence_json: row.get(8)?,
        confidence: row.get(9)?,
        access_count: row.get(10)?,
        last_accessed_at: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        _expires_at: row.get(14)?,
        _deleted_at: row.get(15)?,
    })
}

fn row_to_memory_record(row: &MemoryRecordRow) -> Result<MemoryRecord, MemoryError> {
    let mut metadata: MemoryMetadata = serde_json::from_str(&row.metadata_json)
        .map_err(|e| MemoryError::Message(format!("invalid memory metadata json: {e}")))?;
    let evidence_value: serde_json::Value = serde_json::from_str(&row.evidence_json)
        .map_err(|e| MemoryError::Message(format!("invalid memory evidence json: {e}")))?;
    metadata.evidence = if evidence_value
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        None
    } else {
        Some(
            serde_json::from_value(evidence_value)
                .map_err(|e| MemoryError::Message(format!("invalid memory evidence json: {e}")))?,
        )
    };
    metadata.confidence = row.confidence as f32;
    metadata.access_count = row.access_count.max(0) as u32;
    metadata.last_accessed_at = row
        .last_accessed_at
        .as_deref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| MemoryError::Message(format!("invalid last_accessed_at: {e}")))
        })
        .transpose()?;

    Ok(MemoryRecord {
        id: MemoryId::parse(&row.id)
            .map_err(|e| MemoryError::Message(format!("invalid memory id: {e}")))?,
        tenant_id: TenantId::parse(&row.tenant_id)
            .map_err(|e| MemoryError::Message(format!("invalid tenant id: {e}")))?,
        kind: str_to_kind(&row.kind),
        visibility: parse_visibility(&row.visibility)?,
        content: row.content.clone(),
        metadata,
        created_at: row
            .created_at
            .parse()
            .map_err(|e| MemoryError::Message(format!("invalid created_at: {e}")))?,
        updated_at: row
            .updated_at
            .parse()
            .map_err(|e| MemoryError::Message(format!("invalid updated_at: {e}")))?,
    })
}

fn rank_score_breakdown(score: &RankScore) -> MemoryScoreBreakdown {
    MemoryScoreBreakdown {
        lexical_score: score.lexical_score,
        vector_score: score.vector_score,
        confidence_score: score.confidence_score,
        recency_score: score.recency_score,
        access_score: score.access_score,
        source_trust_score: score.source_trust_score,
        explicit_selection_boost: score.explicit_selection_boost,
        final_score: score.final_score,
    }
}

fn upsert_embedding(
    tx: &Transaction<'_>,
    memory_id: MemoryId,
    embedding: Option<Option<(String, Vec<f32>)>>,
) -> Result<(), rusqlite::Error> {
    let now = Utc::now().to_rfc3339();
    match embedding {
        Some(Some((model_id, vector))) => tx.execute(
            &format!(
                "INSERT INTO {} (memory_id, embedding_state, dimension, vector_le_f32, model_id, updated_at, error_kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
                 ON CONFLICT(memory_id) DO UPDATE SET
                   embedding_state = excluded.embedding_state,
                   dimension = excluded.dimension,
                   vector_le_f32 = excluded.vector_le_f32,
                   model_id = excluded.model_id,
                   updated_at = excluded.updated_at,
                   error_kind = NULL",
                schema::TABLE_MEMORY_EMBEDDINGS,
            ),
            rusqlite::params![
                memory_id.to_string(),
                schema::EMBEDDING_STATE_READY,
                vector.len() as i64,
                serialize_vector_le(&vector),
                model_id,
                now,
            ],
        ),
        Some(None) => tx.execute(
            &format!(
                "INSERT INTO {} (memory_id, embedding_state, dimension, vector_le_f32, model_id, updated_at, error_kind)
                 VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)
                 ON CONFLICT(memory_id) DO UPDATE SET
                   embedding_state = excluded.embedding_state,
                   dimension = NULL,
                   vector_le_f32 = NULL,
                   model_id = NULL,
                   updated_at = excluded.updated_at,
                   error_kind = excluded.error_kind",
                schema::TABLE_MEMORY_EMBEDDINGS,
            ),
            rusqlite::params![
                memory_id.to_string(),
                schema::EMBEDDING_STATE_FAILED,
                now,
                "embedding_unavailable",
            ],
        ),
        None => tx.execute(
            &format!(
                "INSERT INTO {} (memory_id, embedding_state, dimension, vector_le_f32, model_id, updated_at, error_kind)
                 VALUES (?1, ?2, NULL, NULL, NULL, ?3, NULL)
                 ON CONFLICT(memory_id) DO UPDATE SET
                   embedding_state = excluded.embedding_state,
                   dimension = NULL,
                   vector_le_f32 = NULL,
                   model_id = NULL,
                   updated_at = excluded.updated_at,
                   error_kind = NULL",
                schema::TABLE_MEMORY_EMBEDDINGS,
            ),
            rusqlite::params![memory_id.to_string(), schema::EMBEDDING_STATE_MISSING, now],
        ),
    }?;
    Ok(())
}

fn embedding_score_for_record(
    conn: &Connection,
    memory_id: &str,
    query_vector: &[f32],
) -> Result<Option<f32>, MemoryError> {
    let row = match conn.query_row(
        &format!(
            "SELECT dimension, vector_le_f32 FROM {} WHERE memory_id = ?1 AND embedding_state = ?2",
            schema::TABLE_MEMORY_EMBEDDINGS,
        ),
        rusqlite::params![memory_id, schema::EMBEDDING_STATE_READY],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
    ) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => {
            return Err(MemoryError::Message(format!(
                "embedding lookup failed: {error}"
            )));
        }
    };
    let (dimension, vector_bytes) = row;
    if dimension < 0 || dimension as usize != query_vector.len() {
        return Err(MemoryError::Message(format!(
            "embedding dimension mismatch: memory_id={memory_id} stored={dimension} query={}",
            query_vector.len()
        )));
    }
    let record_vector = deserialize_vector_le(&vector_bytes).ok_or_else(|| {
        MemoryError::Message(format!(
            "invalid embedding vector bytes: memory_id={memory_id}"
        ))
    })?;
    if record_vector.len() != dimension as usize {
        return Err(MemoryError::Message(format!(
            "embedding dimension mismatch: memory_id={memory_id} stored={dimension} vector={}",
            record_vector.len()
        )));
    }
    cosine_similarity(query_vector, &record_vector)
        .map(Some)
        .ok_or_else(|| {
            MemoryError::Message(format!(
                "embedding dimension mismatch: memory_id={memory_id} query={} vector={}",
                query_vector.len(),
                record_vector.len()
            ))
        })
}

// ── String conversion helpers ──

fn kind_to_str(kind: &MemoryKind) -> &str {
    match kind {
        MemoryKind::UserPreference => "user_preference",
        MemoryKind::Feedback => "feedback",
        MemoryKind::ProjectFact => "project_fact",
        MemoryKind::Reference => "reference",
        MemoryKind::AgentSelfNote => "agent_self_note",
        MemoryKind::Custom(s) => s.as_str(),
        _ => "unknown",
    }
}

fn str_to_kind(s: &str) -> MemoryKind {
    match s {
        "user_preference" => MemoryKind::UserPreference,
        "feedback" => MemoryKind::Feedback,
        "project_fact" => MemoryKind::ProjectFact,
        "reference" => MemoryKind::Reference,
        "agent_self_note" => MemoryKind::AgentSelfNote,
        other => MemoryKind::Custom(other.to_owned()),
    }
}

fn visibility_to_str(v: &MemoryVisibility) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "tenant".to_owned())
}

fn str_to_visibility(s: &str) -> MemoryVisibility {
    serde_json::from_str(s).unwrap_or(MemoryVisibility::Tenant)
}

fn parse_visibility(s: &str) -> Result<MemoryVisibility, MemoryError> {
    serde_json::from_str(s)
        .map_err(|e| MemoryError::Message(format!("invalid memory visibility: {e}")))
}

fn source_to_str(s: &harness_contracts::MemorySource) -> &str {
    match s {
        harness_contracts::MemorySource::UserInput => "user_input",
        harness_contracts::MemorySource::AgentDerived => "agent_derived",
        harness_contracts::MemorySource::SubagentDerived { .. } => "subagent_derived",
        harness_contracts::MemorySource::ToolOutput => "tool_output",
        harness_contracts::MemorySource::McpToolOutput => "mcp_tool_output",
        harness_contracts::MemorySource::PluginOutput => "plugin_output",
        harness_contracts::MemorySource::WebRetrieval => "web_retrieval",
        harness_contracts::MemorySource::WorkspaceFile => "workspace_file",
        harness_contracts::MemorySource::ExternalRetrieval => "external_retrieval",
        harness_contracts::MemorySource::Imported => "imported",
        harness_contracts::MemorySource::Consolidated { .. } => "consolidated",
        _ => "unknown",
    }
}

fn source_trust_score(source: &harness_contracts::MemorySource) -> f32 {
    match source {
        harness_contracts::MemorySource::UserInput => 0.9,
        harness_contracts::MemorySource::Imported => 0.5,
        harness_contracts::MemorySource::AgentDerived => 0.6,
        harness_contracts::MemorySource::Consolidated { .. } => 0.7,
        harness_contracts::MemorySource::WorkspaceFile => 0.75,
        harness_contracts::MemorySource::ToolOutput
        | harness_contracts::MemorySource::McpToolOutput
        | harness_contracts::MemorySource::PluginOutput => 0.55,
        harness_contracts::MemorySource::WebRetrieval
        | harness_contracts::MemorySource::ExternalRetrieval => 0.45,
        harness_contracts::MemorySource::SubagentDerived { .. } => 0.5,
        _ => 0.5,
    }
}

fn build_fts_query(text: &str) -> String {
    // Simple FTS5 query: quote each term for safe matching
    let terms: Vec<String> = text
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if terms.is_empty() {
        "*".to_owned()
    } else {
        terms.join(" OR ")
    }
}

fn visibility_filter_matches_record(
    row: &MemoryRecordRow,
    filter: &MemoryVisibilityFilter,
) -> bool {
    let visibility: MemoryVisibility = str_to_visibility(&row.visibility);
    match filter {
        MemoryVisibilityFilter::EffectiveFor(actor) => {
            let tenant: TenantId = TenantId::parse(&row.tenant_id).unwrap_or(TenantId::SINGLE);
            if actor.tenant_id != tenant {
                return false;
            }
            visibility_matches(&visibility, actor)
        }
        MemoryVisibilityFilter::Exact(v) => &visibility == v,
    }
}

fn kind_filter_matches_record(row: &MemoryRecordRow, filter: Option<&MemoryKindFilter>) -> bool {
    match filter {
        None | Some(MemoryKindFilter::Any) => true,
        Some(MemoryKindFilter::OnlyKinds(kinds)) => kinds.contains(&str_to_kind(&row.kind)),
    }
}

fn list_scope_filter(
    record: &MemoryRecord,
    scope: &MemoryListScope,
    provider_id: &str,
) -> Option<MemorySummary> {
    let matches = match scope {
        MemoryListScope::All => true,
        MemoryListScope::ByKind(kind) => &record.kind == kind,
        MemoryListScope::ByVisibility(visibility) => &record.visibility == visibility,
        MemoryListScope::ForActor(actor) => {
            record.tenant_id == actor.tenant_id && visibility_matches(&record.visibility, actor)
        }
    };
    if matches {
        Some(MemorySummary {
            id: record.id,
            provider_id: Some(provider_id.to_owned()),
            kind: record.kind.clone(),
            visibility: record.visibility.clone(),
            content_preview: content_preview(&record.content),
            content_hash: harness_contracts::ContentHash(
                *blake3::hash(record.content.as_bytes()).as_bytes(),
            ),
            metadata: record.metadata.clone(),
            expires_at: record
                .metadata
                .ttl
                .and_then(|ttl| chrono::Duration::from_std(ttl).ok())
                .map(|ttl| record.created_at + ttl),
            deleted: false,
            updated_at: record.updated_at,
        })
    } else {
        None
    }
}
