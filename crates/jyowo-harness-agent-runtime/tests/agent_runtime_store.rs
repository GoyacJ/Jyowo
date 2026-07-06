use harness_agent_runtime::{
    AgentRuntimeStore, BackgroundAgentStoreRecord, AGENT_RUNTIME_DB_FILENAME,
    CURRENT_SCHEMA_VERSION,
};
use harness_contracts::BackgroundAgentState;
use rusqlite::Connection;
use std::path::PathBuf;
use tempfile::{tempdir, TempDir};

#[test]
fn store_open_creates_runtime_directory_and_sqlite_file() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");

    assert!(store.runtime_dir().exists());
    assert!(store.db_path().is_file());
    assert_eq!(
        store.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
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
    let store = AgentRuntimeStore::open(&workspace_root)
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

    let error = match AgentRuntimeStore::open(&workspace_root) {
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
    let first = AgentRuntimeStore::open(&workspace_root).expect("first open");
    assert_eq!(
        first.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );

    let second = AgentRuntimeStore::open(&workspace_root).expect("second open");
    assert_eq!(
        second.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn store_open_enables_wal_journal_mode() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let journal_mode: String = store
        .with_connection(|connection| {
            connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        })
        .expect("journal mode");

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
}

#[test]
fn migration_creates_required_tables() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");

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
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    store
        .with_connection(|connection| {
            connection.execute("UPDATE schema_version SET version = 999", [])?;
            Ok(())
        })
        .expect("force unsupported schema");

    assert!(matches!(
        AgentRuntimeStore::open(&workspace_root),
        Err(harness_agent_runtime::AgentRuntimeStoreError::UnsupportedSchema(_))
    ));
}

#[test]
fn current_schema_version_constant_matches_migration() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");

    assert_eq!(
        store.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn migration_v2_adds_background_attempt_prior_attempt_link() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_dir = workspace_root.join(".jyowo/runtime");
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

    let store = AgentRuntimeStore::open(&workspace_root).expect("store migrates");
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
    let workspace_root = canonical_temp_root(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
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

    let reopened = AgentRuntimeStore::open(&workspace_root).expect("store reopens");
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
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
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
    assert_eq!(
        store.schema_version().expect("schema version"),
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn open_runtime_dir_and_open_produce_same_schema_for_workspace() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let runtime_root = workspace_root.join(".jyowo/runtime");

    let store_via_open = AgentRuntimeStore::open(&workspace_root).expect("open succeeds");
    let store_via_dir =
        AgentRuntimeStore::open_runtime_dir(&runtime_root).expect("open_runtime_dir succeeds");

    assert_eq!(store_via_open.runtime_dir(), store_via_dir.runtime_dir());
    assert_eq!(
        store_via_open.schema_version().expect("schema version"),
        store_via_dir.schema_version().expect("schema version")
    );
}

fn canonical_temp_root(temp: &TempDir) -> PathBuf {
    temp.path().canonicalize().expect("canonical tempdir")
}
