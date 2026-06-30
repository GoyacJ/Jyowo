use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{
    Event, ModelRef, SessionId, TenantId, UsageAccumulatedEvent, UsageSnapshot,
};
use jyowo_desktop_shell::commands::{
    collect_persisted_usage_events, get_model_usage_summary_with_runtime_state,
};
use jyowo_harness_sdk::ext::EventStore;
use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopRedactor};

use super::runtime_state_with_harness;

fn usage_event(model_ref: ModelRef, input_tokens: u64, diagnostic: bool) -> Event {
    Event::UsageAccumulated(UsageAccumulatedEvent {
        session_id: SessionId::new(),
        run_id: None,
        delta: UsageSnapshot {
            input_tokens,
            ..UsageSnapshot::default()
        },
        model_ref: Some(model_ref),
        pricing_snapshot_id: None,
        at: Utc::now(),
        diagnostic,
    })
}

#[tokio::test]
async fn get_model_usage_summary_aggregates_persisted_usage_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-4.1".to_owned(),
    };

    state
        .harness()
        .expect("harness")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                usage_event(model.clone(), 12, false),
                usage_event(model.clone(), 99, true),
            ],
        )
        .await
        .expect("usage events append");

    let response = get_model_usage_summary_with_runtime_state(&state)
        .await
        .expect("usage summary should succeed");

    assert_eq!(response.all_time.total.input_tokens, 12);
    assert_eq!(response.all_time.by_model.len(), 1);
    assert_eq!(response.all_time.by_model[0].key, "openai/gpt-4.1");
    assert!(response.today.period_start.is_some());
    assert!(!response.generated_at.is_empty());
}

#[tokio::test]
async fn get_model_usage_summary_requires_active_harness() {
    let workspace = super::unique_workspace("usage-summary-no-harness");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(
        workspace,
    )
    .unwrap();

    let error = get_model_usage_summary_with_runtime_state(&state)
        .await
        .expect_err("missing harness should fail closed");

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
}

#[tokio::test]
async fn collect_persisted_usage_events_reads_all_tenant_events() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_a = SessionId::new();
    let session_b = SessionId::new();
    let model = ModelRef {
        provider_id: "anthropic".to_owned(),
        model_id: "claude".to_owned(),
    };

    store
        .append(
            TenantId::SINGLE,
            session_a,
            &[usage_event(model.clone(), 3, false)],
        )
        .await
        .unwrap();
    store
        .append(
            TenantId::SINGLE,
            session_b,
            &[usage_event(model, 5, false)],
        )
        .await
        .unwrap();

    let events = collect_persisted_usage_events(store.as_ref(), TenantId::SINGLE)
        .await
        .expect("events should load");

    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .map(|event| event.delta.input_tokens)
            .sum::<u64>(),
        8
    );
}
