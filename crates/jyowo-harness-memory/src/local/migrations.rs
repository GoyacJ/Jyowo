//! Manual SQLite migrations for the local memory provider.
//!
//! Inlined SQL to avoid dependency conflicts with refinery's rusqlite version pin.

use rusqlite::Connection;

/// Embedded migration: (version, SQL).
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("migrations/V1__initial_schema.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/V2__durable_memory_platform_stores.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/V3__memory_records_fts_triggers.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("migrations/V4__model_request_previews.sql"),
    },
];

struct Migration {
    version: i64,
    sql: &'static str,
}

/// Run all pending migrations against the given connection.
///
/// Creates the `schema_version` table if it doesn't exist, then applies
/// any migrations with a version greater than the current max.
pub fn run(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    if let Err(error) = run_locked(conn) {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(error);
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

fn run_locked(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
    )?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for migration in MIGRATIONS {
        if migration.version > current_version {
            conn.execute_batch(migration.sql)?;
            conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![migration.version, chrono::Utc::now().to_rfc3339()],
            )?;
        }
    }

    Ok(())
}
