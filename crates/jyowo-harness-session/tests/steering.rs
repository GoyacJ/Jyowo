#![cfg(feature = "steering")]

use std::sync::Arc;

use harness_contracts::{
    EndReason, NoopRedactor, RunId, SteeringBody, SteeringKind, SteeringOverflow, SteeringPolicy,
    SteeringSource, TenantId,
};
use harness_journal::InMemoryEventStore;
use harness_session::{Session, SessionOptions, SteeringRequest};

#[tokio::test]
async fn enqueue_snapshot_and_dedup_work() {
    let session = test_session(SteeringPolicy::default()).await;
    let first = session.push_steering(text("focus")).await.unwrap();
    let second = session.push_steering(text("focus")).await.unwrap();

    let snapshot = session.steering_snapshot().await;

    assert_eq!(first, second);
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.policy.capacity, 8);
}

#[tokio::test]
async fn overflow_policies_are_applied() {
    let drop_oldest = test_session(SteeringPolicy {
        capacity: 1,
        overflow: SteeringOverflow::DropOldest,
        ..SteeringPolicy::default()
    })
    .await;
    drop_oldest.push_steering(text("old")).await.unwrap();
    drop_oldest.push_steering(text("new")).await.unwrap();
    assert_eq!(
        body_text(&drop_oldest.steering_snapshot().await.messages[0]),
        "new"
    );

    let drop_newest = test_session(SteeringPolicy {
        capacity: 1,
        overflow: SteeringOverflow::DropNewest,
        ..SteeringPolicy::default()
    })
    .await;
    drop_newest.push_steering(text("old")).await.unwrap();
    assert!(drop_newest.push_steering(text("new")).await.is_err());
    assert_eq!(
        body_text(&drop_newest.steering_snapshot().await.messages[0]),
        "old"
    );

    let disabled = test_session(SteeringPolicy {
        capacity: 0,
        ..SteeringPolicy::default()
    })
    .await;
    assert!(disabled.push_steering(text("drop")).await.is_err());
    assert!(disabled.steering_snapshot().await.messages.is_empty());
}

#[tokio::test]
async fn ttl_expired_messages_are_dropped_on_drain() {
    let session = test_session(SteeringPolicy {
        ttl_ms: 10,
        ..SteeringPolicy::default()
    })
    .await;
    let now = harness_contracts::now();
    session.push_steering_at(text("old"), now).await.unwrap();

    let merged = session
        .drain_and_merge_at(RunId::new(), now + chrono::Duration::milliseconds(11))
        .await
        .unwrap();

    assert!(merged.is_none());
    assert!(session.steering_snapshot().await.messages.is_empty());
}

#[tokio::test]
async fn drain_merges_append_replace_and_ignores_nudge_body() {
    let session = test_session(SteeringPolicy::default()).await;
    session.push_steering(text("append-a")).await.unwrap();
    session
        .push_steering(SteeringRequest {
            kind: SteeringKind::NudgeOnly,
            ..text("nudge")
        })
        .await
        .unwrap();
    session
        .push_steering(SteeringRequest {
            kind: SteeringKind::Replace,
            ..text("replace")
        })
        .await
        .unwrap();
    session.push_steering(text("append-b")).await.unwrap();

    let merged = session
        .drain_and_merge(RunId::new())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(merged.body, "replace\nappend-b");
    assert_eq!(merged.ids.len(), 4);
    assert!(session.steering_snapshot().await.messages.is_empty());
}

#[tokio::test]
async fn run_end_keeps_pending_messages_but_session_end_clears_them() {
    let session = test_session(SteeringPolicy::default()).await;
    session.push_steering(text("next turn")).await.unwrap();

    session
        .handle_run_ended_for_test(RunId::new())
        .await
        .unwrap();
    assert_eq!(session.steering_snapshot().await.messages.len(), 1);

    session.end(EndReason::Completed).await.unwrap();
    assert!(session.steering_snapshot().await.messages.is_empty());
}

#[tokio::test]
async fn fork_does_not_inherit_steering_queue() {
    let session = test_session(SteeringPolicy::default()).await;
    session.push_steering(text("parent only")).await.unwrap();

    let child = session
        .fork(harness_contracts::ForkReason::UserRequested)
        .await
        .unwrap();

    assert_eq!(session.steering_snapshot().await.messages.len(), 1);
    assert!(child.steering_snapshot().await.messages.is_empty());
}

async fn test_session(policy: SteeringPolicy) -> Session {
    Session::builder()
        .with_options(
            SessionOptions::new(tempfile::tempdir().unwrap().path())
                .with_tenant_id(TenantId::SINGLE),
        )
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_steering_policy(policy)
        .build()
        .await
        .unwrap()
}

fn text(body: &str) -> SteeringRequest {
    SteeringRequest {
        kind: SteeringKind::Append,
        body: SteeringBody::Text(body.to_owned()),
        priority: None,
        correlation_id: None,
        source: SteeringSource::User,
    }
}

fn body_text(message: &harness_contracts::SteeringMessage) -> &str {
    match &message.body {
        SteeringBody::Text(text) => text,
        SteeringBody::Structured { instruction, .. } => instruction,
        _ => "",
    }
}
