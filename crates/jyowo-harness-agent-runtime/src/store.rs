use std::path::{Path, PathBuf};
use std::sync::Mutex;

use harness_contracts::BackgroundAgentState;
use rusqlite::Connection;
use thiserror::Error;

use crate::migrations;

pub const AGENT_RUNTIME_DB_FILENAME: &str = "agent-runtime.sqlite";
const RUNTIME_DIR: &str = ".jyowo/runtime";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceIsolationLease {
    pub lease_id: String,
    pub conversation_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub path: String,
    pub branch: Option<String>,
    pub base_commit: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTeamTaskRecord {
    pub task_id: String,
    pub team_id: String,
    pub run_id: String,
    pub title: String,
    pub status: String,
    pub assignee_profile_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTeamMailboxRecord {
    pub message_id: String,
    pub team_id: String,
    pub sender_profile_id: String,
    pub recipient_profile_id: Option<String>,
    pub created_at: String,
    pub summary: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentStoreRecord {
    pub background_agent_id: String,
    pub conversation_id: String,
    pub run_id: Option<String>,
    pub state: BackgroundAgentState,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentAttemptRecord {
    pub attempt_id: String,
    pub background_agent_id: String,
    pub prior_attempt_id: Option<String>,
    pub attempt_number: u32,
    pub state: BackgroundAgentState,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub payload_json: String,
}

#[derive(Debug, Error)]
pub enum AgentRuntimeStoreError {
    #[error("agent runtime store io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("agent runtime store sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("agent runtime store lock poisoned")]
    LockPoisoned,
    #[error("unsupported agent runtime schema: {0}")]
    UnsupportedSchema(String),
}

pub struct AgentRuntimeStore {
    runtime_dir: PathBuf,
    db_path: PathBuf,
    connection: Mutex<Connection>,
}

impl AgentRuntimeStore {
    /// Open the agent runtime store at `runtime_root`, using the standard
    /// `agent-runtime.sqlite` and `agent-profiles.json` under that directory.
    ///
    /// Prefer this over `open` when you already have a resolved runtime root
    /// (e.g. from `RuntimeLayout`).
    pub fn open_runtime_dir(
        runtime_root: impl AsRef<Path>,
    ) -> Result<Self, AgentRuntimeStoreError> {
        let runtime_dir = runtime_root.as_ref().to_path_buf();
        // Resolve any benign OS-level symlinks (e.g. /tmp on macOS) before
        // running the strict no-symlink directory check.
        let runtime_dir =
            harness_fs::resolve_canonical_prefix(&runtime_dir).map_err(store_fs_error)?;
        ensure_app_dir_no_symlink(&runtime_dir)?;
        let db_path = runtime_dir.join(AGENT_RUNTIME_DB_FILENAME);
        ensure_sqlite_file_no_symlink(&db_path)?;
        let connection = Connection::open(&db_path)?;
        enable_wal_journal_mode(&connection)?;
        migrations::migrate(&connection).map_err(|error| match error {
            rusqlite::Error::InvalidParameterName(message) => {
                AgentRuntimeStoreError::UnsupportedSchema(message)
            }
            other => AgentRuntimeStoreError::Sqlite(other),
        })?;
        Ok(Self {
            runtime_dir,
            db_path,
            connection: Mutex::new(connection),
        })
    }

    /// Open the agent runtime store from a workspace root, appending
    /// `.jyowo/runtime` internally.
    ///
    /// This is a compatibility wrapper. Prefer `open_runtime_dir` when a
    /// resolved runtime root is available from `RuntimeLayout`.
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, AgentRuntimeStoreError> {
        let runtime_dir = workspace_root.as_ref().join(RUNTIME_DIR);
        Self::open_runtime_dir(runtime_dir)
    }

    #[must_use]
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    #[must_use]
    pub fn profiles_file_path(&self) -> PathBuf {
        self.runtime_dir.join("agent-profiles.json")
    }

    pub fn with_connection<R>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<R, rusqlite::Error>,
    ) -> Result<R, AgentRuntimeStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| AgentRuntimeStoreError::LockPoisoned)?;
        operation(&connection).map_err(AgentRuntimeStoreError::from)
    }

    pub fn schema_version(&self) -> Result<i64, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
        })
    }

    pub fn table_exists(&self, table: &str) -> Result<bool, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    pub fn insert_workspace_isolation_lease(
        &self,
        lease: &WorkspaceIsolationLease,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO workspace_isolation_leases (
                    lease_id,
                    conversation_id,
                    run_id,
                    agent_id,
                    path,
                    branch,
                    base_commit,
                    status,
                    created_at,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    lease.lease_id,
                    lease.conversation_id,
                    lease.run_id,
                    lease.agent_id,
                    lease.path,
                    lease.branch,
                    lease.base_commit,
                    lease.status,
                    lease.created_at,
                    lease.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_workspace_isolation_lease(
        &self,
        lease_id: &str,
    ) -> Result<Option<WorkspaceIsolationLease>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT lease_id, conversation_id, run_id, agent_id, path, branch, base_commit, status, created_at, updated_at
                 FROM workspace_isolation_leases
                 WHERE lease_id = ?1",
            )?;
            let mut rows = statement.query([lease_id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(read_workspace_isolation_lease_row(row)?))
            } else {
                Ok(None)
            }
        })
    }

    pub fn list_workspace_isolation_leases(
        &self,
    ) -> Result<Vec<WorkspaceIsolationLease>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT lease_id, conversation_id, run_id, agent_id, path, branch, base_commit, status, created_at, updated_at
                 FROM workspace_isolation_leases
                 ORDER BY created_at ASC",
            )?;
            let mut rows = statement.query([])?;
            let mut leases = Vec::new();
            while let Some(row) = rows.next()? {
                leases.push(read_workspace_isolation_lease_row(&row)?);
            }
            Ok(leases)
        })
    }

    pub fn list_active_workspace_isolation_leases(
        &self,
    ) -> Result<Vec<WorkspaceIsolationLease>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT lease_id, conversation_id, run_id, agent_id, path, branch, base_commit, status, created_at, updated_at
                 FROM workspace_isolation_leases
                 WHERE status = 'active'
                 ORDER BY created_at ASC",
            )?;
            let mut rows = statement.query([])?;
            let mut leases = Vec::new();
            while let Some(row) = rows.next()? {
                leases.push(read_workspace_isolation_lease_row(&row)?);
            }
            Ok(leases)
        })
    }

    pub fn insert_background_agent(
        &self,
        record: &BackgroundAgentStoreRecord,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO background_agent_registry (
                    background_agent_id,
                    conversation_id,
                    run_id,
                    state,
                    title,
                    created_at,
                    updated_at,
                    payload_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    record.background_agent_id,
                    record.conversation_id,
                    record.run_id,
                    background_state_wire(record.state),
                    record.title,
                    record.created_at,
                    record.updated_at,
                    record.payload_json,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_background_agent(
        &self,
        background_agent_id: &str,
    ) -> Result<Option<BackgroundAgentStoreRecord>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT background_agent_id, conversation_id, run_id, state, title, created_at, updated_at, payload_json
                 FROM background_agent_registry
                 WHERE background_agent_id = ?1",
            )?;
            let mut rows = statement.query([background_agent_id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(read_background_agent_row(row)?))
            } else {
                Ok(None)
            }
        })
    }

    pub fn list_background_agents(
        &self,
        include_archived: bool,
    ) -> Result<Vec<BackgroundAgentStoreRecord>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let sql = if include_archived {
                "SELECT background_agent_id, conversation_id, run_id, state, title, created_at, updated_at, payload_json
                 FROM background_agent_registry
                 ORDER BY created_at ASC"
            } else {
                "SELECT background_agent_id, conversation_id, run_id, state, title, created_at, updated_at, payload_json
                 FROM background_agent_registry
                 WHERE state != 'archived'
                 ORDER BY created_at ASC"
            };
            let mut statement = connection.prepare(sql)?;
            let mut rows = statement.query([])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push(read_background_agent_row(&row)?);
            }
            Ok(records)
        })
    }

    pub fn update_background_agent_state(
        &self,
        background_agent_id: &str,
        state: BackgroundAgentState,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE background_agent_registry
                 SET state = ?2, updated_at = ?3
                 WHERE background_agent_id = ?1",
                rusqlite::params![
                    background_agent_id,
                    background_state_wire(state),
                    updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_background_agent_state_and_payload_json(
        &self,
        background_agent_id: &str,
        state: BackgroundAgentState,
        payload_json: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE background_agent_registry
                 SET state = ?2, payload_json = ?3, updated_at = ?4
                 WHERE background_agent_id = ?1",
                rusqlite::params![
                    background_agent_id,
                    background_state_wire(state),
                    payload_json,
                    updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_background_agent_state_and_run_id(
        &self,
        background_agent_id: &str,
        state: BackgroundAgentState,
        run_id: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE background_agent_registry
                 SET state = ?2, run_id = ?3, updated_at = ?4
                 WHERE background_agent_id = ?1",
                rusqlite::params![
                    background_agent_id,
                    background_state_wire(state),
                    run_id,
                    updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_background_agent_state_payload_json_and_run_id(
        &self,
        background_agent_id: &str,
        state: BackgroundAgentState,
        payload_json: &str,
        run_id: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE background_agent_registry
                 SET state = ?2, payload_json = ?3, run_id = ?4, updated_at = ?5
                 WHERE background_agent_id = ?1",
                rusqlite::params![
                    background_agent_id,
                    background_state_wire(state),
                    payload_json,
                    run_id,
                    updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_background_agent_payload_json(
        &self,
        background_agent_id: &str,
        payload_json: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE background_agent_registry
                 SET payload_json = ?2, updated_at = ?3
                 WHERE background_agent_id = ?1",
                rusqlite::params![background_agent_id, payload_json, updated_at],
            )?;
            Ok(())
        })
    }

    pub fn claim_background_agent_payload_json(
        &self,
        background_agent_id: &str,
        expected_payload_json: &str,
        payload_json: &str,
        updated_at: &str,
    ) -> Result<bool, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "UPDATE background_agent_registry
                 SET payload_json = ?3, updated_at = ?4
                 WHERE background_agent_id = ?1
                   AND payload_json = ?2
                   AND state IN ('queued', 'running')",
                rusqlite::params![
                    background_agent_id,
                    expected_payload_json,
                    payload_json,
                    updated_at
                ],
            )?;
            Ok(changed == 1)
        })
    }

    pub fn update_background_agent_run_id(
        &self,
        background_agent_id: &str,
        run_id: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE background_agent_registry
                 SET run_id = ?2, updated_at = ?3
                 WHERE background_agent_id = ?1",
                rusqlite::params![background_agent_id, run_id, updated_at],
            )?;
            Ok(())
        })
    }

    pub fn delete_background_agent(
        &self,
        background_agent_id: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM background_agent_attempts WHERE background_agent_id = ?1",
                [background_agent_id],
            )?;
            connection.execute(
                "DELETE FROM background_agent_registry WHERE background_agent_id = ?1",
                [background_agent_id],
            )?;
            Ok(())
        })
    }

    pub fn delete_background_agents_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM background_agent_attempts
                 WHERE background_agent_id IN (
                   SELECT background_agent_id
                   FROM background_agent_registry
                   WHERE conversation_id = ?1
                 )",
                [conversation_id],
            )?;
            connection.execute(
                "DELETE FROM background_agent_registry WHERE conversation_id = ?1",
                [conversation_id],
            )?;
            Ok(())
        })
    }

    pub fn insert_background_agent_attempt(
        &self,
        attempt: &BackgroundAgentAttemptRecord,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO background_agent_attempts (
                    attempt_id,
                    background_agent_id,
                    prior_attempt_id,
                    attempt_number,
                    state,
                    started_at,
                    ended_at,
                    payload_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    attempt.attempt_id,
                    attempt.background_agent_id,
                    attempt.prior_attempt_id,
                    attempt.attempt_number,
                    background_state_wire(attempt.state),
                    attempt.started_at,
                    attempt.ended_at,
                    attempt.payload_json,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_background_agent_attempts(
        &self,
        background_agent_id: &str,
    ) -> Result<Vec<BackgroundAgentAttemptRecord>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT attempt_id, background_agent_id, prior_attempt_id, attempt_number, state, started_at, ended_at, payload_json
                 FROM background_agent_attempts
                 WHERE background_agent_id = ?1
                 ORDER BY attempt_number ASC",
            )?;
            let mut rows = statement.query([background_agent_id])?;
            let mut attempts = Vec::new();
            while let Some(row) = rows.next()? {
                attempts.push(BackgroundAgentAttemptRecord {
                    attempt_id: row.get(0)?,
                    background_agent_id: row.get(1)?,
                    prior_attempt_id: row.get(2)?,
                    attempt_number: row.get::<_, i64>(3)? as u32,
                    state: parse_background_state(row.get::<_, String>(4)?.as_str())?,
                    started_at: row.get(5)?,
                    ended_at: row.get(6)?,
                    payload_json: row.get(7)?,
                });
            }
            Ok(attempts)
        })
    }

    pub fn find_active_workspace_isolation_lease_by_branch(
        &self,
        branch: &str,
    ) -> Result<Option<WorkspaceIsolationLease>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT lease_id, conversation_id, run_id, agent_id, path, branch, base_commit, status, created_at, updated_at
                 FROM workspace_isolation_leases
                 WHERE branch = ?1 AND status = 'active'
                 LIMIT 1",
            )?;
            let mut rows = statement.query([branch])?;
            if let Some(row) = rows.next()? {
                Ok(Some(read_workspace_isolation_lease_row(row)?))
            } else {
                Ok(None)
            }
        })
    }

    pub fn update_workspace_isolation_lease_status(
        &self,
        lease_id: &str,
        status: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE workspace_isolation_leases
                 SET status = ?2, updated_at = ?3
                 WHERE lease_id = ?1",
                rusqlite::params![lease_id, status, updated_at],
            )?;
            Ok(())
        })
    }

    pub fn insert_agent_team_task(
        &self,
        task: &AgentTeamTaskRecord,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO agent_team_tasks (
                    task_id,
                    team_id,
                    run_id,
                    title,
                    status,
                    assignee_profile_id,
                    created_at,
                    updated_at,
                    payload_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    task.task_id,
                    task.team_id,
                    task.run_id,
                    task.title,
                    task.status,
                    task.assignee_profile_id,
                    task.created_at,
                    task.updated_at,
                    task.payload_json,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_agent_team_task_status(
        &self,
        task_id: &str,
        status: &str,
        updated_at: &str,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE agent_team_tasks SET status = ?2, updated_at = ?3 WHERE task_id = ?1",
                rusqlite::params![task_id, status, updated_at],
            )?;
            Ok(())
        })
    }

    pub fn list_agent_team_tasks_for_team(
        &self,
        team_id: &str,
    ) -> Result<Vec<AgentTeamTaskRecord>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT task_id, team_id, run_id, title, status, assignee_profile_id, created_at, updated_at, payload_json
                 FROM agent_team_tasks
                 WHERE team_id = ?1
                 ORDER BY created_at ASC",
            )?;
            let mut rows = statement.query([team_id])?;
            let mut tasks = Vec::new();
            while let Some(row) = rows.next()? {
                tasks.push(AgentTeamTaskRecord {
                    task_id: row.get(0)?,
                    team_id: row.get(1)?,
                    run_id: row.get(2)?,
                    title: row.get(3)?,
                    status: row.get(4)?,
                    assignee_profile_id: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                    payload_json: row.get(8)?,
                });
            }
            Ok(tasks)
        })
    }

    pub fn insert_agent_team_mailbox_message(
        &self,
        message: &AgentTeamMailboxRecord,
    ) -> Result<(), AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO agent_team_mailbox (
                    message_id,
                    team_id,
                    sender_profile_id,
                    recipient_profile_id,
                    created_at,
                    summary,
                    payload_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    message.message_id,
                    message.team_id,
                    message.sender_profile_id,
                    message.recipient_profile_id,
                    message.created_at,
                    message.summary,
                    message.payload_json,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_agent_team_mailbox_for_team(
        &self,
        team_id: &str,
    ) -> Result<Vec<AgentTeamMailboxRecord>, AgentRuntimeStoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT message_id, team_id, sender_profile_id, recipient_profile_id, created_at, summary, payload_json
                 FROM agent_team_mailbox
                 WHERE team_id = ?1
                 ORDER BY created_at ASC",
            )?;
            let mut rows = statement.query([team_id])?;
            let mut messages = Vec::new();
            while let Some(row) = rows.next()? {
                messages.push(AgentTeamMailboxRecord {
                    message_id: row.get(0)?,
                    team_id: row.get(1)?,
                    sender_profile_id: row.get(2)?,
                    recipient_profile_id: row.get(3)?,
                    created_at: row.get(4)?,
                    summary: row.get(5)?,
                    payload_json: row.get(6)?,
                });
            }
            Ok(messages)
        })
    }
}

fn ensure_sqlite_file_no_symlink(path: &Path) -> Result<(), AgentRuntimeStoreError> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("agent runtime sqlite path has no parent"))?;
        let file_name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("agent runtime sqlite path has no file name"))?;
        let directory = std::fs::File::open(parent)?;
        let fd = rustix::fs::openat(
            &directory,
            Path::new(file_name),
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0o600),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
                std::io::Error::other("agent runtime sqlite path must not use symlinks")
            } else {
                std::io::Error::other(format!("agent runtime sqlite open failed: {error}"))
            }
        })?;
        let file = std::fs::File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(std::io::Error::other("agent runtime sqlite path is not a file").into());
        }
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(
                std::io::Error::other("agent runtime sqlite path must not use symlinks").into(),
            );
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(std::io::Error::other("agent runtime sqlite path is not a file").into());
        }
        Ok(())
    }
}

fn enable_wal_journal_mode(connection: &Connection) -> Result<(), rusqlite::Error> {
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if journal_mode.eq_ignore_ascii_case("wal") {
        Ok(())
    } else {
        Err(rusqlite::Error::InvalidParameterName(format!(
            "sqlite WAL journal mode unavailable: {journal_mode}"
        )))
    }
}

fn ensure_app_dir_no_symlink(path: &Path) -> Result<(), AgentRuntimeStoreError> {
    harness_fs::ensure_app_dir_no_symlink(path).map_err(store_fs_error)
}

fn store_fs_error(error: harness_fs::FsError) -> AgentRuntimeStoreError {
    match error {
        harness_fs::FsError::Io(source) => AgentRuntimeStoreError::Io(source),
        other => AgentRuntimeStoreError::Io(std::io::Error::other(other.to_string())),
    }
}

fn read_workspace_isolation_lease_row(
    row: &rusqlite::Row<'_>,
) -> Result<WorkspaceIsolationLease, rusqlite::Error> {
    Ok(WorkspaceIsolationLease {
        lease_id: row.get(0)?,
        conversation_id: row.get(1)?,
        run_id: row.get(2)?,
        agent_id: row.get(3)?,
        path: row.get(4)?,
        branch: row.get(5)?,
        base_commit: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn read_background_agent_row(
    row: &rusqlite::Row<'_>,
) -> Result<BackgroundAgentStoreRecord, rusqlite::Error> {
    Ok(BackgroundAgentStoreRecord {
        background_agent_id: row.get(0)?,
        conversation_id: row.get(1)?,
        run_id: row.get(2)?,
        state: parse_background_state(row.get::<_, String>(3)?.as_str())?,
        title: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        payload_json: row.get(7)?,
    })
}

pub(crate) fn background_state_wire(state: BackgroundAgentState) -> &'static str {
    match state {
        BackgroundAgentState::Queued => "queued",
        BackgroundAgentState::Running => "running",
        BackgroundAgentState::WaitingForPermission => "waiting_for_permission",
        BackgroundAgentState::WaitingForInput => "waiting_for_input",
        BackgroundAgentState::Paused => "paused",
        BackgroundAgentState::Cancelling => "cancelling",
        BackgroundAgentState::Cancelled => "cancelled",
        BackgroundAgentState::Succeeded => "succeeded",
        BackgroundAgentState::Failed => "failed",
        BackgroundAgentState::Interrupted => "interrupted",
        BackgroundAgentState::Recoverable => "recoverable",
        BackgroundAgentState::Archived => "archived",
    }
}

fn parse_background_state(value: &str) -> Result<BackgroundAgentState, rusqlite::Error> {
    match value {
        "queued" => Ok(BackgroundAgentState::Queued),
        "running" => Ok(BackgroundAgentState::Running),
        "waiting_for_permission" => Ok(BackgroundAgentState::WaitingForPermission),
        "waiting_for_input" => Ok(BackgroundAgentState::WaitingForInput),
        "paused" => Ok(BackgroundAgentState::Paused),
        "cancelling" => Ok(BackgroundAgentState::Cancelling),
        "cancelled" => Ok(BackgroundAgentState::Cancelled),
        "succeeded" => Ok(BackgroundAgentState::Succeeded),
        "failed" => Ok(BackgroundAgentState::Failed),
        "interrupted" => Ok(BackgroundAgentState::Interrupted),
        "recoverable" => Ok(BackgroundAgentState::Recoverable),
        "archived" => Ok(BackgroundAgentState::Archived),
        other => Err(rusqlite::Error::InvalidParameterName(format!(
            "unknown background agent state: {other}"
        ))),
    }
}
