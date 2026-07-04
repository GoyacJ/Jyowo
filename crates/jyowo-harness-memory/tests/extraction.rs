//! Tests for extraction worker and job queue.

use chrono::{Duration, Utc};
use harness_contracts::*;
use harness_memory::extraction::{
    ExtractionJob, ExtractionJobConfig, ExtractionJobKind, ExtractionJobQueue,
    ExtractionJobState, ExtractionWorker, ExtractionWorkerConfig,
};
use harness_memory::inbox::MemoryInbox;
use harness_memory::policy::MemoryPolicyEngine;

fn make_config() -> ExtractionWorkerConfig {
    ExtractionWorkerConfig {
        min_session_duration_seconds: 1,
        min_idle_seconds: 1,
        max_candidates_per_run: 5,
        job_config: ExtractionJobConfig {
            max_attempts: 3,
            base_backoff: Duration::seconds(1),
            max_backoff: Duration::minutes(1),
            lease_duration: Duration::seconds(10),
        },
    }
}

fn make_policy() -> MemoryPolicyEngine {
    MemoryPolicyEngine::new(MemoryGlobalSettings {
        use_memories: true,
        generate_memories: true,
        disable_generation_when_external_context_used: false,
        retention_days: None,
        max_memory_bytes: 1_000_000,
        max_recall_records_per_turn: 20,
        max_recall_chars_per_turn: 50_000,
    })
}

fn make_worker() -> ExtractionWorker {
    let inbox = MemoryInbox::new(TenantId::SINGLE);
    ExtractionWorker::new(make_config(), make_policy(), inbox)
}

#[test]
fn job_queue_enqueue_and_lease() {
    let queue = ExtractionJobQueue::new(ExtractionJobConfig::default());
    let job = ExtractionJob {
        job_id: "job-1".to_owned(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        evidence_hash: [1u8; 32],
        job_kind: ExtractionJobKind::MemoryExtraction,
        state: ExtractionJobState::Queued,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
        next_attempt_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let id = queue.enqueue(job).unwrap();
    assert_eq!(id, "job-1");

    let leased = queue.lease_next("worker-1").unwrap().unwrap();
    assert_eq!(leased.job_id, "job-1");
    assert_eq!(leased.state, ExtractionJobState::Leased);
    assert_eq!(leased.attempt_count, 1);
}

#[test]
fn job_queue_idempotency_prevents_duplicates() {
    let queue = ExtractionJobQueue::new(ExtractionJobConfig::default());
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let hash = [2u8; 32];

    let job1 = ExtractionJob {
        job_id: "job-a".to_owned(),
        tenant_id: tenant,
        session_id: session,
        run_id: run,
        evidence_hash: hash,
        job_kind: ExtractionJobKind::MemoryExtraction,
        state: ExtractionJobState::Queued,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
        next_attempt_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    queue.enqueue(job1).unwrap();

    let job2 = ExtractionJob {
        job_id: "job-b".to_owned(),
        tenant_id: tenant,
        session_id: session,
        run_id: run,
        evidence_hash: hash,
        job_kind: ExtractionJobKind::MemoryExtraction,
        state: ExtractionJobState::Queued,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
        next_attempt_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let id = queue.enqueue(job2).unwrap();
    // Should return the first job's id, not create a duplicate
    assert_eq!(id, "job-a");
}

#[test]
fn job_queue_retry_backoff_on_failure() {
    let queue = ExtractionJobQueue::new(ExtractionJobConfig::default());

    queue
        .enqueue(ExtractionJob {
            job_id: "retry-1".to_owned(),
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
            run_id: RunId::new(),
            evidence_hash: [3u8; 32],
            job_kind: ExtractionJobKind::MemoryExtraction,
            state: ExtractionJobState::Queued,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
            next_attempt_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

    // Lease and fail
    let leased = queue.lease_next("w1").unwrap().unwrap();
    queue.fail(&leased.job_id).unwrap();

    // Should not be available immediately (backoff)
    let next = queue.lease_next("w1").unwrap();
    assert!(next.is_none()); // backoff prevents immediate re-lease
}

#[test]
fn job_queue_skip_and_complete() {
    let queue = ExtractionJobQueue::new(ExtractionJobConfig::default());
    let sid = SessionId::new();
    let rid = RunId::new();

    // Job 1: skip
    queue
        .enqueue(ExtractionJob {
            job_id: "skip-me".to_owned(),
            tenant_id: TenantId::SINGLE,
            session_id: sid,
            run_id: rid,
            evidence_hash: [4u8; 32],
            job_kind: ExtractionJobKind::MemoryExtraction,
            state: ExtractionJobState::Queued,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
            next_attempt_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

    queue.lease_next("w1").unwrap();
    queue.skip("skip-me").unwrap();

    // Job 2: complete
    queue
        .enqueue(ExtractionJob {
            job_id: "complete-me".to_owned(),
            tenant_id: TenantId::SINGLE,
            session_id: sid,
            run_id: rid,
            evidence_hash: [5u8; 32],
            job_kind: ExtractionJobKind::MemoryExtraction,
            state: ExtractionJobState::Queued,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
            next_attempt_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

    queue.lease_next("w1").unwrap();
    queue.complete("complete-me").unwrap();

    // Neither should be leasable
    assert!(queue.lease_next("w1").unwrap().is_none());
}

#[test]
fn worker_skips_active_session() {
    let worker = make_worker();
    // Session not ended and idle is below minimum
    let outcome = worker
        .poll_and_process("w1", false, 0, false)
        .unwrap()
        .unwrap();
    assert!(outcome.skipped_reason.is_some());
    assert_eq!(outcome.candidates_created, 0);
}

#[test]
fn worker_skips_when_no_jobs() {
    let worker = make_worker();
    let result = worker
        .poll_and_process("w1", true, 999, false)
        .unwrap();
    assert!(result.is_none());
}
