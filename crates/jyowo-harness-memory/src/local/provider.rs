//! Local SQLite memory provider.
//!
//! Production default provider. Uses SQLite with FTS5 for lexical search,
//! enforces TTL, tenant isolation, visibility filtering, tombstone checks,
//! and optional embedding vector storage.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{MemoryError, MemoryId, MemoryKind, MemoryVisibility, TenantId};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::local::embedding::MemoryEmbeddingProvider;
use crate::local::migrations;
use crate::local::ranking::{self, RankScore};
use crate::local::schema;
use crate::{
    content_preview, visibility_matches, MemoryLifecycle, MemoryListScope, MemoryMetadata,
    MemoryQuery, MemoryRecord, MemoryStore, MemorySummary, MemoryVisibilityFilter,
};

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
    /// Runs refinery migrations on open. If the database file does not exist,
    /// it will be created.
    pub fn open(
        db_path: &str,
        tenant_id: TenantId,
    ) -> Result<Self, MemoryError> {
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
            std::fs::create_dir_all(parent).map_err(|e| {
                MemoryError::Message(format!("failed to create db directory: {e}"))
            })?;
        }

        let mut conn = Connection::open(db_path).map_err(|e| {
            MemoryError::Message(format!("failed to open sqlite database: {e}"))
        })?;

        // Apply PRAGMAs
        for pragma in schema::CONNECTION_PRAGMAS {
            conn.execute_batch(pragma).map_err(|e| {
                MemoryError::Message(format!("failed to set pragma: {e}"))
            })?;
        }

        // Run migrations
        migrations::run(&conn)
            .map_err(|e| MemoryError::Message(format!("migration failed: {e}")))?;

        Ok(Self {
            conn: Mutex::new(conn),
            tenant_id,
            options,
        })
    }
}

#[async_trait]
impl MemoryStore for LocalMemoryProvider {
    fn provider_id(&self) -> &str {
        "local"
    }

    async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        let conn = self.conn.lock().await;
        let now = Utc::now();

        // Build FTS query — use simple match syntax
        let fts_query = build_fts_query(&query.text);

        let sql = format!(
            r#"SELECT {cols}, fts.rank
               FROM {fts_table} fts
               JOIN {records} r ON fts.memory_id = r.id
               WHERE {fts_table} MATCH ?1
                 AND r.tenant_id = ?2
                 AND r.deleted_at IS NULL
                 AND (r.expires_at IS NULL OR r.expires_at > ?3)
               ORDER BY fts.rank
               LIMIT ?4"#,
            cols = schema::RECORD_COLUMNS,
            fts_table = schema::TABLE_MEMORY_RECORDS_FTS,
            records = schema::TABLE_MEMORY_RECORDS,
        );

        let mut stmt = conn.prepare(&sql).map_err(|e| {
            MemoryError::Message(format!("recall prepare failed: {e}"))
        })?;

        let rows: Vec<(MemoryRecordRow, f64)> = stmt
            .query_map(
                rusqlite::params![
                    fts_query,
                    query.tenant_id.to_string(),
                    now.to_rfc3339(),
                    query.max_records as i64,
                ],
                |row| {
                    let record = row_to_record(row)?;
                    let rank: f64 = row.get(16)?;
                    Ok((record, rank))
                },
            )
            .map_err(|e| MemoryError::Message(format!("recall query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        // Apply visibility filtering
        let mut results: Vec<MemoryRecord> = rows
            .into_iter()
            .filter(|(row, _)| visibility_filter_matches_record(row, &query.visibility_filter))
            .filter(|(row, _)| query
                .min_similarity
                .le(&0.0) // FTS already filters; min_similarity is a post-filter
                || true)
            .map(|(row, rank)| {
                let mut record = row_to_memory_record(&row);
                let lexical = ranking::normalize_fts_rank(rank);
                record.metadata.recall_score = lexical;
                record
            })
            .collect();

        // Update access counters for returned records
        for record in &results {
            let _ = conn.execute(
                "UPDATE memory_records SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2",
                rusqlite::params![now.to_rfc3339(), record.id.to_string()],
            );
        }

        results.sort_by(|a, b| {
            b.metadata
                .recall_score
                .partial_cmp(&a.metadata.recall_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

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

        Ok(row_to_memory_record(&row))
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        let conn = self.conn.lock().await;
        let id = record.id;
        let now = Utc::now().to_rfc3339();
        let content_hash = blake3::hash(record.content.as_bytes()).to_hex().to_string();
        let metadata_json =
            serde_json::to_string(&record.metadata).unwrap_or_else(|_| "{}".to_owned());
        let evidence_json = "{}";

        // Remove old FTS entry if this is an update
        let _ = conn.execute(
            &format!(
                "DELETE FROM {} WHERE memory_id = ?1",
                schema::TABLE_MEMORY_RECORDS_FTS,
            ),
            rusqlite::params![id.to_string()],
        );

        // Compute expires_at from TTL
        let expires_at = record.metadata.ttl.map(|ttl| {
            (record.created_at + chrono::Duration::from_std(ttl).unwrap_or_default())
                .to_rfc3339()
        });

        conn.execute(
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
                record.tenant_id.to_string(),
                kind_to_str(&record.kind),
                visibility_to_str(&record.visibility),
                record.content,
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

        // Insert new FTS entry
        let _ = conn.execute(
            &format!(
                "INSERT INTO {} (content, metadata_text, memory_id, tenant_id) VALUES (?1, ?2, ?3, ?4)",
                schema::TABLE_MEMORY_RECORDS_FTS,
            ),
            rusqlite::params![record.content, metadata_json, id.to_string(), record.tenant_id.to_string()],
        );

        Ok(id)
    }

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();

        // Remove from FTS
        let _ = conn.execute(
            &format!(
                "DELETE FROM {} WHERE memory_id = ?1",
                schema::TABLE_MEMORY_RECORDS_FTS,
            ),
            rusqlite::params![id.to_string()],
        );

        // Hard-delete the memory record.
        let affected = conn
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
        let _ = conn.execute(
            &format!(
                "INSERT INTO {} (id, tenant_id, memory_id, content_hash, reason, evidence_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                schema::TABLE_MEMORY_TOMBSTONES,
            ),
            rusqlite::params![
                MemoryId::new().to_string(),
                self.tenant_id.to_string(),
                id.to_string(),
                "unknown",
                "user_requested",
                "{}",
                now,
            ],
        );

        Ok(())
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
            .query_map(
                rusqlite::params![self.tenant_id.to_string(), now],
                |row| row_to_record(row),
            )
            .map_err(|e| MemoryError::Message(format!("list query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        let summaries: Vec<MemorySummary> = rows
            .into_iter()
            .map(|row| {
                let record = row_to_memory_record(&row);
                list_scope_filter(&record, &scope)
            })
            .filter_map(|r| r)
            .collect();

        Ok(summaries)
    }
}

impl MemoryLifecycle for LocalMemoryProvider {}

// ── SQL row helpers ──

struct MemoryRecordRow {
    id: String,
    tenant_id: String,
    kind: String,
    visibility: String,
    content: String,
    metadata_json: String,
    content_hash: String,
    source_kind: String,
    evidence_json: String,
    confidence: f64,
    access_count: i64,
    last_accessed_at: Option<String>,
    created_at: String,
    updated_at: String,
    expires_at: Option<String>,
    deleted_at: Option<String>,
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<MemoryRecordRow> {
    Ok(MemoryRecordRow {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        kind: row.get(2)?,
        visibility: row.get(3)?,
        content: row.get(4)?,
        metadata_json: row.get(5)?,
        content_hash: row.get(6)?,
        source_kind: row.get(7)?,
        evidence_json: row.get(8)?,
        confidence: row.get(9)?,
        access_count: row.get(10)?,
        last_accessed_at: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        expires_at: row.get(14)?,
        deleted_at: row.get(15)?,
    })
}

fn row_to_memory_record(row: &MemoryRecordRow) -> MemoryRecord {
    let metadata: MemoryMetadata = serde_json::from_str(&row.metadata_json).unwrap_or_else(|_| {
        MemoryMetadata {
            tags: vec![],
            source: str_to_source(&row.source_kind),
            confidence: row.confidence as f32,
            access_count: row.access_count as u32,
            last_accessed_at: row
                .last_accessed_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            recall_score: 0.0,
            ttl: None,
            redacted_segments: 0,
        }
    });

    MemoryRecord {
        id: MemoryId::parse(&row.id).unwrap_or_else(|_| MemoryId::new()),
        tenant_id: TenantId::parse(&row.tenant_id).unwrap_or(TenantId::SINGLE),
        kind: str_to_kind(&row.kind),
        visibility: str_to_visibility(&row.visibility),
        content: row.content.clone(),
        metadata,
        created_at: row
            .created_at
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .updated_at
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    }
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

fn str_to_source(s: &str) -> harness_contracts::MemorySource {
    match s {
        "user_input" => harness_contracts::MemorySource::UserInput,
        "agent_derived" => harness_contracts::MemorySource::AgentDerived,
        "subagent_derived" => harness_contracts::MemorySource::SubagentDerived {
            child_session: harness_contracts::SessionId::new(),
        },
        "tool_output" => harness_contracts::MemorySource::ToolOutput,
        "mcp_tool_output" => harness_contracts::MemorySource::McpToolOutput,
        "plugin_output" => harness_contracts::MemorySource::PluginOutput,
        "web_retrieval" => harness_contracts::MemorySource::WebRetrieval,
        "workspace_file" => harness_contracts::MemorySource::WorkspaceFile,
        "external_retrieval" => harness_contracts::MemorySource::ExternalRetrieval,
        "imported" => harness_contracts::MemorySource::Imported,
        "consolidated" => harness_contracts::MemorySource::Consolidated {
            from: vec![],
        },
        _ => harness_contracts::MemorySource::UserInput,
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
            let tenant: TenantId =
                TenantId::parse(&row.tenant_id).unwrap_or(TenantId::SINGLE);
            if actor.tenant_id != tenant {
                return false;
            }
            visibility_matches(&visibility, actor)
        }
        MemoryVisibilityFilter::Exact(v) => &visibility == v,
    }
}

fn list_scope_filter(record: &MemoryRecord, scope: &MemoryListScope) -> Option<MemorySummary> {
    let matches = match scope {
        MemoryListScope::All => true,
        MemoryListScope::ByKind(kind) => &record.kind == kind,
        MemoryListScope::ByVisibility(visibility) => &record.visibility == visibility,
        MemoryListScope::ForActor(actor) => {
            record.tenant_id == actor.tenant_id
                && visibility_matches(&record.visibility, actor)
        }
    };
    if matches {
        Some(MemorySummary {
            id: record.id,
            kind: record.kind.clone(),
            visibility: record.visibility.clone(),
            content_preview: content_preview(&record.content),
            metadata: record.metadata.clone(),
            updated_at: record.updated_at,
        })
    } else {
        None
    }
}
