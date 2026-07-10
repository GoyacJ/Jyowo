use chrono::{Datelike, TimeZone, Timelike, Utc};
use harness_contracts::{
    ConfigHash, ConversationModelCapability, CorrelationId, EndReason, Event, Message, MessageId,
    MessagePart, MessageRole, ModelProtocol, ModelRef, ModelUsagePeriod, ModelUsageSummary,
    PermissionMode, RunEndedEvent, RunId, RunModelSnapshot, RunStartedEvent, SessionId, SnapshotId,
    TenantId, TurnInput, UsageAccumulatedEvent, UsageSnapshot,
};
use harness_observability::{
    summarize_from_events, summarize_model_usage, IanaTimezoneResolver, WorkspaceTimezoneResolver,
};

fn classify_with_fixed_current_offset(
    events: &[UsageAccumulatedEvent],
    now_utc: chrono::DateTime<Utc>,
    offset_minutes: i32,
) -> ModelUsageSummary {
    struct FixedOffsetNowResolver {
        offset_minutes: i32,
    }

    impl WorkspaceTimezoneResolver for FixedOffsetNowResolver {
        fn timezone_id(&self) -> Option<&str> {
            None
        }

        fn local_datetime(&self, utc: chrono::DateTime<Utc>) -> chrono::NaiveDateTime {
            (utc + chrono::Duration::minutes(i64::from(self.offset_minutes))).naive_utc()
        }

        fn offset_minutes_at(&self, utc: chrono::DateTime<Utc>) -> i32 {
            let _ = utc;
            self.offset_minutes
        }

        fn local_day_start_utc(&self, now_utc: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
            let local = self.local_datetime(now_utc);
            let elapsed = local.time().num_seconds_from_midnight();
            now_utc - chrono::Duration::seconds(i64::from(elapsed))
        }

        fn local_month_start_utc(&self, now_utc: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
            let local = self.local_datetime(now_utc);
            let day_of_month = local.day0();
            self.local_day_start_utc(now_utc) - chrono::Duration::days(i64::from(day_of_month))
        }
    }

    summarize_model_usage(
        events.iter(),
        now_utc,
        &FixedOffsetNowResolver { offset_minutes },
    )
}

fn sample_model_ref(provider_id: &str, model_id: &str) -> ModelRef {
    ModelRef {
        provider_id: provider_id.to_owned(),
        model_id: model_id.to_owned(),
    }
}

fn usage_event(
    at: chrono::DateTime<Utc>,
    model_ref: Option<ModelRef>,
    delta: UsageSnapshot,
) -> UsageAccumulatedEvent {
    UsageAccumulatedEvent {
        session_id: SessionId::new(),
        run_id: None,
        delta,
        model_ref,
        pricing_snapshot_id: None,
        at,
        diagnostic: false,
    }
}

fn usage_delta(
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    tool_calls: u64,
    cost_micros: u64,
) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        tool_calls,
        cost_micros,
    }
}

fn total_tokens(snapshot: &UsageSnapshot) -> u64 {
    snapshot.input_tokens
        + snapshot.output_tokens
        + snapshot.cache_read_tokens
        + snapshot.cache_write_tokens
}

fn test_run_model_snapshot() -> RunModelSnapshot {
    RunModelSnapshot {
        model_config_id: None,
        provider_id: "test".to_owned(),
        model_id: "test-model".to_owned(),
        display_name: "Test Model".to_owned(),
        protocol: ModelProtocol::Messages,
        context_window: 128_000,
        max_output_tokens: 8_192,
        conversation_capability: ConversationModelCapability::default(),
    }
}

fn run_started(run_id: RunId, started_at: chrono::DateTime<Utc>) -> RunStartedEvent {
    RunStartedEvent {
        run_id,
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        parent_run_id: None,
        model: test_run_model_snapshot(),
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
    }
}

fn run_ended(run_id: RunId, ended_at: chrono::DateTime<Utc>) -> RunEndedEvent {
    RunEndedEvent {
        run_id,
        reason: EndReason::Completed,
        usage: None,
        ended_at,
    }
}

#[test]
fn model_usage_summary_aggregates_global_totals_and_by_model() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 15, 0, 0).unwrap();
    let openai = sample_model_ref("openai", "gpt-4.1");
    let anthropic = sample_model_ref("anthropic", "claude-sonnet-4");

    let events = vec![
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
            Some(openai.clone()),
            usage_delta(10, 5, 1, 2, 3, 100),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 30, 11, 0, 0).unwrap(),
            Some(anthropic.clone()),
            usage_delta(4, 6, 0, 1, 2, 50),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap(),
            None,
            usage_delta(1, 1, 0, 0, 0, 10),
        ),
    ];

    let summary = summarize_model_usage(events.iter(), now, &timezone);
    let all_time = &summary.all_time;

    assert_eq!(all_time.total.input_tokens, 15);
    assert_eq!(all_time.total.output_tokens, 12);
    assert_eq!(all_time.total.cache_read_tokens, 1);
    assert_eq!(all_time.total.cache_write_tokens, 3);
    assert_eq!(all_time.total.tool_calls, 5);
    assert_eq!(all_time.total.cost_micros, 160);
    assert_eq!(all_time.by_model.len(), 2);
    assert_eq!(all_time.by_model[0].key, "anthropic/claude-sonnet-4");
    assert_eq!(all_time.by_model[1].key, "openai/gpt-4.1");
    assert_eq!(
        all_time.by_model[1].last_used_at,
        Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap())
    );
}

#[test]
fn model_usage_summary_skips_zero_usage_rows_and_diagnostic_probe_events() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 15, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");

    let zero_usage = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
        Some(model.clone()),
        UsageSnapshot::default(),
    );
    let mut diagnostic = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 30, 11, 0, 0).unwrap(),
        Some(model.clone()),
        usage_delta(99, 0, 0, 0, 0, 0),
    );
    diagnostic.diagnostic = true;

    let summary = summarize_model_usage([zero_usage, diagnostic].iter(), now, &timezone);

    assert!(summary.all_time.by_model.is_empty());
    assert_eq!(summary.all_time.total.input_tokens, 0);
}

#[test]
fn model_usage_summary_returns_today_month_to_date_and_all_time_windows() {
    let timezone = IanaTimezoneResolver::try_from_iana("America/New_York").expect("tz");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 16, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");

    let events = vec![
        usage_event(
            Utc.with_ymd_and_hms(2026, 5, 31, 10, 0, 0).unwrap(),
            Some(model.clone()),
            usage_delta(1, 0, 0, 0, 0, 0),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 1, 10, 0, 0).unwrap(),
            Some(model.clone()),
            usage_delta(2, 0, 0, 0, 0, 0),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
            Some(model.clone()),
            usage_delta(4, 0, 0, 0, 0, 0),
        ),
    ];

    let summary = summarize_model_usage(events.iter(), now, &timezone);

    assert_eq!(summary.today.period, ModelUsagePeriod::Today);
    assert_eq!(summary.month_to_date.period, ModelUsagePeriod::MonthToDate);
    assert_eq!(summary.all_time.period, ModelUsagePeriod::AllTime);
    assert_eq!(summary.today.total.input_tokens, 4);
    assert_eq!(summary.month_to_date.total.input_tokens, 6);
    assert_eq!(summary.all_time.total.input_tokens, 7);
    assert!(summary.today.period_start.is_some());
    assert_eq!(summary.today.period_end, Some(now));
    assert_eq!(summary.month_to_date.period_end, Some(now));
    assert!(summary.all_time.period_start.is_none());
    assert!(summary.all_time.period_end.is_none());
    assert_eq!(summary.timezone_id.as_deref(), Some("America/New_York"));
}

#[test]
fn model_usage_summary_classifies_day_and_month_boundaries_with_fixed_clock() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");

    let just_before_day = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 14, 23, 59, 59).unwrap(),
        Some(model.clone()),
        usage_delta(1, 0, 0, 0, 0, 0),
    );
    let just_after_day = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 15, 0, 0, 0).unwrap(),
        Some(model.clone()),
        usage_delta(2, 0, 0, 0, 0, 0),
    );
    let just_before_month = usage_event(
        Utc.with_ymd_and_hms(2026, 5, 31, 23, 59, 59).unwrap(),
        Some(model.clone()),
        usage_delta(4, 0, 0, 0, 0, 0),
    );
    let just_after_month = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
        Some(model.clone()),
        usage_delta(8, 0, 0, 0, 0, 0),
    );

    let summary = summarize_model_usage(
        [
            just_before_day,
            just_after_day,
            just_before_month,
            just_after_month,
        ]
        .iter(),
        now,
        &timezone,
    );

    assert_eq!(summary.today.total.input_tokens, 2);
    assert_eq!(summary.month_to_date.total.input_tokens, 11);
    assert_eq!(summary.all_time.total.input_tokens, 15);
}

#[test]
fn model_usage_summary_uses_per_event_timezone_rules_across_dst_transition() {
    let timezone = IanaTimezoneResolver::try_from_iana("America/New_York").expect("tz");
    let now = Utc.with_ymd_and_hms(2025, 11, 3, 12, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");

    let today_event = usage_event(
        Utc.with_ymd_and_hms(2025, 11, 3, 10, 0, 0).unwrap(),
        Some(model.clone()),
        usage_delta(3, 0, 0, 0, 0, 0),
    );
    let previous_local_day_event = usage_event(
        Utc.with_ymd_and_hms(2025, 11, 3, 4, 30, 0).unwrap(),
        Some(model.clone()),
        usage_delta(5, 0, 0, 0, 0, 0),
    );

    let events = vec![today_event, previous_local_day_event];
    let correct = summarize_model_usage(events.iter(), now, &timezone);
    let wrong = classify_with_fixed_current_offset(&events, now, -240);

    assert_eq!(correct.today.total.input_tokens, 3);
    assert_eq!(wrong.today.total.input_tokens, 8);
}

#[test]
fn summarize_from_events_reads_usage_accumulated_events_only() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");
    let usage = usage_event(now, Some(model), usage_delta(7, 0, 0, 0, 0, 0));

    let summary = summarize_from_events([Event::UsageAccumulated(usage)].iter(), now, &timezone);

    assert_eq!(summary.all_time.total.input_tokens, 7);
}

#[test]
fn model_usage_activity_returns_365_local_days_with_zero_fill() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");
    let usage = usage_event(
        Utc.with_ymd_and_hms(2025, 7, 1, 10, 0, 0).unwrap(),
        Some(model),
        usage_delta(7, 0, 0, 0, 0, 0),
    );

    let summary = summarize_model_usage([usage].iter(), now, &timezone);

    assert_eq!(summary.activity.range_start.to_string(), "2025-07-01");
    assert_eq!(summary.activity.range_end.to_string(), "2026-06-30");
    assert_eq!(summary.activity.days.len(), 365);
    assert_eq!(summary.activity.days[0].date.to_string(), "2025-07-01");
    assert_eq!(summary.activity.days[0].usage.input_tokens, 7);
    assert_eq!(summary.activity.days[1].usage, UsageSnapshot::default());
    assert_eq!(summary.activity.days[364].date.to_string(), "2026-06-30");
}

#[test]
fn model_usage_activity_groups_by_workspace_timezone_and_skips_diagnostics() {
    let timezone = IanaTimezoneResolver::try_from_iana("Asia/Shanghai").expect("tz");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 16, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");
    let mut diagnostic = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 29, 16, 30, 0).unwrap(),
        Some(model.clone()),
        usage_delta(99, 0, 0, 0, 0, 0),
    );
    diagnostic.diagnostic = true;
    let local_june_30 = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 29, 16, 30, 0).unwrap(),
        Some(model),
        usage_delta(4, 6, 1, 2, 0, 0),
    );

    let summary = summarize_model_usage([diagnostic, local_june_30].iter(), now, &timezone);
    let day = summary
        .activity
        .days
        .iter()
        .find(|day| day.date.to_string() == "2026-06-30")
        .expect("local day exists");

    assert_eq!(total_tokens(&day.usage), 13);
    assert_eq!(summary.activity.peak_day_tokens, 13);
}

#[test]
fn model_usage_activity_calculates_current_and_longest_streaks() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");
    let events = [
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 26, 12, 0, 0).unwrap(),
            Some(model.clone()),
            usage_delta(1, 0, 0, 0, 0, 0),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 27, 12, 0, 0).unwrap(),
            Some(model.clone()),
            usage_delta(1, 0, 0, 0, 0, 0),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 28, 12, 0, 0).unwrap(),
            Some(model.clone()),
            usage_delta(1, 0, 0, 0, 0, 0),
        ),
        usage_event(
            Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap(),
            Some(model),
            usage_delta(1, 0, 0, 0, 0, 0),
        ),
    ];

    let summary = summarize_model_usage(events.iter(), now, &timezone);

    assert_eq!(summary.activity.current_streak_days, 1);
    assert_eq!(summary.activity.longest_streak_days, 3);
}

#[test]
fn model_usage_activity_current_streak_is_zero_when_today_has_no_tokens() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
    let model = sample_model_ref("openai", "gpt-4.1");
    let usage = usage_event(
        Utc.with_ymd_and_hms(2026, 6, 29, 12, 0, 0).unwrap(),
        Some(model),
        usage_delta(1, 0, 0, 0, 0, 0),
    );

    let summary = summarize_model_usage([usage].iter(), now, &timezone);

    assert_eq!(summary.activity.current_streak_days, 0);
    assert_eq!(summary.activity.longest_streak_days, 1);
}

#[test]
fn model_usage_activity_calculates_longest_completed_run_duration() {
    let timezone = IanaTimezoneResolver::try_from_iana("UTC").expect("UTC timezone");
    let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
    let fast_run = RunId::new();
    let slow_run = RunId::new();
    let missing_start = RunId::new();
    let missing_end = RunId::new();
    let negative_run = RunId::new();

    let events = [
        Event::RunStarted(run_started(
            fast_run,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
        )),
        Event::RunEnded(run_ended(
            fast_run,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 2).unwrap(),
        )),
        Event::RunStarted(run_started(
            slow_run,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
        )),
        Event::RunEnded(run_ended(
            slow_run,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 3, 0).unwrap(),
        )),
        Event::RunEnded(run_ended(
            missing_start,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
        )),
        Event::RunStarted(run_started(
            missing_end,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
        )),
        Event::RunStarted(run_started(
            negative_run,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 1, 0).unwrap(),
        )),
        Event::RunEnded(run_ended(
            negative_run,
            Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap(),
        )),
    ];

    let summary = summarize_from_events(events.iter(), now, &timezone);

    assert_eq!(summary.activity.longest_task_duration_ms, 180_000);
}
