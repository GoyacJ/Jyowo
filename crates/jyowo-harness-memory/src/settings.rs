//! Durable memory settings store.
//!
//! Stores tenant global settings and per-thread overrides in SQLite.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use harness_contracts::{
    MemoryGlobalSettings, MemoryThreadMode, MemoryThreadSettings, SessionId, TenantId,
};
use rusqlite::Connection;

use crate::local::{migrations, schema};

#[derive(Debug)]
pub struct MemorySettingsStore {
    conn: Mutex<Connection>,
}

impl MemorySettingsStore {
    #[must_use]
    pub fn new() -> Self {
        let conn = open_memory_connection().expect("open in-memory memory settings store");
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub fn open(db_path: &str) -> Result<Self, String> {
        let conn = open_file_connection(db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_global(&self, tenant_id: TenantId) -> Result<MemoryGlobalSettings, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("settings lock: {e}"))?;
        let result = conn.query_row(
            "SELECT settings_json FROM memory_global_settings WHERE tenant_id = ?1",
            rusqlite::params![tenant_id.to_string()],
            decode_global_settings_row,
        );

        match result {
            Ok(settings) => Ok(settings),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default_global_settings()),
            Err(error) => Err(format!("read global settings: {error}")),
        }
    }

    pub fn update_global(
        &self,
        tenant_id: TenantId,
        settings: MemoryGlobalSettings,
    ) -> Result<MemoryGlobalSettings, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("settings lock: {e}"))?;
        let settings_json =
            serde_json::to_string(&settings).map_err(|e| format!("serialize settings: {e}"))?;
        conn.execute(
            "INSERT INTO memory_global_settings (tenant_id, settings_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(tenant_id) DO UPDATE SET
               settings_json = excluded.settings_json,
               updated_at = excluded.updated_at",
            rusqlite::params![
                tenant_id.to_string(),
                settings_json,
                Utc::now().to_rfc3339()
            ],
        )
        .map_err(|e| format!("write global settings: {e}"))?;
        Ok(settings)
    }

    pub fn get_thread(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<MemoryThreadSettings, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("settings lock: {e}"))?;
        let result = conn.query_row(
            "SELECT settings_json FROM memory_thread_settings WHERE tenant_id = ?1 AND session_id = ?2",
            rusqlite::params![tenant_id.to_string(), session_id.to_string()],
            decode_thread_settings_row,
        );

        match result {
            Ok(settings) => Ok(settings),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default_thread_settings(session_id)),
            Err(error) => Err(format!("read thread settings: {error}")),
        }
    }

    pub fn current_memory_bytes(&self, tenant_id: TenantId) -> Result<u64, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("settings lock: {e}"))?;
        conn.query_row(
            "SELECT COALESCE(SUM(length(content)), 0)
             FROM memory_records
             WHERE tenant_id = ?1 AND deleted_at IS NULL",
            rusqlite::params![tenant_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .map(|bytes| bytes.max(0) as u64)
        .map_err(|error| format!("read memory byte usage: {error}"))
    }

    pub fn update_thread(
        &self,
        tenant_id: TenantId,
        settings: MemoryThreadSettings,
    ) -> Result<MemoryThreadSettings, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("settings lock: {e}"))?;
        let settings_json =
            serde_json::to_string(&settings).map_err(|e| format!("serialize settings: {e}"))?;
        conn.execute(
            "INSERT INTO memory_thread_settings (tenant_id, session_id, settings_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(tenant_id, session_id) DO UPDATE SET
               settings_json = excluded.settings_json,
               updated_at = excluded.updated_at",
            rusqlite::params![
                tenant_id.to_string(),
                settings.session_id.to_string(),
                settings_json,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| format!("write thread settings: {e}"))?;
        Ok(settings)
    }
}

impl Default for MemorySettingsStore {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub fn default_global_settings() -> MemoryGlobalSettings {
    MemoryGlobalSettings {
        use_memories: true,
        generate_memories: true,
        disable_generation_when_external_context_used: false,
        retention_days: None,
        max_memory_bytes: 10_000_000,
        max_recall_records_per_turn: 20,
        max_recall_chars_per_turn: 50_000,
    }
}

#[must_use]
pub fn default_thread_settings(session_id: SessionId) -> MemoryThreadSettings {
    MemoryThreadSettings {
        session_id,
        use_memories: None,
        generate_memories: None,
        memory_mode: MemoryThreadMode::ReadWrite,
    }
}

fn open_memory_connection() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|e| format!("open sqlite: {e}"))?;
    initialize_connection(&conn)?;
    Ok(conn)
}

fn open_file_connection(db_path: &str) -> Result<Connection, String> {
    if let Some(parent) = Path::new(db_path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create db directory: {e}"))?;
    }
    let conn = Connection::open(db_path).map_err(|e| format!("open sqlite: {e}"))?;
    initialize_connection(&conn)?;
    Ok(conn)
}

fn initialize_connection(conn: &Connection) -> Result<(), String> {
    for pragma in schema::CONNECTION_PRAGMAS {
        conn.execute_batch(pragma)
            .map_err(|e| format!("set sqlite pragma: {e}"))?;
    }
    migrations::run(conn).map_err(|e| format!("run migrations: {e}"))
}

fn decode_global_settings_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryGlobalSettings> {
    let json: String = row.get(0)?;
    serde_json::from_str::<MemoryGlobalSettings>(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn decode_thread_settings_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryThreadSettings> {
    let json: String = row.get(0)?;
    serde_json::from_str::<MemoryThreadSettings>(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
