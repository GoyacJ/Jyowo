use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{Event, TenantId, UsageAccumulatedEvent};
use harness_model::{
    default_account_usage_registry, fetch_official_quota, with_staleness, ProviderAccountUsageRegistry,
    ProviderAccountUsageRequest,
};
use harness_observability::{
    summarize_model_usage, IanaTimezoneResolver, LocalTimezoneResolver, WorkspaceTimezoneResolver,
};

use jyowo_harness_sdk::ext::EventStore;

use super::contracts::{
    ProviderProbeSnapshotPayload, RefreshOfficialQuotaResponse,
};
use super::error::{invalid_payload, runtime_operation_failed, runtime_unavailable, CommandErrorPayload};
use super::providers::{build_provider_for_config, provider_config_by_id};
use super::{
    DesktopRuntimeState, GetModelUsageSummaryResponse, ListOfficialQuotaSnapshotsResponse,
    ListProviderProbeSnapshotsResponse, ModelProtocol, ModelProvider, ProbeProviderConfigRequest,
    ProbeProviderConfigResponse, ProviderConfigRecord, ProviderDiagnosticsStore,
    ProviderProbeInput, ProviderProbeRunner,
};

pub(crate) const DEFAULT_PROBE_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MIN_PROBE_TIMEOUT_MS: u64 = 1_000;
pub(crate) const MAX_PROBE_TIMEOUT_MS: u64 = 60_000;

pub(crate) fn normalize_probe_timeout_ms(timeout_ms: Option<u64>) -> u64 {
    timeout_ms
        .unwrap_or(DEFAULT_PROBE_TIMEOUT_MS)
        .clamp(MIN_PROBE_TIMEOUT_MS, MAX_PROBE_TIMEOUT_MS)
}

pub(crate) type ProviderProbeFlights =
    Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<ProbeProviderConfigResponse>>>>>;

pub(crate) fn new_provider_probe_flights() -> ProviderProbeFlights {
    Arc::new(tokio::sync::Mutex::new(HashMap::new()))
}

pub(crate) type OfficialQuotaFlights = Arc<
    tokio::sync::Mutex<
        HashMap<String, Arc<tokio::sync::OnceCell<RefreshOfficialQuotaResponse>>>,
    >,
>;

pub(crate) fn new_official_quota_flights() -> OfficialQuotaFlights {
    Arc::new(tokio::sync::Mutex::new(HashMap::new()))
}

pub(crate) async fn run_official_quota_refresh_single_flight<F, Fut>(
    flights: &OfficialQuotaFlights,
    config_id: &str,
    refresh: F,
) -> Result<RefreshOfficialQuotaResponse, CommandErrorPayload>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<RefreshOfficialQuotaResponse, CommandErrorPayload>>,
{
    let cell = {
        let mut map = flights.lock().await;
        map.entry(config_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    let response = cell.get_or_try_init(refresh).await?.clone();
    flights.lock().await.remove(config_id);
    Ok(response)
}

pub(crate) async fn run_provider_probe_single_flight<F, Fut>(
    flights: &ProviderProbeFlights,
    config_id: &str,
    probe: F,
) -> Result<ProbeProviderConfigResponse, CommandErrorPayload>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<ProbeProviderConfigResponse, CommandErrorPayload>>,
{
    let cell = {
        let mut map = flights.lock().await;
        map.entry(config_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    let response = cell.get_or_try_init(probe).await?.clone();

    flights.lock().await.remove(config_id);
    Ok(response)
}

pub async fn probe_provider_config_with_runtime_state(
    request: ProbeProviderConfigRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<ProbeProviderConfigResponse, CommandErrorPayload> {
    let config_id = request.config_id.trim();
    if config_id.is_empty() {
        return Err(invalid_payload("configId is required".to_owned()));
    }
    let timeout_ms = normalize_probe_timeout_ms(request.timeout_ms);
    let diagnostics_store = Arc::clone(&runtime_state.provider_diagnostics_store);
    let settings_store = Arc::clone(&runtime_state.provider_settings_store);
    let flights = Arc::clone(&runtime_state.provider_probe_flights);
    let config_id_key = config_id.to_owned();
    let config_id_lookup = config_id_key.clone();

    run_provider_probe_single_flight(&flights, &config_id_key, || async move {
        let record = settings_store
            .load_record()?
            .ok_or_else(|| invalid_payload("provider config was not found".to_owned()))?;
        let config = provider_config_by_id(&record, &config_id_lookup)?;
        if config.api_key.trim().is_empty() {
            return Err(invalid_payload(
                "apiKey is not configured for this provider config".to_owned(),
            ));
        }

        let (provider, protocol) = build_provider_for_config(config)?;
        let outcome = ProviderProbeRunner::run(
            provider.as_ref(),
            ProviderProbeInput {
                config_id: config.id.clone(),
                provider_id: config.provider_id.clone(),
                model_id: config.model_id.clone(),
                timeout_ms,
            },
            protocol,
        )
        .await;

        diagnostics_store.upsert_snapshot(&outcome.snapshot)?;

        Ok(ProbeProviderConfigResponse {
            snapshot: outcome.snapshot.into(),
            diagnostic_usage: outcome.diagnostic_usage.map(Into::into),
        })
    })
    .await
}

pub fn list_provider_probe_snapshots_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<ListProviderProbeSnapshotsResponse, CommandErrorPayload> {
    let record = runtime_state.provider_diagnostics_store.load_record()?;
    Ok(ListProviderProbeSnapshotsResponse {
        snapshots: record
            .snapshots
            .into_iter()
            .map(ProviderProbeSnapshotPayload::from)
            .collect(),
    })
}

pub async fn probe_provider_config_with_provider(
    request: ProbeProviderConfigRequest,
    config: &ProviderConfigRecord,
    provider: Arc<dyn ModelProvider>,
    protocol: ModelProtocol,
    diagnostics_store: &dyn ProviderDiagnosticsStore,
    flights: &ProviderProbeFlights,
) -> Result<ProbeProviderConfigResponse, CommandErrorPayload> {
    let config_id = request.config_id.trim();
    if config_id.is_empty() {
        return Err(invalid_payload("configId is required".to_owned()));
    }
    if config.id != config_id {
        return Err(invalid_payload("provider config was not found".to_owned()));
    }
    if config.api_key.trim().is_empty() {
        return Err(invalid_payload(
            "apiKey is not configured for this provider config".to_owned(),
        ));
    }
    let timeout_ms = normalize_probe_timeout_ms(request.timeout_ms);
    let config = config.clone();
    let provider = Arc::clone(&provider);
    let config_id_key = config_id.to_owned();

    run_provider_probe_single_flight(flights, &config_id_key, || async move {
        let outcome = ProviderProbeRunner::run(
            provider.as_ref(),
            ProviderProbeInput {
                config_id: config.id.clone(),
                provider_id: config.provider_id.clone(),
                model_id: config.model_id.clone(),
                timeout_ms,
            },
            protocol,
        )
        .await;

        diagnostics_store.upsert_snapshot(&outcome.snapshot)?;

        Ok(ProbeProviderConfigResponse {
            snapshot: outcome.snapshot.into(),
            diagnostic_usage: outcome.diagnostic_usage.map(Into::into),
        })
    })
    .await
}

pub(crate) enum DesktopWorkspaceTimezone {
    Iana(IanaTimezoneResolver),
    Local(LocalTimezoneResolver),
}

impl WorkspaceTimezoneResolver for DesktopWorkspaceTimezone {
    fn timezone_id(&self) -> Option<&str> {
        match self {
            Self::Iana(resolver) => resolver.timezone_id(),
            Self::Local(resolver) => resolver.timezone_id(),
        }
    }

    fn local_datetime(&self, utc: chrono::DateTime<Utc>) -> chrono::NaiveDateTime {
        match self {
            Self::Iana(resolver) => resolver.local_datetime(utc),
            Self::Local(resolver) => resolver.local_datetime(utc),
        }
    }

    fn offset_minutes_at(&self, utc: chrono::DateTime<Utc>) -> i32 {
        match self {
            Self::Iana(resolver) => resolver.offset_minutes_at(utc),
            Self::Local(resolver) => resolver.offset_minutes_at(utc),
        }
    }

    fn local_day_start_utc(&self, now_utc: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
        match self {
            Self::Iana(resolver) => resolver.local_day_start_utc(now_utc),
            Self::Local(resolver) => resolver.local_day_start_utc(now_utc),
        }
    }

    fn local_month_start_utc(&self, now_utc: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
        match self {
            Self::Iana(resolver) => resolver.local_month_start_utc(now_utc),
            Self::Local(resolver) => resolver.local_month_start_utc(now_utc),
        }
    }
}

pub(crate) fn workspace_timezone_resolver() -> DesktopWorkspaceTimezone {
    if let Ok(timezone_id) = iana_time_zone::get_timezone() {
        if let Some(resolver) = IanaTimezoneResolver::try_from_iana(&timezone_id) {
            return DesktopWorkspaceTimezone::Iana(resolver);
        }
    }
    DesktopWorkspaceTimezone::Local(LocalTimezoneResolver)
}

pub async fn collect_persisted_usage_events(
    store: &dyn EventStore,
    tenant: TenantId,
) -> Result<Vec<UsageAccumulatedEvent>, CommandErrorPayload> {
    const PAGE_SIZE: usize = 1024;
    let mut events = Vec::new();
    let mut after = None;

    loop {
        let batch = store
            .query_after(tenant, after, PAGE_SIZE)
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("usage summary read failed: {error}"))
            })?;
        let batch_len = batch.len();
        for envelope in batch {
            after = Some(envelope.event_id);
            if let Event::UsageAccumulated(usage) = envelope.payload {
                events.push(usage);
            }
        }
        if batch_len < PAGE_SIZE {
            break;
        }
    }

    Ok(events)
}

pub async fn get_model_usage_summary_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<GetModelUsageSummaryResponse, CommandErrorPayload> {
    let harness = runtime_state
        .harness()
        .ok_or_else(|| runtime_unavailable("Model usage summary requires an active harness runtime."))?;
    let events = collect_persisted_usage_events(harness.event_store().as_ref(), TenantId::SINGLE)
        .await?;
    let now = Utc::now();
    let timezone = workspace_timezone_resolver();
    let summary = summarize_model_usage(events.iter(), now, &timezone);
    Ok(summary.into())
}

pub(crate) fn account_usage_registry() -> ProviderAccountUsageRegistry {
    default_account_usage_registry()
}

pub async fn refresh_official_quota_with_runtime_state(
    config_id: &str,
    runtime_state: &DesktopRuntimeState,
) -> Result<RefreshOfficialQuotaResponse, CommandErrorPayload> {
    let config_id = config_id.trim();
    if config_id.is_empty() {
        return Err(invalid_payload("configId is required".to_owned()));
    }

    let quota_store = Arc::clone(&runtime_state.provider_quota_cache_store);
    let settings_store = Arc::clone(&runtime_state.provider_settings_store);
    let flights = Arc::clone(&runtime_state.official_quota_flights);
    let config_id_key = config_id.to_owned();

    let config_id_lookup = config_id_key.clone();

    run_official_quota_refresh_single_flight(&flights, &config_id_key, || async move {
        let record = settings_store
            .load_record()?
            .ok_or_else(|| invalid_payload("provider config was not found".to_owned()))?;
        let config = provider_config_by_id(&record, &config_id_lookup)?;
        let registry = account_usage_registry();
        let snapshot = fetch_official_quota(
            &registry,
            ProviderAccountUsageRequest {
                config_id: config.id.clone(),
                provider_id: config.provider_id.clone(),
                model_id: Some(config.model_id.clone()),
                api_key: config.api_key.clone(),
                base_url: config.base_url.clone(),
            },
        )
        .await;
        quota_store.upsert_snapshot(&snapshot)?;
        Ok(RefreshOfficialQuotaResponse {
            snapshot: with_staleness(snapshot, Utc::now()).into(),
        })
    })
    .await
}

pub fn list_official_quota_snapshots_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<ListOfficialQuotaSnapshotsResponse, CommandErrorPayload> {
    let record = runtime_state.provider_quota_cache_store.load_record()?;
    let now = Utc::now();
    Ok(ListOfficialQuotaSnapshotsResponse {
        snapshots: record
            .snapshots
            .into_iter()
            .map(|snapshot| with_staleness(snapshot, now).into())
            .collect(),
    })
}
