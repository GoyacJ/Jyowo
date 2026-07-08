use harness_agent_runtime::{
    AgentRuntimeStore, BackgroundAgentAttemptRecord, BackgroundAgentStoreRecord,
    AGENT_RUNTIME_DB_FILENAME,
};
use harness_contracts::BackgroundAgentState;
use rusqlite::Connection;
use std::path::PathBuf;
use tempfile::{tempdir, TempDir};

#[test]
fn store_open_creates_runtime_directory_and_sqlite_file() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store opens");

    assert!(store.runtime_dir().exists());
    assert!(store.db_path().is_file());
    assert!(store
        .table_exists("background_agent_registry")
        .expect("table lookup"));
}

#[cfg(unix)]
#[test]
fn store_open_resolves_symlink_runtime_parent_via_canonical_path() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let external = tempdir().expect("external tempdir");
    std::os::unix::fs::symlink(external.path(), workspace_root.join(".jyowo"))
        .expect("symlink .jyowo");

    // The store should resolve the symlink via canonical prefix and operate
    // at the canonical (real) location — not follow the symlink blindly.
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store open should resolve symlink prefix via canonical path");

    // Database and profiles should be created at the canonical target.
    let canonical_runtime = external.path().canonicalize().unwrap().join("runtime");
    assert!(canonical_runtime.join("agent-runtime.sqlite").exists());
    assert_eq!(
        store.runtime_dir().canonicalize().unwrap(),
        canonical_runtime
    );
}

#[cfg(unix)]
#[test]
fn store_open_rejects_symlink_sqlite_file_without_opening_target() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let external = tempfile::NamedTempFile::new().expect("external sqlite target");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    std::os::unix::fs::symlink(external.path(), &db_path).expect("sqlite symlink");

    let error =
        match AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime")) {
            Ok(_) => panic!("store open should reject sqlite symlink"),
            Err(error) => error,
        };

    assert!(error.to_string().contains("symlink"));
    assert_eq!(
        std::fs::metadata(external.path())
            .expect("external metadata")
            .len(),
        0
    );
    assert!(std::fs::symlink_metadata(&db_path)
        .expect("link metadata")
        .file_type()
        .is_symlink());
}

#[test]
fn store_reopen_is_idempotent() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let first = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("first open");
    assert!(first
        .table_exists("background_agent_registry")
        .expect("table lookup"));

    let second = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("second open");
    assert!(second
        .table_exists("background_agent_registry")
        .expect("table lookup"));
}

#[test]
fn store_open_enables_wal_journal_mode() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store opens");
    let journal_mode: String = store
        .with_connection(|connection| {
            connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        })
        .expect("journal mode");

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
}

#[test]
fn schema_initialization_creates_required_tables() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store opens");

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
fn old_marker_table_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch(&format!(
            "CREATE TABLE {} (version INTEGER NOT NULL);",
            old_marker_table()
        ))
        .expect("seed old marker");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime")),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn extra_current_table_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo").join("runtime");
    let store = AgentRuntimeStore::open_runtime_dir(&runtime_dir).expect("store opens");
    let db_path = store.db_path().to_path_buf();
    drop(store);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch("CREATE TABLE extra_runtime_table (id TEXT PRIMARY KEY NOT NULL);")
        .expect("seed extra table");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(runtime_dir),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn extra_current_column_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo").join("runtime");
    let store = AgentRuntimeStore::open_runtime_dir(&runtime_dir).expect("store opens");
    let db_path = store.db_path().to_path_buf();
    drop(store);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch("ALTER TABLE agent_profile_cache ADD COLUMN extra TEXT;")
        .expect("seed extra column");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(runtime_dir),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn missing_current_table_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch("CREATE TABLE agent_profile_cache (profile_id TEXT PRIMARY KEY NOT NULL);")
        .expect("seed partial store");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime")),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn missing_current_column_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch(
            "
            CREATE TABLE agent_profile_cache (
                profile_id TEXT PRIMARY KEY NOT NULL,
                scope TEXT NOT NULL,
                role TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE background_agent_registry (
                background_agent_id TEXT PRIMARY KEY NOT NULL
            );
            CREATE TABLE background_agent_attempts (
                attempt_id TEXT PRIMARY KEY NOT NULL,
                background_agent_id TEXT NOT NULL,
                prior_attempt_id TEXT,
                attempt_number INTEGER NOT NULL,
                state TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE agent_team_tasks (task_id TEXT PRIMARY KEY NOT NULL);
            CREATE TABLE agent_team_mailbox (message_id TEXT PRIMARY KEY NOT NULL);
            CREATE TABLE workspace_isolation_leases (lease_id TEXT PRIMARY KEY NOT NULL);
            CREATE TABLE restart_recovery_markers (marker_id TEXT PRIMARY KEY NOT NULL);
            ",
        )
        .expect("seed partial store");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime")),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn missing_agent_team_task_column_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch(
            "
            CREATE TABLE agent_profile_cache (
                profile_id TEXT PRIMARY KEY NOT NULL,
                scope TEXT NOT NULL,
                role TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE background_agent_registry (
                background_agent_id TEXT PRIMARY KEY NOT NULL,
                conversation_id TEXT NOT NULL,
                run_id TEXT,
                state TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE background_agent_attempts (
                attempt_id TEXT PRIMARY KEY NOT NULL,
                background_agent_id TEXT NOT NULL,
                prior_attempt_id TEXT,
                attempt_number INTEGER NOT NULL,
                state TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE agent_team_tasks (
                task_id TEXT PRIMARY KEY NOT NULL,
                team_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                title TEXT NOT NULL
            );
            CREATE TABLE agent_team_mailbox (message_id TEXT PRIMARY KEY NOT NULL);
            CREATE TABLE workspace_isolation_leases (lease_id TEXT PRIMARY KEY NOT NULL);
            CREATE TABLE restart_recovery_markers (marker_id TEXT PRIMARY KEY NOT NULL);
            ",
        )
        .expect("seed partial store");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime")),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn partial_store_shape_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch(
            "
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
        .expect("seed partial schema");
    drop(connection);

    assert!(matches!(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime")),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn workspace_isolation_lease_survives_store_reopen() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store opens");
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

    let reopened =
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store reopens");
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
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store opens");
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

#[test]
fn delete_background_agents_for_conversation_removes_registry_and_attempts_only_for_session() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("store opens");

    for (background_agent_id, conversation_id) in [
        ("background-delete", "conversation-delete"),
        ("background-keep", "conversation-keep"),
    ] {
        store
            .insert_background_agent(&BackgroundAgentStoreRecord {
                background_agent_id: background_agent_id.to_owned(),
                conversation_id: conversation_id.to_owned(),
                run_id: Some(format!("{background_agent_id}-run")),
                state: BackgroundAgentState::Running,
                title: background_agent_id.to_owned(),
                created_at: "2026-06-30T00:00:00Z".to_owned(),
                updated_at: "2026-06-30T00:00:00Z".to_owned(),
                payload_json: "{}".to_owned(),
            })
            .expect("background insert");
        store
            .insert_background_agent_attempt(&BackgroundAgentAttemptRecord {
                attempt_id: format!("{background_agent_id}-attempt"),
                background_agent_id: background_agent_id.to_owned(),
                prior_attempt_id: None,
                attempt_number: 1,
                state: BackgroundAgentState::Running,
                started_at: "2026-06-30T00:00:00Z".to_owned(),
                ended_at: None,
                payload_json: "{}".to_owned(),
            })
            .expect("attempt insert");
    }

    store
        .delete_background_agents_for_conversation("conversation-delete")
        .expect("delete conversation background agents");

    assert!(store
        .get_background_agent("background-delete")
        .expect("deleted background lookup")
        .is_none());
    assert!(store
        .list_background_agent_attempts("background-delete")
        .expect("deleted attempts lookup")
        .is_empty());
    assert!(store
        .get_background_agent("background-keep")
        .expect("kept background lookup")
        .is_some());
    assert_eq!(
        store
            .list_background_agent_attempts("background-keep")
            .expect("kept attempts lookup")
            .len(),
        1
    );
}

#[test]
fn open_runtime_dir_uses_explicit_runtime_root() {
    let temp = tempdir().expect("tempdir");
    let temp_root = canonical_temp_root(&temp);
    // Use a path that is NOT <workspace>/.jyowo/runtime
    let runtime_root = temp_root.join("custom-runtime-dir");

    let store =
        AgentRuntimeStore::open_runtime_dir(&runtime_root).expect("open_runtime_dir succeeds");

    assert!(store.runtime_dir().exists());
    assert_eq!(store.runtime_dir(), &runtime_root);
    assert!(store.db_path().starts_with(&runtime_root));
    assert!(store
        .table_exists("background_agent_registry")
        .expect("table lookup"));
}

#[test]
fn open_runtime_dir_reopen_produces_same_schema_for_workspace() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_root = workspace_root.join(".jyowo/runtime");

    let first = AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
        .expect("open succeeds");
    let second = AgentRuntimeStore::open_runtime_dir(&runtime_root).expect("reopen succeeds");

    assert_eq!(first.runtime_dir(), second.runtime_dir());
    assert_eq!(first.db_path(), second.db_path());
}

fn old_marker_table() -> String {
    ["schema", "version"].join("_")
}

fn canonical_temp_root(temp: &TempDir) -> PathBuf {
    temp.path().canonicalize().expect("canonical tempdir")
}
