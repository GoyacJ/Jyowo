//! Restart-stable model usage aggregation for Settings > Models.
//!
//! SPEC: docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use harness_contracts::{
    Event, ModelRef, ModelUsageBucket, ModelUsagePeriod, ModelUsageSummary, ModelUsageWindow,
    UsageAccumulatedEvent, UsageSnapshot,
};
use harness_journal::EventEnvelope;

pub trait WorkspaceTimezoneResolver {
    fn timezone_id(&self) -> Option<&str>;

    fn local_datetime(&self, utc: DateTime<Utc>) -> chrono::NaiveDateTime;

    fn offset_minutes_at(&self, utc: DateTime<Utc>) -> i32;

    fn local_day_start_utc(&self, now_utc: DateTime<Utc>) -> DateTime<Utc>;

    fn local_month_start_utc(&self, now_utc: DateTime<Utc>) -> DateTime<Utc>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IanaTimezoneResolver {
    timezone_id: String,
    tz: Tz,
}

impl IanaTimezoneResolver {
    #[must_use]
    pub fn try_from_iana(timezone_id: &str) -> Option<Self> {
        let tz: Tz = timezone_id.parse().ok()?;
        Some(Self {
            timezone_id: timezone_id.to_owned(),
            tz,
        })
    }

    fn local_midnight_utc(&self, local_date: NaiveDate) -> DateTime<Utc> {
        let local_midnight =
            local_date.and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("midnight"));
        self.tz
            .from_local_datetime(&local_midnight)
            .single()
            .expect("local midnight should be unambiguous")
            .with_timezone(&Utc)
    }
}

impl WorkspaceTimezoneResolver for IanaTimezoneResolver {
    fn timezone_id(&self) -> Option<&str> {
        Some(&self.timezone_id)
    }

    fn local_datetime(&self, utc: DateTime<Utc>) -> chrono::NaiveDateTime {
        utc.with_timezone(&self.tz).naive_local()
    }

    fn offset_minutes_at(&self, utc: DateTime<Utc>) -> i32 {
        utc.with_timezone(&self.tz).offset().fix().local_minus_utc() / 60
    }

    fn local_day_start_utc(&self, now_utc: DateTime<Utc>) -> DateTime<Utc> {
        let local = self.local_datetime(now_utc);
        self.local_midnight_utc(local.date())
    }

    fn local_month_start_utc(&self, now_utc: DateTime<Utc>) -> DateTime<Utc> {
        let local = self.local_datetime(now_utc);
        let month_start = NaiveDate::from_ymd_opt(local.year(), local.month(), 1)
            .expect("month start should be valid");
        self.local_midnight_utc(month_start)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalTimezoneResolver;

impl WorkspaceTimezoneResolver for LocalTimezoneResolver {
    fn timezone_id(&self) -> Option<&str> {
        None
    }

    fn local_datetime(&self, utc: DateTime<Utc>) -> chrono::NaiveDateTime {
        utc.with_timezone(&chrono::Local).naive_local()
    }

    fn offset_minutes_at(&self, utc: DateTime<Utc>) -> i32 {
        utc.with_timezone(&chrono::Local)
            .offset()
            .local_minus_utc()
            / 60
    }

    fn local_day_start_utc(&self, now_utc: DateTime<Utc>) -> DateTime<Utc> {
        let local = self.local_datetime(now_utc);
        let local_midnight = local
            .date()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("midnight"));
        chrono::Local
            .from_local_datetime(&local_midnight)
            .single()
            .expect("local day start should be unambiguous")
            .with_timezone(&Utc)
    }

    fn local_month_start_utc(&self, now_utc: DateTime<Utc>) -> DateTime<Utc> {
        let local = self.local_datetime(now_utc);
        let month_start = NaiveDate::from_ymd_opt(local.year(), local.month(), 1)
            .expect("month start should be valid");
        let local_midnight = month_start.and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("midnight"));
        chrono::Local
            .from_local_datetime(&local_midnight)
            .single()
            .expect("local month start should be unambiguous")
            .with_timezone(&Utc)
    }
}

#[must_use]
pub fn is_diagnostic_probe_usage(event: &UsageAccumulatedEvent) -> bool {
    event.diagnostic
}

#[must_use]
pub fn summarize_model_usage<'a, I>(
    events: I,
    now_utc: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> ModelUsageSummary
where
    I: IntoIterator<Item = &'a UsageAccumulatedEvent>,
{
    let today_start = timezone.local_day_start_utc(now_utc);
    let month_start = timezone.local_month_start_utc(now_utc);
    let period_end = now_utc;

    let mut today = WindowAccumulator::new(ModelUsagePeriod::Today, Some(today_start), Some(period_end));
    let mut month_to_date = WindowAccumulator::new(
        ModelUsagePeriod::MonthToDate,
        Some(month_start),
        Some(period_end),
    );
    let mut all_time = WindowAccumulator::new(ModelUsagePeriod::AllTime, None, None);

    for event in events {
        if is_diagnostic_probe_usage(event) {
            continue;
        }

        let in_today = event.at >= today_start && event.at <= period_end;
        let in_month = event.at >= month_start && event.at <= period_end;

        if in_today {
            today.add_event(event);
        }
        if in_month {
            month_to_date.add_event(event);
        }
        all_time.add_event(event);
    }

    ModelUsageSummary {
        timezone_id: timezone.timezone_id().map(str::to_owned),
        timezone_offset_minutes: timezone.offset_minutes_at(now_utc),
        today: today.into_window(),
        month_to_date: month_to_date.into_window(),
        all_time: all_time.into_window(),
        generated_at: now_utc,
    }
}

#[must_use]
pub fn summarize_from_events<'a, I>(
    events: I,
    now_utc: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> ModelUsageSummary
where
    I: IntoIterator<Item = &'a Event>,
{
    let usage_events = events.into_iter().filter_map(|event| match event {
        Event::UsageAccumulated(event) => Some(event),
        _ => None,
    });
    summarize_model_usage(usage_events, now_utc, timezone)
}

#[must_use]
pub fn summarize_from_envelopes<'a, I>(
    envelopes: I,
    now_utc: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> ModelUsageSummary
where
    I: IntoIterator<Item = &'a EventEnvelope>,
{
    summarize_from_events(
        envelopes.into_iter().map(|envelope| &envelope.payload),
        now_utc,
        timezone,
    )
}

#[derive(Debug, Clone)]
struct ModelBucketState {
    provider_id: String,
    model_id: String,
    usage: UsageSnapshot,
    last_used_at: Option<DateTime<Utc>>,
}

struct WindowAccumulator {
    period: ModelUsagePeriod,
    period_start: Option<DateTime<Utc>>,
    period_end: Option<DateTime<Utc>>,
    total: UsageSnapshot,
    by_model: BTreeMap<String, ModelBucketState>,
}

impl WindowAccumulator {
    fn new(
        period: ModelUsagePeriod,
        period_start: Option<DateTime<Utc>>,
        period_end: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            period,
            period_start,
            period_end,
            total: UsageSnapshot::default(),
            by_model: BTreeMap::new(),
        }
    }

    fn add_event(&mut self, event: &UsageAccumulatedEvent) {
        if usage_snapshot_is_empty(&event.delta) {
            return;
        }

        merge_usage(&mut self.total, &event.delta);

        let Some(model_ref) = &event.model_ref else {
            return;
        };

        let key = model_usage_key(model_ref);
        let bucket = self.by_model.entry(key.clone()).or_insert_with(|| ModelBucketState {
            provider_id: model_ref.provider_id.clone(),
            model_id: model_ref.model_id.clone(),
            usage: UsageSnapshot::default(),
            last_used_at: None,
        });
        merge_usage(&mut bucket.usage, &event.delta);
        bucket.last_used_at = Some(match bucket.last_used_at {
            Some(current) => current.max(event.at),
            None => event.at,
        });
    }

    fn into_window(self) -> ModelUsageWindow {
        ModelUsageWindow {
            period: self.period,
            period_start: self.period_start,
            period_end: self.period_end,
            total: self.total,
            by_model: self
                .by_model
                .into_iter()
                .map(|(key, bucket)| ModelUsageBucket {
                    key,
                    provider_id: bucket.provider_id,
                    model_id: bucket.model_id,
                    usage: bucket.usage,
                    last_used_at: bucket.last_used_at,
                })
                .collect(),
        }
    }
}

fn model_usage_key(model_ref: &ModelRef) -> String {
    format!("{}/{}", model_ref.provider_id, model_ref.model_id)
}

fn merge_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
    total.tool_calls = total.tool_calls.saturating_add(delta.tool_calls);
}

fn usage_snapshot_is_empty(snapshot: &UsageSnapshot) -> bool {
    snapshot.input_tokens == 0
        && snapshot.output_tokens == 0
        && snapshot.cache_read_tokens == 0
        && snapshot.cache_write_tokens == 0
        && snapshot.cost_micros == 0
        && snapshot.tool_calls == 0
}
