//! SQLite schema constants for the local memory provider.
//!
//! These define table names, column lists, and the DDL that must match
//! the refinery migrations in `migrations.rs`.

/// `memory_records` — durable memory record storage.
pub const TABLE_MEMORY_RECORDS: &str = "memory_records";

/// `memory_embeddings` — per-record embedding vectors.
pub const TABLE_MEMORY_EMBEDDINGS: &str = "memory_embeddings";

/// `memory_tombstones` — deletion barriers.
pub const TABLE_MEMORY_TOMBSTONES: &str = "memory_tombstones";

/// `memory_records_fts` — FTS5 virtual table.
pub const TABLE_MEMORY_RECORDS_FTS: &str = "memory_records_fts";

/// `schema_version` — refinery migration tracking.
pub const TABLE_SCHEMA_VERSION: &str = "schema_version";

/// All record columns (for SELECT), qualified with table alias `r`.
pub const RECORD_COLUMNS: &str = "\
    r.id, r.tenant_id, r.kind, r.visibility, r.content, r.metadata_json, r.content_hash, \
    r.source_kind, r.evidence_json, r.confidence, r.access_count, r.last_accessed_at, \
    r.created_at, r.updated_at, r.expires_at, r.deleted_at";

/// Columns without table qualifier (for single-table queries).
pub const RECORD_COLUMNS_BARE: &str = "\
    id, tenant_id, kind, visibility, content, metadata_json, content_hash, \
    source_kind, evidence_json, confidence, access_count, last_accessed_at, \
    created_at, updated_at, expires_at, deleted_at";

/// Embedding state enum values.
pub const EMBEDDING_STATE_MISSING: &str = "missing";
pub const EMBEDDING_STATE_READY: &str = "ready";
pub const EMBEDDING_STATE_FAILED: &str = "failed";
pub const EMBEDDING_STATE_DISABLED: &str = "disabled";

/// PRAGMA statements applied on every connection open.
pub const CONNECTION_PRAGMAS: &[&str] = &["PRAGMA busy_timeout = 5000", "PRAGMA foreign_keys = ON"];
