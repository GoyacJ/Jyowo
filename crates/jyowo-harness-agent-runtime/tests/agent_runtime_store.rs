use harness_agent_runtime::{
    AgentRuntimeStore, BackgroundAgentStoreRecord, AGENT_RUNTIME_DB_FILENAME,
    CURRENT_SCHEMA_VERSION,
};
use harness_contracts::BackgroundAgentState;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn store_open_creates_runtime_directory_and_sqlite_file() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");

    assert!(store.runtime_dir().exists());
    assert!(store.db_path().is_file());
    assert_eq!(
        store.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn store_reopen_is_idempotent() {
    let workspace = tempdir().expect("tempdir");
    let first = AgentRuntimeStore::open(workspace.path()).expect("first open");
    assert_eq!(
        first.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );

    let second = AgentRuntimeStore::open(workspace.path()).expect("second open");
    assert_eq!(
        second.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn migration_creates_required_tables() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");

    for table in [
        "agent_profile_cache",
        "background_agent_registry",
        "background_agent_attempts",
        "agent_team_tasks",
        "agent_team_mailbox",
        "workspace_isolation_leases",
        "restart_recovery_markers",
    ] {
        assert!(
            store.table_exists(table).expect("table lookup"),
            "missing table {table}"
        );
    }
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    store
        .with_connection(|connection| {
            connection.execute("UPDATE schema_version SET version = 999", [])?;
            Ok(())
        })
        .expect("force unsupported schema");

    assert!(matches!(
        AgentRuntimeStore::open(workspace.path()),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn current_schema_version_constant_matches_migration() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");

    assert_eq!(
        store.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn migration_v2_adds_background_attempt_prior_attempt_link() {
    let workspace = tempdir().expect("tempdir");
    let runtime_dir = workspace.path().join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch(
            "
            CREATE TABLE schema_version (
                version INTEGER NOT NULL
            );
            INSERT INTO schema_version(version) VALUES (1);

            CREATE TABLE background_agent_attempts (
                attempt_id TEXT PRIMARY KEY NOT NULL,
                background_agent_id TEXT NOT NULL,
                attempt_number INTEGER NOT NULL,
                state TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                payload_json TEXT NOT NULL
            );
            ",
        )
        .expect("seed v1 schema");
    drop(connection);

    let store = AgentRuntimeStore::open(workspace.path()).expect("store migrates");
    let has_prior_attempt_id = store
        .with_connection(|connection| {
            let mut statement =
                connection.prepare("PRAGMA table_info(background_agent_attempts)")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name == "prior_attempt_id" {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .expect("read columns");

    assert_eq!(
        store.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
    assert!(has_prior_attempt_id);
}

#[test]
fn workspace_isolation_lease_survives_store_reopen() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let lease = harness_agent_runtime::WorkspaceIsolationLease {
        lease_id: "lease-1".to_owned(),
        conversation_id: "conversation-1".to_owned(),
        run_id: "run-1".to_owned(),
        agent_id: "agent-1".to_owned(),
        path: workspace
            .path()
            .join(".jyowo/runtime/agent-worktrees/lease-1")
            .to_string_lossy()
            .into_owned(),
        branch: Some("jyowo/agent-agent-1".to_owned()),
        base_commit: Some("abc123".to_owned()),
        status: "active".to_owned(),
        created_at: "2026-06-30T00:00:00Z".to_owned(),
        updated_at: "2026-06-30T00:00:00Z".to_owned(),
    };
    store
        .insert_workspace_isolation_lease(&lease)
        .expect("lease insert");

    let reopened = AgentRuntimeStore::open(workspace.path()).expect("store reopens");
    let loaded = reopened
        .get_workspace_isolation_lease("lease-1")
        .expect("lease lookup")
        .expect("lease exists");
    assert_eq!(loaded, lease);
    assert!(reopened
        .table_exists("workspace_isolation_leases")
        .expect("table exists"));
}

#[test]
fn background_agent_payload_claim_is_atomic_by_prior_payload() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let original_payload = r#"{"supervisorExecution":{"status":"queued"}}"#;
    let running_payload = r#"{"supervisorExecution":{"status":"running"}}"#;
    let duplicate_payload = r#"{"supervisorExecution":{"status":"duplicate"}}"#;
    store
        .insert_background_agent(&BackgroundAgentStoreRecord {
            background_agent_id: "background-1".to_owned(),
            conversation_id: "conversation-1".to_owned(),
            run_id: Some("attempt-1".to_owned()),
            state: BackgroundAgentState::Running,
            title: "queued background".to_owned(),
            created_at: "2026-06-30T00:00:00Z".to_owned(),
            updated_at: "2026-06-30T00:00:00Z".to_owned(),
            payload_json: original_payload.to_owned(),
        })
        .expect("background insert");

    assert!(store
        .claim_background_agent_payload_json(
            "background-1",
            original_payload,
            running_payload,
            "2026-06-30T00:00:01Z",
        )
        .expect("first claim"));
    assert!(!store
        .claim_background_agent_payload_json(
            "background-1",
            original_payload,
            duplicate_payload,
            "2026-06-30T00:00:02Z",
        )
        .expect("second claim"));

    let loaded = store
        .get_background_agent("background-1")
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(loaded.payload_json, running_payload);
    assert_eq!(loaded.updated_at, "2026-06-30T00:00:01Z");
}
