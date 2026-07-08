use rusqlite::Connection;

pub(crate) fn initialize(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
    let result = initialize_in_transaction(connection);
    match result {
        Ok(()) => connection.execute_batch("COMMIT;"),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

fn initialize_in_transaction(connection: &Connection) -> Result<(), rusqlite::Error> {
    if has_table(connection, &old_marker_table())? {
        return Err(unsupported_store_shape("old runtime marker table"));
    }
    if has_existing_runtime_tables(connection)? {
        validate_current_schema(connection)
    } else {
        create_current_schema(connection)
    }
}

fn has_existing_runtime_tables(connection: &Connection) -> Result<bool, rusqlite::Error> {
    let count: i64 = connection.query_row(
        "
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table'
          AND name NOT LIKE 'sqlite_%'
        ",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn create_current_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
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
            prior_attempt_id TEXT,
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

        ",
    )
}

fn validate_current_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    let tables = user_tables(connection)?;
    let mut expected_tables = [
        "agent_profile_cache",
        "background_agent_registry",
        "background_agent_attempts",
        "agent_team_tasks",
        "agent_team_mailbox",
        "workspace_isolation_leases",
        "restart_recovery_markers",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    expected_tables.sort();
    if tables != expected_tables {
        return Err(unsupported_store_shape(format!(
            "unexpected tables {tables:?}"
        )));
    }

    for (table, columns) in [
        (
            "agent_profile_cache",
            &["profile_id", "scope", "role", "updated_at", "payload_json"][..],
        ),
        (
            "background_agent_registry",
            &[
                "background_agent_id",
                "conversation_id",
                "run_id",
                "state",
                "title",
                "created_at",
                "updated_at",
                "payload_json",
            ][..],
        ),
        (
            "background_agent_attempts",
            &[
                "attempt_id",
                "background_agent_id",
                "prior_attempt_id",
                "attempt_number",
                "state",
                "started_at",
                "ended_at",
                "payload_json",
            ][..],
        ),
        (
            "agent_team_tasks",
            &[
                "task_id",
                "team_id",
                "run_id",
                "title",
                "status",
                "assignee_profile_id",
                "created_at",
                "updated_at",
                "payload_json",
            ][..],
        ),
        (
            "agent_team_mailbox",
            &[
                "message_id",
                "team_id",
                "sender_profile_id",
                "recipient_profile_id",
                "created_at",
                "summary",
                "payload_json",
            ][..],
        ),
        (
            "workspace_isolation_leases",
            &[
                "lease_id",
                "conversation_id",
                "run_id",
                "agent_id",
                "path",
                "branch",
                "base_commit",
                "status",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "restart_recovery_markers",
            &[
                "marker_id",
                "background_agent_id",
                "marker_kind",
                "created_at",
                "payload_json",
            ][..],
        ),
    ] {
        let actual = table_columns(connection, table)?;
        let expected = columns
            .iter()
            .map(|column| (*column).to_owned())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(unsupported_store_shape(format!(
                "unexpected columns {table}: {actual:?}"
            )));
        }
    }

    Ok(())
}

fn has_table(connection: &Connection, table: &str) -> Result<bool, rusqlite::Error> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn user_tables(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut statement = connection.prepare(
        "
        SELECT name
        FROM sqlite_master
        WHERE type = 'table'
          AND name NOT LIKE 'sqlite_%'
        ORDER BY name ASC
        ",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut tables = Vec::new();
    for row in rows {
        tables.push(row?);
    }
    Ok(tables)
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, rusqlite::Error> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    Ok(columns)
}

fn old_marker_table() -> String {
    ["schema", "version"].join("_")
}

fn unsupported_store_shape(details: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(format!(
        "unsupported agent runtime store shape: {}",
        details.into()
    ))
}
