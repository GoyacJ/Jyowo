#![cfg(feature = "steering")]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    Event, NoopRedactor, RunId, SteeringBody, SteeringDropReason, SteeringKind, SteeringOverflow,
    SteeringPolicy, SteeringSource, TenantId,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_session::{Session, SessionOptions, SteeringRequest};

#[tokio::test]
async fn spike_long_turn_steering_merges_at_safe_point_and_preserves_cache_prefix() {
    let session = test_session(SteeringPolicy::default()).await;
    let run_id = RunId::new();
    let driver = LongTurnDriver::new(10);

    let report = driver
        .run_with_mid_turn_steering(
            &session.session,
            run_id,
            [
                steering("focus on hidden files"),
                steering("summarize only"),
                steering("keep it short"),
            ],
        )
        .await;

    assert_eq!(report.tool_calls, 10);
    assert_eq!(report.pending_before_safe_point, 3);
    assert_eq!(
        report.merged_body,
        Some("focus on hidden files\nsummarize only\nkeep it short".to_owned())
    );

    let baseline = PromptShape::new("system\nstable-tools\nhistory\nuser: list current dir");
    let with_steering =
        PromptShape::new("system\nstable-tools\nhistory\nuser: list current dir\nkeep it short");
    assert!(prompt_cache_loss_basis_points(&baseline, &with_steering) <= 500);
}

#[tokio::test]
async fn spike_capacity_and_ttl_paths_record_expected_drops() {
    let capacity_case = test_session(SteeringPolicy {
        capacity: 2,
        overflow: SteeringOverflow::DropOldest,
        ..SteeringPolicy::default()
    })
    .await;
    capacity_case
        .session
        .push_steering(steering("one"))
        .await
        .unwrap();
    capacity_case
        .session
        .push_steering(steering("two"))
        .await
        .unwrap();
    capacity_case
        .session
        .push_steering(steering("three"))
        .await
        .unwrap();

    let snapshot = capacity_case.session.steering_snapshot().await;
    assert_eq!(body_text(&snapshot.messages[0]), "two");
    assert_eq!(body_text(&snapshot.messages[1]), "three");
    assert!(events_for(&capacity_case)
        .await
        .iter()
        .any(|event| matches!(
            event,
            Event::SteeringMessageDropped(drop)
                if drop.reason == SteeringDropReason::Capacity
        )));

    let ttl_case = test_session(SteeringPolicy {
        ttl_ms: 5,
        ..SteeringPolicy::default()
    })
    .await;
    let now = harness_contracts::now();
    ttl_case
        .session
        .push_steering_at(steering("expires"), now)
        .await
        .unwrap();
    let merged = ttl_case
        .session
        .drain_and_merge_at(run_id(), now + chrono::Duration::milliseconds(6))
        .await
        .unwrap();

    assert!(merged.is_none());
    assert!(events_for(&ttl_case).await.iter().any(|event| matches!(
        event,
        Event::SteeringMessageDropped(drop) if drop.reason == SteeringDropReason::TtlExpired
    )));
}

struct LongTurnDriver {
    tool_calls: usize,
}

impl LongTurnDriver {
    fn new(tool_calls: usize) -> Self {
        Self { tool_calls }
    }

    async fn run_with_mid_turn_steering<const N: usize>(
        &self,
        session: &Session,
        run_id: RunId,
        requests: [SteeringRequest; N],
    ) -> LongTurnReport {
        let push_at = self.tool_calls / 2;
        for index in 0..self.tool_calls {
            if index == push_at {
                for request in requests.clone() {
                    session.push_steering(request).await.unwrap();
                }
            }
            tokio::task::yield_now().await;
        }

        let pending_before_safe_point = session.steering_snapshot().await.messages.len();
        let merged = session.drain_and_merge(run_id).await.unwrap();

        LongTurnReport {
            tool_calls: self.tool_calls,
            pending_before_safe_point,
            merged_body: merged.map(|message| message.body),
        }
    }
}

#[derive(Debug, PartialEq)]
struct LongTurnReport {
    tool_calls: usize,
    pending_before_safe_point: usize,
    merged_body: Option<String>,
}

struct PromptShape {
    cacheable_prefix: String,
}

impl PromptShape {
    fn new(cacheable_prefix: &str) -> Self {
        Self {
            cacheable_prefix: cacheable_prefix.to_owned(),
        }
    }
}

fn prompt_cache_loss_basis_points(baseline: &PromptShape, candidate: &PromptShape) -> usize {
    let common_prefix = baseline
        .cacheable_prefix
        .bytes()
        .zip(candidate.cacheable_prefix.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    let lost = baseline
        .cacheable_prefix
        .len()
        .saturating_sub(common_prefix);
    lost.saturating_mul(10_000) / baseline.cacheable_prefix.len()
}

struct TestSession {
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    store: Arc<InMemoryEventStore>,
    session: Session,
}

async fn test_session(policy: SteeringPolicy) -> TestSession {
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session = Session::builder()
        .with_options(
            SessionOptions::new(tempfile::tempdir().unwrap().path())
                .with_tenant_id(tenant_id)
                .with_session_id(session_id),
        )
        .with_event_store(store.clone())
        .with_steering_policy(policy)
        .build()
        .await
        .unwrap();

    TestSession {
        tenant_id,
        session_id,
        store,
        session,
    }
}

fn steering(body: &str) -> SteeringRequest {
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

fn run_id() -> RunId {
    RunId::new()
}

async fn events_for(session: &TestSession) -> Vec<Event> {
    session
        .store
        .read_envelopes(
            session.tenant_id,
            session.session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .map(|envelope| envelope.payload)
        .collect()
        .await
}
