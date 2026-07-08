use rusqlite::{Connection, OptionalExtension};

pub const CURRENT_SCHEMA_VERSION: i64 = 2;

pub(crate) fn migrate(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
    let result = migrate_in_transaction(connection);
    match result {
        Ok(()) => connection.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

fn migrate_in_transaction(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );
        ",
    )?;

    let version: Option<i64> = connection
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;

    match version {
        None => {
            apply_v1(connection)?;
            apply_v2(connection)?;
        }
        Some(1) => apply_v2(connection)?,
        Some(existing) if existing == CURRENT_SCHEMA_VERSION => {}
        Some(existing) => {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "unsupported agent runtime schema version {existing}"
            )));
        }
    }

    Ok(())
}

fn apply_v2(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "
        ALTER TABLE background_agent_attempts ADD COLUMN prior_attempt_id TEXT;

        DELETE FROM schema_version;
        INSERT INTO schema_version(version) VALUES (2);
        ",
    )
}

fn apply_v1(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS agent_profile_cache (
            profile_id TEXT PRIMARY KEY NOT NULL,
            scope TEXT NOT NULL,
            role TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS background_agent_registry (
            background_agent_id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT NOT NULL,
            run_id TEXT,
            state TEXT NOT NULL,
            title TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS background_agent_attempts (
            attempt_id TEXT PRIMARY KEY NOT NULL,
            background_agent_id TEXT NOT NULL,
            attempt_number INTEGER NOT NULL,
            state TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            payload_json TEXT NOT NULL,
            FOREIGN KEY(background_agent_id) REFERENCES background_agent_registry(background_agent_id)
        );

        CREATE TABLE IF NOT EXISTS agent_team_tasks (
            task_id TEXT PRIMARY KEY NOT NULL,
            team_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            assignee_profile_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_team_mailbox (
            message_id TEXT PRIMARY KEY NOT NULL,
            team_id TEXT NOT NULL,
            sender_profile_id TEXT NOT NULL,
            recipient_profile_id TEXT,
            created_at TEXT NOT NULL,
            summary TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspace_isolation_leases (
            lease_id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            path TEXT NOT NULL,
            branch TEXT,
            base_commit TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS restart_recovery_markers (
            marker_id TEXT PRIMARY KEY NOT NULL,
            background_agent_id TEXT,
            marker_kind TEXT NOT NULL,
            created_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        DELETE FROM schema_version;
        INSERT INTO schema_version(version) VALUES (1);
        ",
    )
}
