use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use harness_contracts::{
    ConfigHash, ConversationModelCapability, CorrelationId, EndReason, Event, EventId, EventSource,
    EventSourceKind, Message, MessageId, MessagePart, MessageRole, ModelProtocol, ModelRef,
    PermissionMode, RunEndedEvent, RunId, RunModelSnapshot, RunStartedEvent, ServerMessage,
    SessionId, SnapshotId, TaskEventEnvelope, TaskEventHistoryPage, TaskId, TenantId, TurnInput,
    UsageAccumulatedEvent, UsageSnapshot,
};
use harness_observability::IanaTimezoneResolver;
use jyowo_desktop_shell::commands::{
    catch_up_model_usage_with_source, project_model_usage_with_source, CommandErrorPayload,
    DesktopModelUsageRollupStore, ModelUsageHistorySource, ModelUsageRollupRecord,
    ModelUsageRollupStore,
};

#[derive(Default)]
struct MemoryStore {
    record: Mutex<Option<ModelUsageRollupRecord>>,
    fail_next_save: Mutex<bool>,
}

impl MemoryStore {
    fn fail_next_save(&self) {
        *self.fail_next_save.lock().unwrap() = true;
    }

    fn record(&self) -> ModelUsageRollupRecord {
        self.record.lock().unwrap().clone().unwrap()
    }
}

impl ModelUsageRollupStore for MemoryStore {
    fn load_record(&self) -> Result<Option<ModelUsageRollupRecord>, CommandErrorPayload> {
        Ok(self.record.lock().unwrap().clone())
    }

    fn save_record(&self, record: &ModelUsageRollupRecord) -> Result<(), CommandErrorPayload> {
        if std::mem::take(&mut *self.fail_next_save.lock().unwrap()) {
            return Err(CommandErrorPayload {
                code: "RUNTIME_OPERATION_FAILED",
                message: "injected rollup write failure".to_owned(),
            });
        }
        *self.record.lock().unwrap() = Some(record.clone());
        Ok(())
    }
}

struct ScriptedSource {
    pages: Mutex<VecDeque<Result<TaskEventHistoryPage, CommandErrorPayload>>>,
    requested_after: Mutex<Vec<u64>>,
}

impl ScriptedSource {
    fn new(pages: Vec<Result<TaskEventHistoryPage, CommandErrorPayload>>) -> Self {
        Self {
            pages: Mutex::new(pages.into()),
            requested_after: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ModelUsageHistorySource for ScriptedSource {
    async fn load_events(
        &self,
        after_global_offset: u64,
        _limit: u16,
    ) -> Result<TaskEventHistoryPage, CommandErrorPayload> {
        self.requested_after
            .lock()
            .unwrap()
            .push(after_global_offset);
        self.pages.lock().unwrap().pop_front().unwrap()
    }
}

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, day, hour, 0, 0).unwrap()
}

fn usage_event(
    offset: u64,
    when: DateTime<Utc>,
    provider_id: &str,
    model_id: &str,
    diagnostic: bool,
    usage: UsageSnapshot,
) -> TaskEventEnvelope {
    engine_event(
        offset,
        when,
        Event::UsageAccumulated(UsageAccumulatedEvent {
            session_id: SessionId::new(),
            run_id: None,
            delta: usage,
            model_ref: Some(ModelRef {
                provider_id: provider_id.to_owned(),
                model_id: model_id.to_owned(),
            }),
            pricing_snapshot_id: None,
            at: when,
            diagnostic,
        }),
    )
}

fn run_started(offset: u64, run_id: RunId, when: DateTime<Utc>) -> TaskEventEnvelope {
    engine_event(
        offset,
        when,
        Event::RunStarted(RunStartedEvent {
            run_id,
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            parent_run_id: None,
            model: RunModelSnapshot {
                model_config_id: None,
                provider_id: "openrouter".to_owned(),
                model_id: "org/model".to_owned(),
                display_name: "Model".to_owned(),
                protocol: ModelProtocol::Messages,
                context_window: 128_000,
                max_output_tokens: 8_192,
                conversation_capability: ConversationModelCapability::default(),
            },
            input: TurnInput {
                message: Message {
                    id: MessageId::new(),
                    role: MessageRole::User,
                    parts: vec![MessagePart::Text("run".to_owned())],
                    created_at: when,
                },
                metadata: serde_json::Value::Null,
            },
            snapshot_id: SnapshotId::new(),
            effective_config_hash: ConfigHash([0; 32]),
            started_at: when,
            correlation_id: CorrelationId::new(),
            permission_mode: PermissionMode::Default,
        }),
    )
}

fn run_ended(offset: u64, run_id: RunId, when: DateTime<Utc>) -> TaskEventEnvelope {
    engine_event(
        offset,
        when,
        Event::RunEnded(RunEndedEvent {
            run_id,
            reason: EndReason::Completed,
            usage: None,
            ended_at: when,
        }),
    )
}

fn engine_event(offset: u64, when: DateTime<Utc>, event: Event) -> TaskEventEnvelope {
    TaskEventEnvelope {
        global_offset: offset,
        task_id: TaskId::new(),
        stream_sequence: offset,
        event_id: EventId::new(),
        event_type: match &event {
            Event::UsageAccumulated(_) => "engine.usage_accumulated",
            Event::RunStarted(_) => "engine.run_started",
            Event::RunEnded(_) => "engine.run_ended",
            _ => unreachable!(),
        }
        .to_owned(),
        schema_version: 1,
        recorded_at: when,
        source: EventSource {
            kind: EventSourceKind::Engine,
            actor_id: None,
            client_id: None,
        },
        payload: serde_json::json!({
            "tenantId": TenantId::SINGLE,
            "sessionId": SessionId::new(),
            "journalOffset": offset - 1,
            "runId": serde_json::Value::Null,
            "runSegmentId": serde_json::Value::Null,
            "correlationId": CorrelationId::new(),
            "causationId": serde_json::Value::Null,
            "event": event,
        }),
    }
}

fn page(after: u64, latest: u64, events: Vec<TaskEventEnvelope>) -> TaskEventHistoryPage {
    let next = events.last().map_or(after, |event| event.global_offset);
    TaskEventHistoryPage {
        after_global_offset: after,
        latest_global_offset: latest,
        next_after_global_offset: next,
        has_more: next < latest,
        events,
    }
}

#[tokio::test]
async fn projection_is_idempotent_excludes_diagnostics_and_keeps_model_identity_structured() {
    let store = Arc::new(MemoryStore::default());
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let seed = ScriptedSource::new(vec![Ok(page(0, 0, vec![]))]);
    project_model_usage_with_source(store.as_ref(), &seed, at(12, 12), &timezone)
        .await
        .unwrap();
    let event = usage_event(
        1,
        at(12, 10),
        "openrouter",
        "vendor/model/v2",
        false,
        UsageSnapshot {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 3,
            cache_write_tokens: 2,
            cost_micros: 99,
            tool_calls: 4,
        },
    );
    let diagnostic = usage_event(
        2,
        at(12, 11),
        "openrouter",
        "vendor/model/v2",
        true,
        UsageSnapshot {
            input_tokens: 1_000,
            tool_calls: 100,
            ..UsageSnapshot::default()
        },
    );
    let history_page = page(0, 2, vec![event, diagnostic]);
    let source = ScriptedSource::new(vec![Ok(history_page.clone()), Ok(history_page)]);
    store.fail_next_save();

    project_model_usage_with_source(store.as_ref(), &source, at(12, 12), &timezone)
        .await
        .unwrap_err();
    assert_eq!(store.record().last_global_offset, 0);
    assert_eq!(store.record().summary.all_time.total.input_tokens, 0);

    let record = project_model_usage_with_source(store.as_ref(), &source, at(12, 12), &timezone)
        .await
        .unwrap();
    assert_eq!(record.last_global_offset, 2);
    assert!(!record.dirty);
    assert!(!record.rebuilding);
    assert_eq!(record.summary.all_time.total.input_tokens, 10);
    assert_eq!(record.summary.all_time.total.tool_calls, 4);
    assert_eq!(
        record.summary.all_time.by_model[0].provider_id,
        "openrouter"
    );
    assert_eq!(
        record.summary.all_time.by_model[0].model_id,
        "vendor/model/v2"
    );
    assert_eq!(*source.requested_after.lock().unwrap(), vec![0, 0]);
}

#[tokio::test]
async fn malformed_history_pages_are_rejected_before_rollup_save_or_cursor_advance() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let event = |offset| {
        usage_event(
            offset,
            at(12, 10),
            "openai",
            "gpt-5",
            true,
            UsageSnapshot::default(),
        )
    };
    let malformed_pages = vec![
        TaskEventHistoryPage {
            after_global_offset: 0,
            latest_global_offset: 2,
            next_after_global_offset: 2,
            has_more: false,
            events: vec![],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 1,
            next_after_global_offset: 1,
            has_more: false,
            events: vec![],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 3,
            next_after_global_offset: 4,
            has_more: false,
            events: vec![],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 4,
            next_after_global_offset: 3,
            has_more: false,
            events: vec![event(3)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 3,
            next_after_global_offset: 3,
            has_more: true,
            events: vec![event(3)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 3,
            next_after_global_offset: 3,
            has_more: false,
            events: vec![event(3), event(3)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 3,
            next_after_global_offset: 3,
            has_more: false,
            events: vec![event(2), event(3)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 4,
            next_after_global_offset: 4,
            has_more: false,
            events: vec![event(4)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 5,
            next_after_global_offset: 5,
            has_more: false,
            events: vec![event(3), event(5)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 4,
            next_after_global_offset: 4,
            has_more: false,
            events: vec![event(3)],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 3,
            next_after_global_offset: 3,
            has_more: false,
            events: vec![],
        },
        TaskEventHistoryPage {
            after_global_offset: 2,
            latest_global_offset: 3,
            next_after_global_offset: 2,
            has_more: true,
            events: vec![],
        },
    ];

    for malformed_page in malformed_pages {
        let store = Arc::new(MemoryStore::default());
        let seed = ScriptedSource::new(vec![Ok(page(0, 2, vec![event(1), event(2)]))]);
        project_model_usage_with_source(store.as_ref(), &seed, at(12, 12), &timezone)
            .await
            .unwrap();
        let before = store.record();
        let source = ScriptedSource::new(vec![Ok(malformed_page)]);

        let error = project_model_usage_with_source(store.as_ref(), &source, at(12, 12), &timezone)
            .await
            .unwrap_err();

        assert!(error.message.contains("invalid"));
        assert_eq!(store.record(), before);
        assert_eq!(*source.requested_after.lock().unwrap(), vec![2]);
    }
}

#[tokio::test]
async fn cursor_and_pending_run_start_survive_restart_between_pages() {
    let root = tempfile::tempdir().unwrap();
    let runtime_root = root.path().canonicalize().unwrap();
    let store = DesktopModelUsageRollupStore::new_runtime_root(runtime_root.clone());
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let run_id = RunId::new();
    let first = ScriptedSource::new(vec![Ok(page(
        0,
        2,
        vec![run_started(1, run_id, at(11, 10))],
    ))]);

    let interrupted = project_model_usage_with_source(&store, &first, at(12, 12), &timezone)
        .await
        .unwrap();
    assert_eq!(interrupted.last_global_offset, 1);
    assert!(interrupted.dirty);
    assert!(interrupted.rebuilding);
    assert_eq!(interrupted.pending_run_starts.len(), 1);
    assert_eq!(*first.requested_after.lock().unwrap(), vec![0]);

    let reopened = DesktopModelUsageRollupStore::new_runtime_root(runtime_root);
    let second = ScriptedSource::new(vec![Ok(page(
        1,
        2,
        vec![run_ended(
            2,
            run_id,
            at(11, 10) + chrono::Duration::minutes(7),
        )],
    ))]);
    let resumed = project_model_usage_with_source(&reopened, &second, at(12, 12), &timezone)
        .await
        .unwrap();
    assert_eq!(*second.requested_after.lock().unwrap(), vec![1]);
    assert_eq!(resumed.last_global_offset, 2);
    assert!(resumed.pending_run_starts.is_empty());
    assert_eq!(resumed.longest_completed_duration_ms, 7 * 60 * 1_000);
    assert_eq!(
        resumed.summary.activity.longest_task_duration_ms,
        7 * 60 * 1_000
    );
}

#[tokio::test]
async fn catch_up_processes_multiple_pages_without_frontend_round_trips() {
    let store = Arc::new(MemoryStore::default());
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let source = ScriptedSource::new(vec![
        Ok(page(
            0,
            3,
            vec![usage_event(
                1,
                at(12, 10),
                "openai",
                "gpt-5",
                false,
                UsageSnapshot {
                    input_tokens: 10,
                    ..UsageSnapshot::default()
                },
            )],
        )),
        Ok(page(
            1,
            3,
            vec![usage_event(
                2,
                at(12, 11),
                "openai",
                "gpt-5",
                false,
                UsageSnapshot {
                    output_tokens: 5,
                    ..UsageSnapshot::default()
                },
            )],
        )),
        Ok(page(
            2,
            3,
            vec![usage_event(
                3,
                at(12, 12),
                "openai",
                "gpt-5",
                false,
                UsageSnapshot {
                    cache_read_tokens: 3,
                    ..UsageSnapshot::default()
                },
            )],
        )),
    ]);

    let record = catch_up_model_usage_with_source(store.as_ref(), &source, at(12, 12), &timezone)
        .await
        .unwrap();

    assert_eq!(*source.requested_after.lock().unwrap(), vec![0, 1, 2]);
    assert_eq!(record.last_global_offset, 3);
    assert!(!record.dirty);
    assert!(!record.rebuilding);
    assert_eq!(record.summary.all_time.total.input_tokens, 10);
    assert_eq!(record.summary.all_time.total.output_tokens, 5);
    assert_eq!(record.summary.all_time.total.cache_read_tokens, 3);
}

#[tokio::test]
async fn failed_rollup_write_does_not_advance_durable_cursor() {
    let store = Arc::new(MemoryStore::default());
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let empty = ScriptedSource::new(vec![Ok(page(0, 0, vec![]))]);
    project_model_usage_with_source(store.as_ref(), &empty, at(12, 12), &timezone)
        .await
        .unwrap();
    store.fail_next_save();
    let next = ScriptedSource::new(vec![Ok(page(
        0,
        1,
        vec![usage_event(
            1,
            at(12, 10),
            "openai",
            "gpt-5",
            false,
            UsageSnapshot {
                input_tokens: 10,
                ..UsageSnapshot::default()
            },
        )],
    ))]);

    project_model_usage_with_source(store.as_ref(), &next, at(12, 12), &timezone)
        .await
        .unwrap_err();

    assert_eq!(store.record().last_global_offset, 0);
    assert_eq!(store.record().summary.all_time.total.input_tokens, 0);
}

#[tokio::test]
async fn timezone_or_schema_change_resets_cursor_and_replays_day_buckets() {
    let store = Arc::new(MemoryStore::default());
    let utc = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let seed = ScriptedSource::new(vec![Ok(page(
        0,
        1,
        vec![usage_event(
            1,
            at(12, 1),
            "openai",
            "gpt-5",
            false,
            UsageSnapshot {
                input_tokens: 10,
                ..UsageSnapshot::default()
            },
        )],
    ))]);
    project_model_usage_with_source(store.as_ref(), &seed, at(12, 12), &utc)
        .await
        .unwrap();

    let shanghai = IanaTimezoneResolver::try_from_iana("Asia/Shanghai").unwrap();
    let replay = ScriptedSource::new(vec![Ok(page(
        0,
        1,
        vec![usage_event(
            1,
            at(12, 1),
            "openai",
            "gpt-5",
            false,
            UsageSnapshot {
                input_tokens: 10,
                ..UsageSnapshot::default()
            },
        )],
    ))]);
    let rebuilt = project_model_usage_with_source(store.as_ref(), &replay, at(12, 12), &shanghai)
        .await
        .unwrap();

    assert_eq!(*replay.requested_after.lock().unwrap(), vec![0]);
    assert_eq!(rebuilt.summary.all_time.total.input_tokens, 10);
    assert_eq!(
        rebuilt.summary.timezone_id.as_deref(),
        Some("Asia/Shanghai")
    );

    store
        .record
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .schema_version = 0;
    let schema_replay = ScriptedSource::new(vec![Ok(page(
        0,
        1,
        vec![usage_event(
            1,
            at(12, 1),
            "openai",
            "gpt-5",
            false,
            UsageSnapshot {
                input_tokens: 10,
                ..UsageSnapshot::default()
            },
        )],
    ))]);
    let schema_rebuilt =
        project_model_usage_with_source(store.as_ref(), &schema_replay, at(12, 12), &shanghai)
            .await
            .unwrap();
    assert_eq!(*schema_replay.requested_after.lock().unwrap(), vec![0]);
    assert_eq!(schema_rebuilt.summary.all_time.total.input_tokens, 10);
}

#[tokio::test]
async fn caught_up_projection_rebuilds_calendar_windows_from_day_buckets() {
    let store = Arc::new(MemoryStore::default());
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").unwrap();
    let seed = ScriptedSource::new(vec![Ok(page(
        0,
        1,
        vec![usage_event(
            1,
            at(11, 23),
            "openai",
            "gpt-5",
            false,
            UsageSnapshot {
                input_tokens: 10,
                ..UsageSnapshot::default()
            },
        )],
    ))]);
    let initial = project_model_usage_with_source(store.as_ref(), &seed, at(11, 23), &timezone)
        .await
        .unwrap();
    assert_eq!(initial.summary.today.total.input_tokens, 10);

    let next_day = ScriptedSource::new(vec![Ok(page(1, 1, vec![]))]);
    let rebuilt = project_model_usage_with_source(store.as_ref(), &next_day, at(12, 1), &timezone)
        .await
        .unwrap();
    assert_eq!(rebuilt.summary.today.total.input_tokens, 0);
    assert_eq!(rebuilt.summary.month_to_date.total.input_tokens, 10);
    assert_eq!(rebuilt.summary.all_time.total.input_tokens, 10);
}

#[test]
fn history_page_is_a_distinct_daemon_response_contract() {
    let response = ServerMessage::EventHistoryPage(page(0, 0, vec![]));
    assert!(matches!(response, ServerMessage::EventHistoryPage(_)));
}
