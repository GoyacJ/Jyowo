use std::sync::Arc;

use chrono::{Duration, Utc};
use harness_contracts::{
    ConfigHash, ConversationModelCapability, CorrelationId, EndReason, Event, Message, MessageId,
    MessagePart, MessageRole, ModelProtocol, ModelRef, ModelRequestOptions, NoopRedactor,
    PermissionMode, RunEndedEvent, RunId, RunModelSnapshot, RunStartedEvent, SessionId, SnapshotId,
    TenantId, TurnInput, UsageAccumulatedEvent, UsageSnapshot,
};
use jyowo_desktop_shell::commands::{
    collect_persisted_usage_events, get_model_settings_page_with_runtime_state,
    get_model_usage_summary_with_runtime_state, load_model_usage_rollup_record_for_test,
    project_usage_events_into_rollup_for_test, save_model_usage_rollup_record_for_test,
    seed_usage_events_into_clean_rollup_for_test, DesktopRuntimeState, ProviderConfigRecord,
    ProviderSettingsRecord, ProviderSettingsStore,
};
use jyowo_harness_sdk::ext::EventStore;
use jyowo_harness_sdk::testing::InMemoryEventStore;
use serde_json::json;

use super::{
    runtime_state_with_harness, test_storage_layout_for_workspace, ProviderModelModalityRecord,
};

async fn runtime_state_with_openai_settings() -> DesktopRuntimeState {
    let workspace = super::unique_workspace("model-usage-openai-settings");
    runtime_state_with_openai_settings_for_workspace(workspace).await
}

async fn runtime_state_with_openai_settings_for_workspace(
    workspace: std::path::PathBuf,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace
        .canonicalize()
        .expect("test workspace should canonicalize");
    let store = super::provider_settings_store_for_workspace(&workspace);
    store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-test-config".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI test".to_owned(),
                id: "openai-test-config".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: super::openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .expect("openai provider settings should save");
    super::runtime_state_with_harness_for_workspace(workspace).await
}

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

fn run_started(run_id: RunId, started_at: chrono::DateTime<Utc>) -> Event {
    Event::RunStarted(RunStartedEvent {
        run_id,
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        parent_run_id: None,
        model: RunModelSnapshot {
            model_config_id: None,
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            display_name: "GPT-4.1".to_owned(),
            protocol: ModelProtocol::Responses,
            context_window: 128_000,
            max_output_tokens: 8192,
            conversation_capability: ConversationModelCapability::default(),
        },
        input: TurnInput {
            message: Message {
                id: MessageId::new(),
                role: MessageRole::User,
                parts: vec![MessagePart::Text("run".to_owned())],
                created_at: started_at,
            },
            metadata: serde_json::Value::Null,
        },
        snapshot_id: SnapshotId::new(),
        effective_config_hash: ConfigHash([0; 32]),
        started_at,
        correlation_id: CorrelationId::new(),
        permission_mode: PermissionMode::Default,
    })
}

fn run_ended(run_id: RunId, ended_at: chrono::DateTime<Utc>) -> Event {
    Event::RunEnded(RunEndedEvent {
        run_id,
        reason: EndReason::Completed,
        usage: None,
        ended_at,
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
async fn get_model_usage_summary_reports_longest_completed_run_duration() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let started_at = Utc::now() - Duration::seconds(45);

    state
        .harness()
        .expect("harness")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                run_started(run_id, started_at),
                run_ended(run_id, started_at + Duration::seconds(45)),
            ],
        )
        .await
        .expect("run events append");

    let response = get_model_usage_summary_with_runtime_state(&state)
        .await
        .expect("usage summary should succeed");

    assert_eq!(response.activity.longest_task_duration_ms, 45_000);
}

#[tokio::test]
async fn get_model_usage_summary_requires_active_harness() {
    let workspace = super::unique_workspace("usage-summary-no-harness");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(workspace)
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
        .append(TenantId::SINGLE, session_b, &[usage_event(model, 5, false)])
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

#[tokio::test]
async fn model_settings_page_reads_usage_from_rollup_without_harness_scan() {
    let workspace = super::unique_workspace("model-settings-page-rollup");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(workspace)
            .unwrap();
    let model = ModelRef {
        provider_id: "test".to_owned(),
        model_id: "test-model".to_owned(),
    };
    seed_usage_events_into_clean_rollup_for_test(
        &state,
        &[match usage_event(model, 17, false) {
            Event::UsageAccumulated(event) => event,
            _ => unreachable!(),
        }],
    )
    .expect("rollup seed should succeed");

    let page = get_model_settings_page_with_runtime_state(&state)
        .await
        .expect("page read should succeed without active harness");

    assert_eq!(page.usage_summary.status, "ready");
    let usage = page
        .usage_summary
        .data
        .expect("ready usage slice should include data");
    assert_eq!(usage.all_time.total.input_tokens, 17);
}

#[tokio::test]
async fn model_settings_catalog_merges_anthropic_models_api_snapshot() {
    let workspace = super::unique_workspace("model-settings-anthropic-models-api");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(
        workspace.clone(),
    )
    .unwrap();
    let layout = test_storage_layout_for_workspace(&workspace);
    let runtime_root = layout.project_runtime_root(&workspace);
    std::fs::create_dir_all(&runtime_root).unwrap();
    std::fs::write(
        runtime_root.join("provider-catalog-snapshot.json"),
        serde_json::to_vec_pretty(&json!({
            "openrouterModelsApiJson": { "data": [] },
            "anthropicModelsApiJson": {
                "data": [
                    {
                        "id": "claude-sonnet-5",
                        "type": "model",
                        "display_name": "Claude Sonnet 5",
                        "created_at": "2026-02-01T00:00:00Z",
                        "max_input_tokens": 321000,
                        "max_tokens": 123000,
                        "capabilities": {
                            "batch": true,
                            "code_execution": true,
                            "context_management": true,
                            "effort_levels": ["low", "medium", "high", "xhigh", "max"],
                            "image_input": true,
                            "pdf_input": true,
                            "structured_outputs": true,
                            "thinking_types": ["adaptive", "disabled"]
                        }
                    },
                    {
                        "id": "claude-mythos-4-20260101",
                        "type": "model",
                        "display_name": "Claude Mythos 4",
                        "created_at": "2026-01-01T00:00:00Z",
                        "max_input_tokens": 500000,
                        "max_tokens": 128000,
                        "capabilities": {
                            "batch": true,
                            "image_input": true,
                            "pdf_input": true,
                            "thinking": true
                        }
                    }
                ]
            },
            "lastSuccessfulRefreshAt": Utc::now(),
            "lastAttemptAt": Utc::now()
        }))
        .unwrap(),
    )
    .unwrap();

    let page = get_model_settings_page_with_runtime_state(&state)
        .await
        .expect("model settings page should load");
    let anthropic = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "anthropic")
        .expect("anthropic provider should be present");
    let sonnet_5 = anthropic
        .models
        .iter()
        .find(|model| model.model_id == "claude-sonnet-5")
        .expect("bundled model should remain present");
    assert_eq!(sonnet_5.context_window, 321000);
    assert_eq!(sonnet_5.max_output_tokens, 123000);
    assert_eq!(
        sonnet_5
            .provider_capability_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("supportsCodeExecution"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let mythos = anthropic
        .models
        .iter()
        .find(|model| model.model_id == "claude-mythos-4-20260101")
        .expect("models api-only model should be added");
    assert_eq!(mythos.display_name, "Claude Mythos 4");
    assert_eq!(mythos.context_window, 500000);
    assert_eq!(mythos.max_output_tokens, 128000);
    assert!(mythos
        .conversation_capability
        .input_modalities
        .contains(&ProviderModelModalityRecord::Image));
    assert!(mythos
        .conversation_capability
        .input_modalities
        .contains(&ProviderModelModalityRecord::File));
}

#[tokio::test]
async fn model_settings_page_rebuilds_missing_rollup_from_event_store() {
    let state = runtime_state_with_openai_settings().await;
    let session_id = SessionId::new();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-5.4-mini".to_owned(),
    };

    state
        .harness()
        .expect("harness")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[usage_event(model.clone(), 23, false)],
        )
        .await
        .expect("usage event append");

    let page = get_model_settings_page_with_runtime_state(&state)
        .await
        .expect("page read should rebuild missing rollup");
    let usage = page
        .usage_summary
        .data
        .expect("ready usage slice should include data");

    assert_eq!(usage.all_time.total.input_tokens, 23);
    let record = load_model_usage_rollup_record_for_test(&state)
        .expect("rollup load should work")
        .expect("rollup record should exist");
    assert!(!record.dirty);
}

#[tokio::test]
async fn model_settings_page_rebuilds_empty_rollup_created_before_harness_was_available() {
    let workspace = super::unique_workspace("model-usage-empty-rollup-before-harness");
    std::fs::create_dir_all(&workspace).unwrap();
    let no_harness_state = DesktopRuntimeState::with_workspace_for_test(workspace.clone()).unwrap();

    let page = get_model_settings_page_with_runtime_state(&no_harness_state)
        .await
        .expect("page read without harness should create dirty rollup");
    assert_eq!(page.usage_summary.status, "rebuilding");
    let empty_record = load_model_usage_rollup_record_for_test(&no_harness_state)
        .expect("rollup load should work")
        .expect("rollup record should exist");
    assert!(empty_record.dirty);

    let state = runtime_state_with_openai_settings_for_workspace(workspace).await;
    let session_id = SessionId::new();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-5.4-mini".to_owned(),
    };
    state
        .harness()
        .expect("harness")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[usage_event(model.clone(), 31, false)],
        )
        .await
        .expect("usage event append");

    let page = get_model_settings_page_with_runtime_state(&state)
        .await
        .expect("page read should rebuild dirty empty rollup");
    let usage = page
        .usage_summary
        .data
        .expect("ready usage slice should include data");
    assert_eq!(usage.all_time.total.input_tokens, 31);
}

#[tokio::test]
async fn model_settings_page_rebuilds_rollup_when_timezone_changes() {
    let state = runtime_state_with_openai_settings().await;
    let session_id = SessionId::new();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-5.4-mini".to_owned(),
    };

    state
        .harness()
        .expect("harness")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[usage_event(model.clone(), 19, false)],
        )
        .await
        .expect("usage event append");

    let stale_seed = match usage_event(model, 999, false) {
        Event::UsageAccumulated(event) => event,
        _ => unreachable!(),
    };
    seed_usage_events_into_clean_rollup_for_test(&state, &[stale_seed])
        .expect("stale rollup seed should work");
    let mut stale_record = load_model_usage_rollup_record_for_test(&state)
        .expect("rollup load should work")
        .expect("rollup record should exist");
    stale_record.summary.timezone_id = Some("Etc/Stale".to_owned());
    stale_record.summary.timezone_offset_minutes += 1;
    save_model_usage_rollup_record_for_test(&state, &stale_record)
        .expect("stale rollup save should work");

    let page = get_model_settings_page_with_runtime_state(&state)
        .await
        .expect("page read should rebuild stale timezone rollup");
    let usage = page
        .usage_summary
        .data
        .expect("ready usage slice should include data");

    assert_eq!(usage.all_time.total.input_tokens, 19);
}

#[tokio::test]
async fn projected_usage_rollup_ignores_diagnostic_events_incrementally() {
    let workspace = super::unique_workspace("model-usage-rollup-incremental");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(workspace)
            .unwrap();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-4.1".to_owned(),
    };
    let events = [
        match usage_event(model.clone(), 11, false) {
            Event::UsageAccumulated(event) => event,
            _ => unreachable!(),
        },
        match usage_event(model, 99, true) {
            Event::UsageAccumulated(event) => event,
            _ => unreachable!(),
        },
    ];

    seed_usage_events_into_clean_rollup_for_test(&state, &events)
        .expect("rollup update should work");
    let page = get_model_settings_page_with_runtime_state(&state)
        .await
        .expect("page read should succeed");
    let usage = page
        .usage_summary
        .data
        .expect("ready usage slice should include data");

    assert_eq!(usage.all_time.total.input_tokens, 11);
    assert_eq!(usage.all_time.by_model.len(), 1);
    assert_eq!(usage.all_time.by_model[0].key, "openai/gpt-4.1");
}

#[tokio::test]
async fn projected_usage_rollup_marks_missing_record_dirty_instead_of_projecting_partial_history() {
    let workspace = super::unique_workspace("model-usage-rollup-missing-incremental");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(workspace)
            .unwrap();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-4.1".to_owned(),
    };
    let event = match usage_event(model, 13, false) {
        Event::UsageAccumulated(event) => event,
        _ => unreachable!(),
    };

    project_usage_events_into_rollup_for_test(&state, &[event])
        .expect("missing rollup projection should mark dirty");

    let record = load_model_usage_rollup_record_for_test(&state)
        .expect("rollup load should work")
        .expect("rollup record should exist");
    assert!(record.dirty);
    assert_eq!(record.summary.all_time.total.input_tokens, 0);
}

#[tokio::test]
async fn projected_usage_rollup_keeps_old_schema_dirty_for_rebuild() {
    let workspace = super::unique_workspace("model-usage-rollup-schema-rebuild");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        jyowo_desktop_shell::commands::DesktopRuntimeState::with_workspace_for_test(workspace)
            .unwrap();
    let model = ModelRef {
        provider_id: "openai".to_owned(),
        model_id: "gpt-4.1".to_owned(),
    };
    let seed_event = match usage_event(model.clone(), 17, false) {
        Event::UsageAccumulated(event) => event,
        _ => unreachable!(),
    };
    seed_usage_events_into_clean_rollup_for_test(&state, &[seed_event])
        .expect("rollup seed should work");

    let mut old_record = load_model_usage_rollup_record_for_test(&state)
        .expect("rollup load should work")
        .expect("rollup record should exist");
    old_record.schema_version = 1;
    old_record.dirty = false;
    save_model_usage_rollup_record_for_test(&state, &old_record)
        .expect("old rollup save should work");

    let new_event = match usage_event(model, 5, false) {
        Event::UsageAccumulated(event) => event,
        _ => unreachable!(),
    };
    project_usage_events_into_rollup_for_test(&state, &[new_event])
        .expect("rollup projection should mark old schema dirty");

    let projected_record = load_model_usage_rollup_record_for_test(&state)
        .expect("rollup load should work")
        .expect("rollup record should exist");
    assert!(projected_record.dirty);
}
