-- V4: Redacted model request previews for memory context inspection.

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
