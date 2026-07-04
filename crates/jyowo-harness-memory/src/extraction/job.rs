//! Durable extraction job queue.
//!
//! Jobs are stored with lease semantics for crash recovery.
//! Idempotency key: (tenant_id, session_id, run_id, evidence_hash, job_kind).

use chrono::{DateTime, Duration, Utc};
use harness_contracts::{RunId, SessionId, TenantId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

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

/// In-memory job queue (for testing and initial implementation).
#[derive(Debug, Default)]
pub struct ExtractionJobQueue {
    jobs: Mutex<HashMap<JobId, ExtractionJob>>,
    config: ExtractionJobConfig,
}

impl ExtractionJobQueue {
    #[must_use]
    pub fn new(config: ExtractionJobConfig) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Enqueue a job. Returns existing job if idempotency key matches.
    pub fn enqueue(&self, job: ExtractionJob) -> Result<JobId, String> {
        let mut jobs = self.jobs.lock().map_err(|e| format!("lock: {e}"))?;
        // Idempotency: check for existing job with same key
        let existing = jobs.values().find(|j| {
            j.tenant_id == job.tenant_id
                && j.session_id == job.session_id
                && j.run_id == job.run_id
                && j.evidence_hash == job.evidence_hash
                && j.job_kind == job.job_kind
        });
        if let Some(existing) = existing {
            return Ok(existing.job_id.clone());
        }
        let id = job.job_id.clone();
        jobs.insert(id.clone(), job);
        Ok(id)
    }

    /// Lease the next queued job. Returns None if no jobs available.
    pub fn lease_next(&self, owner: &str) -> Result<Option<ExtractionJob>, String> {
        let mut jobs = self.jobs.lock().map_err(|e| format!("lock: {e}"))?;
        let now = Utc::now();

        // Find first queued job or job with expired lease
        let next = jobs
            .values_mut()
            .filter(|j| match j.state {
                ExtractionJobState::Queued => true,
                ExtractionJobState::Leased => {
                    j.lease_expires_at.map_or(false, |expiry| now > expiry)
                }
                ExtractionJobState::FailedRetryable => {
                    j.next_attempt_at.map_or(false, |next| now >= next)
                }
                _ => false,
            })
            .next();

        if let Some(job) = next {
            job.state = ExtractionJobState::Leased;
            job.lease_owner = Some(owner.to_owned());
            job.lease_expires_at = Some(now + self.config.lease_duration);
            job.attempt_count += 1;
            job.updated_at = now;
            Ok(Some(job.clone()))
        } else {
            Ok(None)
        }
    }

    /// Mark a job as completed.
    pub fn complete(&self, job_id: &str) -> Result<(), String> {
        let mut jobs = self.jobs.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(job) = jobs.get_mut(job_id) {
            job.state = ExtractionJobState::Completed;
            job.updated_at = Utc::now();
        }
        Ok(())
    }

    /// Mark a job as failed (retryable or permanent based on attempt count).
    pub fn fail(&self, job_id: &str) -> Result<(), String> {
        let mut jobs = self.jobs.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(job) = jobs.get_mut(job_id) {
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
        }
        Ok(())
    }

    /// Skip a job (e.g., policy decision, quota).
    pub fn skip(&self, job_id: &str) -> Result<(), String> {
        let mut jobs = self.jobs.lock().map_err(|e| format!("lock: {e}"))?;
        if let Some(job) = jobs.get_mut(job_id) {
            job.state = ExtractionJobState::Skipped;
            job.updated_at = Utc::now();
        }
        Ok(())
    }
}
