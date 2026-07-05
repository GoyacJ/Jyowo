-- V2: Durable memory platform stores.
-- Stores contract payloads as JSON while indexing tenant, session, state, and time.

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
