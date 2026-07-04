-- V1: Initial memory platform schema.
-- Creates memory_records, memory_embeddings, memory_tombstones, and FTS5 tables.

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

-- FTS5 virtual table for full-text search
CREATE VIRTUAL TABLE memory_records_fts USING fts5(
    content,
    metadata_text,
    memory_id UNINDEXED,
    tenant_id UNINDEXED,
    tokenize='unicode61 remove_diacritics 2'
);

-- FTS sync is managed by application code (LocalMemoryProvider),
-- not by SQL triggers. This avoids FTS5 content-table delete complexities.

-- Embedding table
CREATE TABLE memory_embeddings (
    memory_id TEXT PRIMARY KEY REFERENCES memory_records(id) ON DELETE CASCADE,
    embedding_state TEXT NOT NULL CHECK (embedding_state IN ('missing', 'ready', 'failed', 'disabled')),
    dimension INTEGER,
    vector_le_f32 BLOB,
    model_id TEXT,
    updated_at TEXT NOT NULL,
    error_kind TEXT
);

-- Tombstone table
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
