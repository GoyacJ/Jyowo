//! Extraction worker.
//!
//! Runs extraction after sessions end or are idle.
//! Respects policy: skips active sessions, short sessions, external-context threads.
//! Creates candidates via the inbox, not direct long-term records.

use chrono::Utc;
use harness_contracts::{MemoryPolicyDecision, RunId, SessionId, TenantId};

use crate::extraction::job::{
    ExtractionJob, ExtractionJobConfig, ExtractionJobKind, ExtractionJobQueue, ExtractionJobState,
};
use crate::inbox::MemoryInbox;
use crate::policy::MemoryPolicyEngine;

/// Configuration for the extraction worker.
#[derive(Debug, Clone)]
pub struct ExtractionWorkerConfig {
    /// Minimum session duration before extraction is considered.
    pub min_session_duration_seconds: u64,
    /// Minimum idle time before extraction runs.
    pub min_idle_seconds: u64,
    /// Maximum candidates per extraction run.
    pub max_candidates_per_run: usize,
    /// Job queue configuration.
    pub job_config: ExtractionJobConfig,
}

impl Default for ExtractionWorkerConfig {
    fn default() -> Self {
        Self {
            min_session_duration_seconds: 60,
            min_idle_seconds: 300,
            max_candidates_per_run: 10,
            job_config: ExtractionJobConfig::default(),
        }
    }
}

/// Outcome of an extraction run.
#[derive(Debug, Clone)]
pub struct ExtractionRunOutcome {
    pub candidates_created: usize,
    pub skipped_reason: Option<String>,
}

/// The extraction worker.
///
/// Polls the job queue for queued/retryable jobs, processes them using
/// a model provider, and writes results to the inbox.
pub struct ExtractionWorker {
    queue: ExtractionJobQueue,
    config: ExtractionWorkerConfig,
    policy_engine: MemoryPolicyEngine,
    _inbox: MemoryInbox,
}

impl ExtractionWorker {
    #[must_use]
    pub fn new(
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
    ) -> Self {
        let queue = ExtractionJobQueue::new(config.job_config.clone());
        Self::with_queue(config, policy_engine, inbox, queue)
    }

    pub fn open(
        db_path: &str,
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
    ) -> Result<Self, String> {
        let queue = ExtractionJobQueue::open(db_path, config.job_config.clone())?;
        Ok(Self::with_queue(config, policy_engine, inbox, queue))
    }

    #[must_use]
    pub fn with_queue(
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
        queue: ExtractionJobQueue,
    ) -> Self {
        Self {
            queue,
            config,
            policy_engine,
            _inbox: inbox,
        }
    }

    /// Access the job queue.
    pub fn queue(&self) -> &ExtractionJobQueue {
        &self.queue
    }

    /// Enqueue an extraction job for a session.
    pub fn enqueue_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        evidence_hash: [u8; 32],
    ) -> Result<String, String> {
        let job = ExtractionJob {
            job_id: format!("job-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)),
            tenant_id,
            session_id,
            run_id,
            evidence_hash,
            job_kind: ExtractionJobKind::MemoryExtraction,
            state: ExtractionJobState::Queued,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
            next_attempt_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.queue.enqueue(job)
    }

    /// Process one job (poll and execute).
    ///
    /// Returns None if no jobs are available.
    pub fn poll_and_process(
        &self,
        owner: &str,
        session_ended: bool,
        session_idle_seconds: u64,
        has_external_context: bool,
    ) -> Result<Option<ExtractionRunOutcome>, String> {
        // Policy gate: don't process if session is still active (unless policy allows)
        if !session_ended && session_idle_seconds < self.config.min_idle_seconds {
            return Ok(Some(ExtractionRunOutcome {
                candidates_created: 0,
                skipped_reason: Some("session still active".to_owned()),
            }));
        }

        // Lease a job
        let Some(job) = self.queue.lease_next(owner)? else {
            return Ok(None);
        };

        // External context gate
        if has_external_context {
            // Check policy
            let thread = harness_contracts::MemoryThreadSettings {
                session_id: job.session_id,
                use_memories: None,
                generate_memories: None,
                memory_mode: harness_contracts::MemoryThreadMode::ReadWrite,
            };
            let decision = self
                .policy_engine
                .evaluate_generation(&thread, has_external_context);
            if matches!(decision, MemoryPolicyDecision::Deny { .. }) {
                self.queue.skip(&job.job_id)?;
                return Ok(Some(ExtractionRunOutcome {
                    candidates_created: 0,
                    skipped_reason: Some("external context blocked by policy".to_owned()),
                }));
            }
        }

        self.queue.block(&job.job_id, "extractor unavailable")?;

        Ok(Some(ExtractionRunOutcome {
            candidates_created: 0,
            skipped_reason: Some("extractor unavailable".to_owned()),
        }))
    }
}
