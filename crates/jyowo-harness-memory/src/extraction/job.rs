//! Durable extraction job queue.
//!
//! Jobs are stored with lease semantics for crash recovery.
//! Idempotency key: (tenant_id, session_id, run_id, evidence_hash, job_kind).

use chrono::{DateTime, Duration, Utc};
use harness_contracts::{RunId, SessionId, TenantId};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

use crate::local::{migrations, schema};

/// Job identifier.
pub type JobId = String;

/// Kind of extraction job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionJobKind {
    SessionSummary,
    MemoryExtraction,
    Consolidation,
}

/// Job state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionJobState {
    Queued,
    Leased,
    Blocked,
    Completed,
    Skipped,
    FailedRetryable,
    FailedPermanent,
}

/// A durable extraction job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionJob {
    pub job_id: JobId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub evidence_hash: [u8; 32],
    pub job_kind: ExtractionJobKind,
    pub state: ExtractionJobState,
    pub attempt_count: u32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Configuration for the job queue.
#[derive(Debug, Clone)]
pub struct ExtractionJobConfig {
    /// Maximum total attempts before marking failed_permanent.
    pub max_attempts: u32,
    /// Base backoff duration.
    pub base_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Lease duration (how long a worker holds a job before it's recoverable).
    pub lease_duration: Duration,
}

impl Default for ExtractionJobConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_backoff: Duration::seconds(30),
            max_backoff: Duration::hours(1),
            lease_duration: Duration::minutes(5),
        }
    }
}

/// SQLite-backed extraction job queue.
#[derive(Debug)]
pub struct ExtractionJobQueue {
    conn: Mutex<Connection>,
    config: ExtractionJobConfig,
}

impl ExtractionJobQueue {
    #[must_use]
    pub fn new(config: ExtractionJobConfig) -> Self {
        let conn = open_memory_connection().expect("open in-memory extraction job queue");
        Self {
            conn: Mutex::new(conn),
            config,
        }
    }

    pub fn open(db_path: &str, config: ExtractionJobConfig) -> Result<Self, String> {
        let conn = open_file_connection(db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
            config,
        })
    }

    /// Enqueue a job. Returns existing job if idempotency key matches.
    pub fn enqueue(&self, job: ExtractionJob) -> Result<JobId, String> {
        let mut conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("begin enqueue transaction: {e}"))?;
        if let Some(existing) = find_job_by_idempotency_key(&tx, &job)? {
            tx.commit()
                .map_err(|e| format!("commit enqueue transaction: {e}"))?;
            return Ok(existing);
        }
        let id = job.job_id.clone();
        upsert_job(&tx, &job, None)?;
        tx.commit()
            .map_err(|e| format!("commit enqueue transaction: {e}"))?;
        Ok(id)
    }

    /// Lease the next queued job. Returns None if no jobs available.
    pub fn lease_next(&self, owner: &str) -> Result<Option<ExtractionJob>, String> {
        let mut conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("begin lease transaction: {e}"))?;
        let now = Utc::now();

        let mut jobs = list_jobs(&tx)?;
        let next = jobs.iter_mut().find(|j| match j.state {
            ExtractionJobState::Queued => true,
            ExtractionJobState::Leased => j.lease_expires_at.map_or(false, |expiry| now > expiry),
            ExtractionJobState::FailedRetryable => {
                j.next_attempt_at.map_or(false, |next| now >= next)
            }
            _ => false,
        });

        if let Some(job) = next {
            job.state = ExtractionJobState::Leased;
            job.lease_owner = Some(owner.to_owned());
            job.lease_expires_at = Some(now + self.config.lease_duration);
            job.attempt_count += 1;
            job.updated_at = now;
            upsert_job(&tx, job, None)?;
            tx.commit()
                .map_err(|e| format!("commit lease transaction: {e}"))?;
            Ok(Some(job.clone()))
        } else {
            tx.commit()
                .map_err(|e| format!("commit lease transaction: {e}"))?;
            Ok(None)
        }
    }

    /// Mark a job as completed.
    pub fn complete(&self, job_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(mut job) = get_job(&conn, job_id)? {
            job.state = ExtractionJobState::Completed;
            job.lease_owner = None;
            job.lease_expires_at = None;
            job.updated_at = Utc::now();
            upsert_job(&conn, &job, None)?;
        }
        Ok(())
    }

    /// Mark a job as failed (retryable or permanent based on attempt count).
    pub fn fail(&self, job_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(mut job) = get_job(&conn, job_id)? {
            if job.attempt_count >= self.config.max_attempts {
                job.state = ExtractionJobState::FailedPermanent;
            } else {
                job.state = ExtractionJobState::FailedRetryable;
                let backoff = std::cmp::min(
                    self.config.base_backoff * (1i32 << (job.attempt_count - 1)) as i32,
                    self.config.max_backoff,
                );
                job.next_attempt_at = Some(Utc::now() + backoff);
            }
            job.lease_owner = None;
            job.lease_expires_at = None;
            job.updated_at = Utc::now();
            upsert_job(&conn, &job, None)?;
        }
        Ok(())
    }

    /// Skip a job (e.g., policy decision, quota).
    pub fn skip(&self, job_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(mut job) = get_job(&conn, job_id)? {
            job.state = ExtractionJobState::Skipped;
            job.lease_owner = None;
            job.lease_expires_at = None;
            job.updated_at = Utc::now();
            upsert_job(&conn, &job, None)?;
        }
        Ok(())
    }

    /// Block a job until extraction runtime is configured.
    pub fn block(&self, job_id: &str, reason: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(mut job) = get_job(&conn, job_id)? {
            job.state = ExtractionJobState::Blocked;
            job.lease_owner = None;
            job.lease_expires_at = None;
            job.updated_at = Utc::now();
            upsert_job(&conn, &job, Some(reason))?;
        }
        Ok(())
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

fn find_job_by_idempotency_key(
    conn: &Connection,
    job: &ExtractionJob,
) -> Result<Option<JobId>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT job_id FROM memory_extraction_jobs
             WHERE tenant_id = ?1 AND session_id = ?2 AND run_id = ?3
               AND evidence_hash = ?4 AND job_kind = ?5
             LIMIT 1",
        )
        .map_err(|e| format!("prepare idempotency lookup: {e}"))?;
    let mut rows = stmt
        .query(rusqlite::params![
            job.tenant_id.to_string(),
            job.session_id.to_string(),
            job.run_id.to_string(),
            job.evidence_hash.as_slice(),
            job_kind_to_db(job.job_kind),
        ])
        .map_err(|e| format!("query idempotency lookup: {e}"))?;

    if let Some(row) = rows
        .next()
        .map_err(|e| format!("read idempotency lookup: {e}"))?
    {
        let id: String = row.get(0).map_err(|e| format!("decode job id: {e}"))?;
        Ok(Some(id))
    } else {
        Ok(None)
    }
}

fn upsert_job(
    conn: &Connection,
    job: &ExtractionJob,
    blocked_reason: Option<&str>,
) -> Result<(), String> {
    let job_json = serde_json::to_string(job).map_err(|e| format!("serialize job: {e}"))?;
    conn.execute(
        "INSERT INTO memory_extraction_jobs (
            job_id, tenant_id, session_id, run_id, evidence_hash, job_kind, state,
            attempt_count, lease_owner, lease_expires_at, next_attempt_at, blocked_reason,
            job_json, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(job_id) DO UPDATE SET
           state = excluded.state,
           attempt_count = excluded.attempt_count,
           lease_owner = excluded.lease_owner,
           lease_expires_at = excluded.lease_expires_at,
           next_attempt_at = excluded.next_attempt_at,
           blocked_reason = excluded.blocked_reason,
           job_json = excluded.job_json,
           updated_at = excluded.updated_at",
        rusqlite::params![
            job.job_id,
            job.tenant_id.to_string(),
            job.session_id.to_string(),
            job.run_id.to_string(),
            job.evidence_hash.as_slice(),
            job_kind_to_db(job.job_kind),
            state_to_db(job.state),
            i64::from(job.attempt_count),
            job.lease_owner,
            job.lease_expires_at.map(|at| at.to_rfc3339()),
            job.next_attempt_at.map(|at| at.to_rfc3339()),
            blocked_reason,
            job_json,
            job.created_at.to_rfc3339(),
            job.updated_at.to_rfc3339(),
        ],
    )
    .map_err(|e| format!("write job: {e}"))?;
    Ok(())
}

fn get_job(conn: &Connection, job_id: &str) -> Result<Option<ExtractionJob>, String> {
    let result = conn.query_row(
        "SELECT job_json FROM memory_extraction_jobs WHERE job_id = ?1",
        rusqlite::params![job_id],
        |row| decode_job_row(row),
    );

    match result {
        Ok(job) => Ok(Some(job)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(format!("read job: {error}")),
    }
}

fn list_jobs(conn: &Connection) -> Result<Vec<ExtractionJob>, String> {
    let mut stmt = conn
        .prepare("SELECT job_json FROM memory_extraction_jobs ORDER BY created_at ASC")
        .map_err(|e| format!("prepare list jobs: {e}"))?;
    let rows = stmt
        .query_map([], decode_job_row)
        .map_err(|e| format!("query jobs: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("decode jobs: {e}"))
}

fn decode_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExtractionJob> {
    let json: String = row.get(0)?;
    serde_json::from_str::<ExtractionJob>(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn state_to_db(state: ExtractionJobState) -> String {
    serde_json::to_string(&state)
        .unwrap_or_else(|_| "\"queued\"".to_owned())
        .trim_matches('"')
        .to_owned()
}

fn job_kind_to_db(kind: ExtractionJobKind) -> String {
    serde_json::to_string(&kind)
        .unwrap_or_else(|_| "\"memory_extraction\"".to_owned())
        .trim_matches('"')
        .to_owned()
}
