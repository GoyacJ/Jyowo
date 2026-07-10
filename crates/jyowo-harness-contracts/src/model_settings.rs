//! Model settings shared contracts for connectivity, usage, quota, and route health.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CapabilityRouteKind, UsageSnapshot};

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeStatus {
    Online,
    Timeout,
    Unauthenticated,
    RateLimited,
    Unsupported,
    Failed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeErrorKind {
    Timeout,
    Auth,
    RateLimit,
    Network,
    Provider,
    Unsupported,
    InvalidConfig,
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderProbeSnapshot {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub status: ProviderProbeStatus,
    pub timeout_ms: u64,
    pub latency_ms: Option<u64>,
    pub checked_at: DateTime<Utc>,
    pub error_kind: Option<ProviderProbeErrorKind>,
    pub safe_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageBucket {
    pub key: String,
    pub provider_id: String,
    pub model_id: String,
    pub usage: UsageSnapshot,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelUsagePeriod {
    Today,
    MonthToDate,
    AllTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageWindow {
    pub period: ModelUsagePeriod,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub total: UsageSnapshot,
    pub by_model: Vec<ModelUsageBucket>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageActivityDay {
    pub date: NaiveDate,
    pub usage: UsageSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageActivity {
    pub range_start: NaiveDate,
    pub range_end: NaiveDate,
    pub days: Vec<ModelUsageActivityDay>,
    pub peak_day_tokens: u64,
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    pub longest_task_duration_ms: u64,
}

fn empty_model_usage_activity() -> ModelUsageActivity {
    let date = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid default date");
    ModelUsageActivity {
        range_start: date,
        range_end: date,
        days: Vec::new(),
        peak_day_tokens: 0,
        current_streak_days: 0,
        longest_streak_days: 0,
        longest_task_duration_ms: 0,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageSummary {
    pub timezone_id: Option<String>,
    pub timezone_offset_minutes: i32,
    pub today: ModelUsageWindow,
    pub month_to_date: ModelUsageWindow,
    pub all_time: ModelUsageWindow,
    #[serde(default = "empty_model_usage_activity")]
    pub activity: ModelUsageActivity,
    pub generated_at: DateTime<Utc>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaScope {
    Account,
    Project,
    Provider,
    Model,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaStatus {
    Supported,
    Unsupported,
    NotConfigured,
    AuthRequired,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, JsonSchema)]
pub struct OfficialQuotaSnapshot {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: Option<String>,
    pub scope: OfficialQuotaScope,
    pub status: OfficialQuotaStatus,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub quota_used: Option<u64>,
    pub quota_total: Option<u64>,
    pub quota_remaining: Option<u64>,
    pub unit: Option<String>,
    pub billing_label: Option<String>,
    pub source_url: String,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub is_stale: bool,
    pub safe_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OfficialQuotaSnapshotRaw {
    config_id: String,
    provider_id: String,
    model_id: Option<String>,
    scope: OfficialQuotaScope,
    status: OfficialQuotaStatus,
    period_start: Option<DateTime<Utc>>,
    period_end: Option<DateTime<Utc>>,
    quota_used: Option<u64>,
    quota_total: Option<u64>,
    quota_remaining: Option<u64>,
    unit: Option<String>,
    billing_label: Option<String>,
    source_url: String,
    fetched_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    is_stale: bool,
    safe_message: Option<String>,
}

fn validate_official_quota_snapshot(raw: &OfficialQuotaSnapshotRaw) -> Result<(), String> {
    if raw.status != OfficialQuotaStatus::NotConfigured && raw.source_url.trim().is_empty() {
        return Err("source_url must be non-empty unless status is not_configured".to_owned());
    }

    let requires_safe_message = matches!(
        raw.status,
        OfficialQuotaStatus::Unsupported
            | OfficialQuotaStatus::AuthRequired
            | OfficialQuotaStatus::Failed
    );
    if requires_safe_message
        && raw
            .safe_message
            .as_ref()
            .is_none_or(|message| message.trim().is_empty())
    {
        return Err(format!(
            "safe_message is required for status {:?}",
            raw.status
        ));
    }

    Ok(())
}

impl<'de> Deserialize<'de> for OfficialQuotaSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = OfficialQuotaSnapshotRaw::deserialize(deserializer)?;
        validate_official_quota_snapshot(&raw).map_err(serde::de::Error::custom)?;
        Ok(Self {
            config_id: raw.config_id,
            provider_id: raw.provider_id,
            model_id: raw.model_id,
            scope: raw.scope,
            status: raw.status,
            period_start: raw.period_start,
            period_end: raw.period_end,
            quota_used: raw.quota_used,
            quota_total: raw.quota_total,
            quota_remaining: raw.quota_remaining,
            unit: raw.unit,
            billing_label: raw.billing_label,
            source_url: raw.source_url,
            fetched_at: raw.fetched_at,
            expires_at: raw.expires_at,
            is_stale: raw.is_stale,
            safe_message: raw.safe_message,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityRouteHealth {
    pub kind: CapabilityRouteKind,
    pub config_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub probe: Option<ProviderProbeSnapshot>,
}
