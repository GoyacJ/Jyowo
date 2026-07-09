use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    Event, EventId, ForkReason, JournalError, JournalOffset, ModelRef, ModelUsageBucket,
    ModelUsagePeriod, ModelUsageSummary, ModelUsageWindow, SessionId, TenantId,
    UsageAccumulatedEvent, UsageSnapshot,
};
use harness_journal::{
    AppendMetadata, EventEnvelope, EventEnvelopePage, PrunePolicy, PruneReport, ReplayCursor,
    SessionFilter, SessionSnapshot, SessionSummary,
};
use harness_model::{fetch_official_quota, with_staleness, ProviderAccountUsageRequest};
use harness_observability::{
    summarize_model_usage, IanaTimezoneResolver, LocalTimezoneResolver, WorkspaceTimezoneResolver,
};

use futures::stream::BoxStream;
use jyowo_harness_sdk::ext::{
    inventory_from_models_api_json, runnable_inventory_models, EventStore,
};

use super::contracts::{
    ModelSettingsCatalogSnapshotPayload, ModelSettingsPageResponse, ModelSettingsPageSlice,
    ModelUsageRollupRecord, ModelUsageRollupStore, ProviderCatalogSnapshotRecord,
    ProviderProbeSnapshotPayload, RefreshModelProviderCatalogResponse,
    RefreshOfficialQuotaResponse,
};
use super::error::{
    invalid_payload, runtime_operation_failed, runtime_unavailable, CommandErrorPayload,
};
use super::providers::{
    build_provider_for_config, desktop_provider_service_adapter_availability,
    list_model_provider_catalog_payload, list_provider_capability_route_options_from_inputs,
    list_provider_capability_routes_with_store, list_provider_settings_with_store,
    model_descriptor_catalog_entry, provider_config_by_id,
};
use super::{
    DesktopRuntimeState, GetModelUsageSummaryResponse, ListOfficialQuotaSnapshotsResponse,
    ListProviderProbeSnapshotsResponse, ModelProtocol, ModelProvider, ModelProviderCatalogResponse,
    ProbeProviderConfigRequest, ProbeProviderConfigResponse, ProviderConfigRecord,
    ProviderDiagnosticsStore, ProviderProbeInput, ProviderProbeRunner,
};

pub(crate) const DEFAULT_PROBE_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MIN_PROBE_TIMEOUT_MS: u64 = 1_000;
pub(crate) const MAX_PROBE_TIMEOUT_MS: u64 = 60_000;

pub(crate) fn normalize_probe_timeout_ms(timeout_ms: Option<u64>) -> u64 {
    timeout_ms
        .unwrap_or(DEFAULT_PROBE_TIMEOUT_MS)
        .clamp(MIN_PROBE_TIMEOUT_MS, MAX_PROBE_TIMEOUT_MS)
}

const MODEL_USAGE_ROLLUP_SCHEMA_VERSION: u32 = 1;
const MAX_OPENROUTER_MODELS_API_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
struct UsageBucketState {
    provider_id: String,
    model_id: String,
    usage: UsageSnapshot,
    last_used_at: Option<DateTime<Utc>>,
}

pub(crate) struct ProjectingEventStore {
    inner: Arc<dyn EventStore>,
    model_usage_rollup_store: Arc<dyn ModelUsageRollupStore>,
}

impl ProjectingEventStore {
    pub(crate) fn new(
        inner: Arc<dyn EventStore>,
        model_usage_rollup_store: Arc<dyn ModelUsageRollupStore>,
    ) -> Self {
        Self {
            inner,
            model_usage_rollup_store,
        }
    }

    fn project_usage_events(&self, events: &[Event]) {
        let usage_events = events
            .iter()
            .filter_map(|event| match event {
                Event::UsageAccumulated(event) => Some(event.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if usage_events.is_empty() {
            return;
        }
        if let Err(error) = project_usage_events_into_store(
            self.model_usage_rollup_store.as_ref(),
            &usage_events,
            Utc::now(),
        ) {
            log::warn!("model usage rollup projection failed: {}", error.message);
        }
    }
}

#[async_trait]
impl EventStore for ProjectingEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let result = self.inner.append(tenant, session_id, events).await;
        if result.is_ok() {
            self.project_usage_events(events);
        }
        result
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let result = self
            .inner
            .append_with_metadata(tenant, session_id, metadata, events)
            .await;
        if result.is_ok() {
            self.project_usage_events(events);
        }
        result
    }

    async fn append_with_metadata_expect_next_offset(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        expected_next_offset: JournalOffset,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let result = self
            .inner
            .append_with_metadata_expect_next_offset(
                tenant,
                session_id,
                metadata,
                expected_next_offset,
                events,
            )
            .await;
        if result.is_ok() {
            self.project_usage_events(events);
        }
        result
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        self.inner.read_envelopes(tenant, session_id, cursor).await
    }

    async fn page_session_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        after_event_id: Option<EventId>,
        limit: usize,
    ) -> Result<EventEnvelopePage, JournalError> {
        self.inner
            .page_session_envelopes(tenant, session_id, after_event_id, limit)
            .await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        self.inner.query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        self.inner.delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        self.inner.prune(tenant, policy).await
    }

    async fn prune_sessions(
        &self,
        tenant: TenantId,
        session_ids: &[SessionId],
        keep_snapshots: bool,
    ) -> Result<PruneReport, JournalError> {
        self.inner
            .prune_sessions(tenant, session_ids, keep_snapshots)
            .await
    }
}

pub async fn get_model_settings_page_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<ModelSettingsPageResponse, CommandErrorPayload> {
    let now = Utc::now();
    let (catalog, catalog_snapshot) = local_model_provider_catalog(runtime_state)?;
    let provider_record = runtime_state
        .provider_settings_store
        .load_record()?
        .unwrap_or_default();
    let provider_settings =
        list_provider_settings_with_store(runtime_state.provider_settings_store.as_ref()).await?;
    let adapter_availability = desktop_provider_service_adapter_availability(runtime_state);

    let probe_snapshots = slice_from_result(list_provider_probe_snapshots_with_runtime_state(
        runtime_state,
    ));
    let usage_summary = usage_summary_slice_from_rollup(runtime_state, now);
    let quota_snapshots = slice_from_result(list_official_quota_snapshots_with_runtime_state(
        runtime_state,
    ));
    let capability_routes = slice_from_result(
        list_provider_capability_routes_with_store(
            runtime_state.provider_capability_route_store.as_ref(),
            &provider_record,
            &catalog,
            &adapter_availability,
        )
        .await,
    );
    let capability_route_options =
        slice_from_result(list_provider_capability_route_options_from_inputs(
            runtime_state.provider_capability_route_store.as_ref(),
            &provider_record,
            &catalog,
            &adapter_availability,
        ));

    Ok(ModelSettingsPageResponse {
        catalog,
        catalog_snapshot,
        provider_settings,
        probe_snapshots,
        usage_summary,
        quota_snapshots,
        capability_routes,
        capability_route_options,
        generated_at: now.to_rfc3339(),
    })
}

pub async fn refresh_model_provider_catalog_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<RefreshModelProviderCatalogResponse, CommandErrorPayload> {
    let now = Utc::now();
    let models_api_json = fetch_openrouter_models_api_json().await?;
    let bytes = serde_json::to_vec(&models_api_json).map_err(|error| {
        runtime_operation_failed(format!(
            "provider catalog refresh serialization failed: {error}"
        ))
    })?;
    inventory_from_models_api_json(&bytes).map_err(|error| {
        runtime_operation_failed(format!("provider catalog refresh parse failed: {error}"))
    })?;
    runtime_state
        .provider_catalog_snapshot_store
        .save_record(&ProviderCatalogSnapshotRecord {
            openrouter_models_api_json: models_api_json,
            last_successful_refresh_at: now,
            last_attempt_at: now,
        })?;
    let (catalog, catalog_snapshot) = local_model_provider_catalog(runtime_state)?;
    Ok(RefreshModelProviderCatalogResponse {
        catalog,
        catalog_snapshot,
    })
}

async fn fetch_openrouter_models_api_json() -> Result<serde_json::Value, CommandErrorPayload> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| {
            runtime_operation_failed(format!("provider catalog client failed: {error}"))
        })?;
    let mut response = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("provider catalog fetch failed: {error}"))
        })?
        .error_for_status()
        .map_err(|error| {
            runtime_operation_failed(format!("provider catalog fetch failed: {error}"))
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OPENROUTER_MODELS_API_BYTES as u64)
    {
        return Err(runtime_operation_failed(
            "provider catalog response is too large".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        runtime_operation_failed(format!("provider catalog read failed: {error}"))
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OPENROUTER_MODELS_API_BYTES {
            return Err(runtime_operation_failed(
                "provider catalog response is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        runtime_operation_failed(format!("provider catalog parse failed: {error}"))
    })
}

fn slice_from_result<T>(result: Result<T, CommandErrorPayload>) -> ModelSettingsPageSlice<T> {
    match result {
        Ok(data) => ModelSettingsPageSlice::ready(data),
        Err(error) => ModelSettingsPageSlice::error(error.message),
    }
}

pub(crate) fn local_model_provider_catalog(
    runtime_state: &DesktopRuntimeState,
) -> Result<
    (
        ModelProviderCatalogResponse,
        ModelSettingsCatalogSnapshotPayload,
    ),
    CommandErrorPayload,
> {
    let mut catalog = list_model_provider_catalog_payload();
    let Some(snapshot) = runtime_state
        .provider_catalog_snapshot_store
        .load_record()?
    else {
        return Ok((
            catalog,
            ModelSettingsCatalogSnapshotPayload {
                source: "bundled",
                last_successful_refresh_at: None,
                last_attempt_at: None,
            },
        ));
    };

    let bytes = serde_json::to_vec(&snapshot.openrouter_models_api_json).map_err(|error| {
        runtime_operation_failed(format!(
            "provider catalog snapshot serialization failed: {error}"
        ))
    })?;
    let Ok(inventory) = inventory_from_models_api_json(&bytes) else {
        return Ok((
            catalog,
            ModelSettingsCatalogSnapshotPayload {
                source: "bundled",
                last_successful_refresh_at: None,
                last_attempt_at: Some(snapshot.last_attempt_at.to_rfc3339()),
            },
        ));
    };
    if let Some(openrouter) = catalog
        .providers
        .iter_mut()
        .find(|provider| provider.provider_id == "openrouter")
    {
        openrouter.models = runnable_inventory_models(&inventory)
            .into_iter()
            .map(model_descriptor_catalog_entry)
            .collect();
    }

    Ok((
        catalog,
        ModelSettingsCatalogSnapshotPayload {
            source: "snapshot",
            last_successful_refresh_at: Some(snapshot.last_successful_refresh_at.to_rfc3339()),
            last_attempt_at: Some(snapshot.last_attempt_at.to_rfc3339()),
        },
    ))
}

fn usage_summary_slice_from_rollup(
    runtime_state: &DesktopRuntimeState,
    now: DateTime<Utc>,
) -> ModelSettingsPageSlice<GetModelUsageSummaryResponse> {
    match load_or_create_usage_rollup(runtime_state, now) {
        Ok(record) if record.dirty => {
            ModelSettingsPageSlice::rebuilding("model usage summary is rebuilding")
        }
        Ok(record) => ModelSettingsPageSlice::ready(record.summary.into()),
        Err(error) => ModelSettingsPageSlice::error(error.message),
    }
}

fn load_or_create_usage_rollup(
    runtime_state: &DesktopRuntimeState,
    now: DateTime<Utc>,
) -> Result<ModelUsageRollupRecord, CommandErrorPayload> {
    let timezone = workspace_timezone_resolver();
    let Some(mut record) = runtime_state.model_usage_rollup_store.load_record()? else {
        let record = ModelUsageRollupRecord {
            schema_version: MODEL_USAGE_ROLLUP_SCHEMA_VERSION,
            dirty: false,
            summary: empty_usage_summary(now, &timezone),
        };
        runtime_state
            .model_usage_rollup_store
            .save_record(&record)?;
        return Ok(record);
    };

    if record.schema_version != MODEL_USAGE_ROLLUP_SCHEMA_VERSION {
        record = ModelUsageRollupRecord {
            schema_version: MODEL_USAGE_ROLLUP_SCHEMA_VERSION,
            dirty: true,
            summary: empty_usage_summary(now, &timezone),
        };
        runtime_state
            .model_usage_rollup_store
            .save_record(&record)?;
        return Ok(record);
    }

    if normalize_usage_windows(&mut record.summary, now, &timezone) {
        runtime_state
            .model_usage_rollup_store
            .save_record(&record)?;
    }

    Ok(record)
}

#[doc(hidden)]
pub fn project_usage_events_into_rollup_for_test(
    runtime_state: &DesktopRuntimeState,
    events: &[UsageAccumulatedEvent],
) -> Result<(), CommandErrorPayload> {
    project_usage_events_into_rollup(runtime_state, events, Utc::now())
}

pub(crate) fn project_usage_events_into_rollup(
    runtime_state: &DesktopRuntimeState,
    events: &[UsageAccumulatedEvent],
    now: DateTime<Utc>,
) -> Result<(), CommandErrorPayload> {
    project_usage_events_into_store(runtime_state.model_usage_rollup_store.as_ref(), events, now)
}

fn project_usage_events_into_store(
    store: &dyn ModelUsageRollupStore,
    events: &[UsageAccumulatedEvent],
    now: DateTime<Utc>,
) -> Result<(), CommandErrorPayload> {
    let timezone = workspace_timezone_resolver();
    let mut record = store
        .load_record()?
        .unwrap_or_else(|| ModelUsageRollupRecord {
            schema_version: MODEL_USAGE_ROLLUP_SCHEMA_VERSION,
            dirty: false,
            summary: empty_usage_summary(now, &timezone),
        });

    normalize_usage_windows(&mut record.summary, now, &timezone);
    record.dirty = true;
    store.save_record(&record)?;

    for event in events {
        add_usage_event_to_summary(&mut record.summary, event);
    }
    record.summary.generated_at = now;
    record.summary.timezone_id = timezone.timezone_id().map(str::to_owned);
    record.summary.timezone_offset_minutes = timezone.offset_minutes_at(now);
    record.dirty = false;
    store.save_record(&record)
}

fn empty_usage_summary(
    now: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> ModelUsageSummary {
    let today_start = timezone.local_day_start_utc(now);
    let month_start = timezone.local_month_start_utc(now);
    ModelUsageSummary {
        timezone_id: timezone.timezone_id().map(str::to_owned),
        timezone_offset_minutes: timezone.offset_minutes_at(now),
        today: empty_usage_window(ModelUsagePeriod::Today, Some(today_start), Some(now)),
        month_to_date: empty_usage_window(
            ModelUsagePeriod::MonthToDate,
            Some(month_start),
            Some(now),
        ),
        all_time: empty_usage_window(ModelUsagePeriod::AllTime, None, None),
        generated_at: now,
    }
}

fn empty_usage_window(
    period: ModelUsagePeriod,
    period_start: Option<DateTime<Utc>>,
    period_end: Option<DateTime<Utc>>,
) -> ModelUsageWindow {
    ModelUsageWindow {
        period,
        period_start,
        period_end,
        total: UsageSnapshot::default(),
        by_model: Vec::new(),
    }
}

fn normalize_usage_windows(
    summary: &mut ModelUsageSummary,
    now: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> bool {
    let today_start = timezone.local_day_start_utc(now);
    let month_start = timezone.local_month_start_utc(now);
    let mut changed = false;

    if summary.today.period_start != Some(today_start) {
        summary.today = empty_usage_window(ModelUsagePeriod::Today, Some(today_start), Some(now));
        changed = true;
    } else {
        summary.today.period_end = Some(now);
    }

    if summary.month_to_date.period_start != Some(month_start) {
        summary.month_to_date =
            empty_usage_window(ModelUsagePeriod::MonthToDate, Some(month_start), Some(now));
        changed = true;
    } else {
        summary.month_to_date.period_end = Some(now);
    }

    summary.generated_at = now;
    summary.timezone_id = timezone.timezone_id().map(str::to_owned);
    summary.timezone_offset_minutes = timezone.offset_minutes_at(now);
    changed
}

fn add_usage_event_to_summary(summary: &mut ModelUsageSummary, event: &UsageAccumulatedEvent) {
    if event.diagnostic || usage_snapshot_is_empty(&event.delta) {
        return;
    }

    let period_end = summary.generated_at;
    if summary
        .today
        .period_start
        .is_some_and(|start| event.at >= start && event.at <= period_end)
    {
        add_usage_event_to_window(&mut summary.today, event);
    }
    if summary
        .month_to_date
        .period_start
        .is_some_and(|start| event.at >= start && event.at <= period_end)
    {
        add_usage_event_to_window(&mut summary.month_to_date, event);
    }
    add_usage_event_to_window(&mut summary.all_time, event);
}

fn add_usage_event_to_window(window: &mut ModelUsageWindow, event: &UsageAccumulatedEvent) {
    merge_usage(&mut window.total, &event.delta);

    let Some(model_ref) = &event.model_ref else {
        return;
    };
    let mut buckets: BTreeMap<String, UsageBucketState> = window
        .by_model
        .drain(..)
        .map(|bucket| {
            (
                bucket.key,
                UsageBucketState {
                    provider_id: bucket.provider_id,
                    model_id: bucket.model_id,
                    usage: bucket.usage,
                    last_used_at: bucket.last_used_at,
                },
            )
        })
        .collect();
    let key = model_usage_key(model_ref);
    let bucket = buckets.entry(key).or_insert_with(|| UsageBucketState {
        provider_id: model_ref.provider_id.clone(),
        model_id: model_ref.model_id.clone(),
        usage: UsageSnapshot::default(),
        last_used_at: None,
    });
    merge_usage(&mut bucket.usage, &event.delta);
    bucket.last_used_at = Some(bucket.last_used_at.map_or(event.at, |at| at.max(event.at)));
    window.by_model = buckets
        .into_iter()
        .map(|(key, bucket)| ModelUsageBucket {
            key,
            provider_id: bucket.provider_id,
            model_id: bucket.model_id,
            usage: bucket.usage,
            last_used_at: bucket.last_used_at,
        })
        .collect();
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

pub(crate) type ProviderProbeFlights = Arc<
    tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<ProbeProviderConfigResponse>>>>,
>;

pub(crate) fn new_provider_probe_flights() -> ProviderProbeFlights {
    Arc::new(tokio::sync::Mutex::new(HashMap::new()))
}

pub(crate) type OfficialQuotaFlights = Arc<
    tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<RefreshOfficialQuotaResponse>>>>,
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
    let harness = runtime_state.harness().ok_or_else(|| {
        runtime_unavailable("Model usage summary requires an active harness runtime.")
    })?;
    let events =
        collect_persisted_usage_events(harness.event_store().as_ref(), TenantId::SINGLE).await?;
    let now = Utc::now();
    let timezone = workspace_timezone_resolver();
    let summary = summarize_model_usage(events.iter(), now, &timezone);
    Ok(summary.into())
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
    let account_usage_registry = Arc::clone(&runtime_state.account_usage_registry);
    let config_id_key = config_id.to_owned();

    let config_id_lookup = config_id_key.clone();

    run_official_quota_refresh_single_flight(&flights, &config_id_key, || async move {
        let record = settings_store
            .load_record()?
            .ok_or_else(|| invalid_payload("provider config was not found".to_owned()))?;
        let config = provider_config_by_id(&record, &config_id_lookup)?;
        let snapshot = fetch_official_quota(
            account_usage_registry.as_ref(),
            ProviderAccountUsageRequest {
                config_id: config.id.clone(),
                provider_id: config.provider_id.clone(),
                model_id: Some(config.model_id.clone()),
                api_key: config.api_key.clone(),
                official_quota_api_key: config.official_quota_api_key.clone(),
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
