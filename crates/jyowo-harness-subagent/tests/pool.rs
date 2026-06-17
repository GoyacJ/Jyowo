use std::time::Duration;

use harness_subagent::{ConcurrencyPolicy, ConcurrentSubagentPool, ParentContext, SubagentError};

#[tokio::test]
async fn pool_enforces_per_parent_concurrency_limit() {
    let pool = ConcurrentSubagentPool::new(1);
    let parent = ParentContext::for_test(0);
    let _first = pool.acquire(&parent).await.unwrap();

    let err = pool.try_acquire(&parent).unwrap_err();
    assert_eq!(err, SubagentError::ConcurrentLimitExceeded);
}

#[tokio::test]
async fn pool_does_not_block_unrelated_parent_or_depth_bucket() {
    let pool = ConcurrentSubagentPool::new(1);
    let parent = ParentContext::for_test(0);
    let same_parent_next_depth = ParentContext {
        depth: 1,
        ..parent.clone()
    };
    let unrelated_parent = ParentContext::for_test(0);
    let _first = pool.acquire(&parent).await.unwrap();

    assert!(pool.try_acquire(&same_parent_next_depth).is_ok());
    assert!(pool.try_acquire(&unrelated_parent).is_ok());
}

#[tokio::test]
async fn pool_releases_slot_when_guard_drops() {
    let pool = ConcurrentSubagentPool::new(1);
    let parent = ParentContext::for_test(0);
    {
        let _first = pool.acquire(&parent).await.unwrap();
    }

    assert!(pool.try_acquire(&parent).is_ok());
}

#[tokio::test]
async fn pool_acquire_times_out_instead_of_waiting_forever() {
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 1,
        global_limit: 128,
        acquire_timeout: Duration::from_millis(1),
        activity_timeout: Duration::from_secs(30),
    });
    let parent = ParentContext::for_test(0);
    let _first = pool.acquire(&parent).await.unwrap();

    let err = pool.acquire(&parent).await.unwrap_err();
    assert_eq!(err, SubagentError::ConcurrentLimitExceeded);
}

#[test]
fn pool_tracks_running_subagents_and_cancels_all() {
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 1,
        global_limit: 128,
        acquire_timeout: Duration::from_millis(1),
        activity_timeout: Duration::ZERO,
    });
    let parent = ParentContext::for_test(0);
    let subagent_id = harness_contracts::SubagentId::new();
    let cancellation = pool.register_running(subagent_id, &parent, "worker".to_owned());

    assert_eq!(pool.running_count(), 1);
    assert_eq!(pool.stalled().len(), 1);
    pool.cancel_all();
    assert!(cancellation.is_cancelled());
    pool.finish(&subagent_id);
    assert_eq!(pool.running_count(), 0);
}
