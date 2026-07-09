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
    ConversationModelCapabilityRecord, ModelCatalogEntry, ModelLifecyclePayload,
    ModelRuntimeStatusPayload, ModelSettingsCatalogSnapshotPayload, ModelSettingsPageResponse,
    ModelSettingsPageSlice, ModelUsageRollupRecord, ModelUsageRollupStore,
    ProviderCatalogSnapshotRecord, ProviderModelModalityRecord, ProviderProbeSnapshotPayload,
    RefreshModelProviderCatalogResponse, RefreshOfficialQuotaResponse,
};
use super::error::{
    invalid_payload, runtime_operation_failed, runtime_unavailable, CommandErrorPayload,
};
use super::providers::{
    build_provider_for_config, desktop_provider_service_adapter_availability,
    list_model_provider_catalog_payload, list_provider_capability_route_options_from_inputs,
    list_provider_capability_routes_with_store, list_provider_settings_with_store,
    model_descriptor_catalog_entry, provider_config_by_id, provider_requires_api_key,
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
    let anthropic_models_api_json =
        fetch_anthropic_models_api_json_for_runtime_state(runtime_state).await?;
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
            anthropic_models_api_json,
            last_successful_refresh_at: now,
            last_attempt_at: now,
        })?;
    let (catalog, catalog_snapshot) = local_model_provider_catalog(runtime_state)?;
    Ok(RefreshModelProviderCatalogResponse {
        catalog,
        catalog_snapshot,
    })
}

async fn fetch_anthropic_models_api_json_for_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<Option<serde_json::Value>, CommandErrorPayload> {
    let Some(record) = runtime_state.provider_settings_store.load_record()? else {
        return Ok(None);
    };
    let Some(config) = record
        .configs
        .iter()
        .find(|config| config.provider_id == "anthropic" && !config.api_key.trim().is_empty())
    else {
        return Ok(None);
    };
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com");
    match fetch_anthropic_models_api_json(base_url, &config.api_key).await {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            log::warn!("Anthropic model catalog refresh skipped: {}", error.message);
            Ok(None)
        }
    }
}

async fn fetch_anthropic_models_api_json(
    base_url: &str,
    api_key: &str,
) -> Result<serde_json::Value, CommandErrorPayload> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| {
            runtime_operation_failed(format!("Anthropic model catalog client failed: {error}"))
        })?;
    let mut response = client
        .get(format!("{}/v1/models", base_url.trim_end_matches('/')))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("Anthropic model catalog fetch failed: {error}"))
        })?
        .error_for_status()
        .map_err(|error| {
            runtime_operation_failed(format!("Anthropic model catalog fetch failed: {error}"))
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OPENROUTER_MODELS_API_BYTES as u64)
    {
        return Err(runtime_operation_failed(
            "Anthropic model catalog response is too large".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        runtime_operation_failed(format!("Anthropic model catalog read failed: {error}"))
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OPENROUTER_MODELS_API_BYTES {
            return Err(runtime_operation_failed(
                "Anthropic model catalog response is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        runtime_operation_failed(format!("Anthropic model catalog parse failed: {error}"))
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
    if let Some(anthropic_models_api_json) = snapshot.anthropic_models_api_json.as_ref() {
        merge_anthropic_models_api_catalog(&mut catalog, anthropic_models_api_json);
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

fn merge_anthropic_models_api_catalog(
    catalog: &mut ModelProviderCatalogResponse,
    models_api_json: &serde_json::Value,
) {
    let Some(anthropic) = catalog
        .providers
        .iter_mut()
        .find(|provider| provider.provider_id == "anthropic")
    else {
        return;
    };
    let Some(models) = models_api_json
        .get("data")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    let default_supported_parameters = anthropic
        .models
        .first()
        .map(|model| model.supported_parameters.clone())
        .unwrap_or_default();

    for api_model in models {
        let Some(model_id) = api_model
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let existing_index = anthropic
            .models
            .iter()
            .position(|model| model.model_id == model_id);
        let existing = existing_index.and_then(|index| anthropic.models.get(index));
        let Some(entry) =
            anthropic_models_api_catalog_entry(api_model, existing, &default_supported_parameters)
        else {
            continue;
        };
        if let Some(index) = existing_index {
            anthropic.models[index] = entry;
        } else {
            anthropic.models.push(entry);
        }
    }
}

fn anthropic_models_api_catalog_entry(
    api_model: &serde_json::Value,
    existing: Option<&ModelCatalogEntry>,
    default_supported_parameters: &[String],
) -> Option<ModelCatalogEntry> {
    let model_id = api_model
        .get("id")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_owned();
    let display_name = api_model
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            api_model
                .get("displayName")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| existing.map(|model| model.display_name.clone()))
        .unwrap_or_else(|| model_id.clone());
    let context_window = u32_field(api_model, &["max_input_tokens", "context_window"])
        .or_else(|| existing.map(|model| model.context_window))
        .unwrap_or(200_000);
    let max_output_tokens = u32_field(api_model, &["max_tokens", "max_output_tokens"])
        .or_else(|| existing.map(|model| model.max_output_tokens))
        .unwrap_or(64_000);
    let conversation_capability = anthropic_models_api_conversation_capability(
        api_model,
        existing,
        context_window,
        max_output_tokens,
    );

    Some(ModelCatalogEntry {
        protocol: existing
            .map(|model| model.protocol)
            .unwrap_or(ModelProtocol::Messages),
        supported_parameters: existing
            .map(|model| model.supported_parameters.clone())
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| default_supported_parameters.to_vec()),
        provider_capability_metadata: Some(anthropic_models_api_metadata(api_model, existing)),
        conversation_capability,
        context_window,
        display_name,
        lifecycle: existing
            .map(|model| model.lifecycle.clone())
            .unwrap_or(ModelLifecyclePayload {
                kind: "stable",
                retirement_date: None,
            }),
        max_output_tokens,
        model_id,
        runtime_status: ModelRuntimeStatusPayload {
            kind: "runnable",
            reason: None,
        },
    })
}

fn anthropic_models_api_conversation_capability(
    api_model: &serde_json::Value,
    existing: Option<&ModelCatalogEntry>,
    context_window: u32,
    max_output_tokens: u32,
) -> ConversationModelCapabilityRecord {
    let existing_capability = existing.map(|model| &model.conversation_capability);
    let capabilities = api_model.get("capabilities");
    let mut input_modalities = existing_capability
        .map(|capability| capability.input_modalities.clone())
        .unwrap_or_else(|| vec![ProviderModelModalityRecord::Text]);
    if capability_bool(capabilities, &["image_input"]).unwrap_or(false)
        && !input_modalities.contains(&ProviderModelModalityRecord::Image)
    {
        input_modalities.push(ProviderModelModalityRecord::Image);
    }
    if (capability_bool(capabilities, &["pdf_input", "document_input", "files_api"])
        .unwrap_or(false))
        && !input_modalities.contains(&ProviderModelModalityRecord::File)
    {
        input_modalities.push(ProviderModelModalityRecord::File);
    }

    ConversationModelCapabilityRecord {
        input_modalities,
        output_modalities: existing_capability
            .map(|capability| capability.output_modalities.clone())
            .unwrap_or_else(|| vec![ProviderModelModalityRecord::Text]),
        context_window,
        max_output_tokens,
        streaming: existing_capability
            .map(|capability| capability.streaming)
            .unwrap_or(true),
        tool_calling: capability_bool(capabilities, &["tool_use", "tools"])
            .or_else(|| existing_capability.map(|capability| capability.tool_calling))
            .unwrap_or(true),
        reasoning: capability_bool(
            capabilities,
            &["thinking", "extended_thinking", "reasoning"],
        )
        .or_else(|| {
            capability_string_array(capabilities, &["effort_levels", "effortLevels"])
                .map(|values| !values.is_empty())
        })
        .or_else(|| existing_capability.map(|capability| capability.reasoning))
        .unwrap_or(false),
        prompt_cache: capability_bool(capabilities, &["prompt_caching", "prompt_cache"])
            .or_else(|| existing_capability.map(|capability| capability.prompt_cache))
            .unwrap_or(true),
        structured_output: capability_bool(
            capabilities,
            &["structured_outputs", "structured_output"],
        )
        .or_else(|| existing_capability.map(|capability| capability.structured_output))
        .unwrap_or(false),
    }
}

fn anthropic_models_api_metadata(
    api_model: &serde_json::Value,
    existing: Option<&ModelCatalogEntry>,
) -> serde_json::Value {
    let mut metadata = existing
        .and_then(|model| model.provider_capability_metadata.as_ref())
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    metadata.insert(
        "provider".to_owned(),
        serde_json::Value::String("anthropic".to_owned()),
    );
    let capabilities = api_model.get("capabilities");
    if let Some(capabilities) = capabilities {
        metadata.insert("rawCapabilities".to_owned(), capabilities.clone());
    }
    metadata.insert(
        "modelsApi".to_owned(),
        serde_json::json!({
            "id": api_model.get("id").cloned().unwrap_or(serde_json::Value::Null),
            "type": api_model.get("type").cloned().unwrap_or(serde_json::Value::Null),
            "createdAt": api_model.get("created_at").cloned().unwrap_or(serde_json::Value::Null),
        }),
    );
    set_metadata_bool(
        &mut metadata,
        "supportsImageInput",
        capabilities,
        &["image_input"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsPdfInput",
        capabilities,
        &["pdf_input"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsFilesApi",
        capabilities,
        &["files_api", "pdf_input", "document_input"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsBatches",
        capabilities,
        &["batch", "batches"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsContextManagement",
        capabilities,
        &["context_management"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsCodeExecution",
        capabilities,
        &["code_execution"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsCitations",
        capabilities,
        &["citations"],
    );
    set_metadata_bool(
        &mut metadata,
        "supportsStructuredOutputs",
        capabilities,
        &["structured_outputs", "structured_output"],
    );
    if let Some(values) = capability_string_array(capabilities, &["effort_levels", "effortLevels"])
    {
        metadata.insert("effortLevels".to_owned(), serde_json::json!(values));
    }
    if let Some(values) =
        capability_string_array(capabilities, &["thinking_types", "thinkingTypes"])
    {
        metadata.insert("thinkingModes".to_owned(), serde_json::json!(values));
    }
    serde_json::Value::Object(metadata)
}

fn set_metadata_bool(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    capabilities: Option<&serde_json::Value>,
    names: &[&str],
) {
    if let Some(value) = capability_bool(capabilities, names) {
        metadata.insert(key.to_owned(), serde_json::Value::Bool(value));
    }
}

fn u32_field(value: &serde_json::Value, names: &[&str]) -> Option<u32> {
    names.iter().find_map(|name| {
        let raw = value.get(*name)?;
        let number = raw
            .as_u64()
            .or_else(|| raw.as_str().and_then(|value| value.parse::<u64>().ok()))?;
        u32::try_from(number).ok()
    })
}

fn capability_bool(capabilities: Option<&serde_json::Value>, names: &[&str]) -> Option<bool> {
    let capabilities = capabilities?;
    names
        .iter()
        .find_map(|name| capability_path_value(capabilities, name))
        .and_then(|value| {
            value.as_bool().or_else(|| {
                value
                    .as_str()
                    .and_then(|value| match value.to_ascii_lowercase().as_str() {
                        "true" | "supported" | "enabled" => Some(true),
                        "false" | "unsupported" | "disabled" => Some(false),
                        _ => None,
                    })
            })
        })
}

fn capability_string_array(
    capabilities: Option<&serde_json::Value>,
    names: &[&str],
) -> Option<Vec<String>> {
    let capabilities = capabilities?;
    names
        .iter()
        .find_map(|name| capability_path_value(capabilities, name))
        .and_then(|value| {
            value.as_array().map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
        })
}

fn capability_path_value<'a>(
    capabilities: &'a serde_json::Value,
    name: &str,
) -> Option<&'a serde_json::Value> {
    let value = capabilities.get(name)?;
    if let Some(enabled) = value.get("enabled") {
        return Some(enabled);
    }
    if let Some(values) = value.get("values") {
        return Some(values);
    }
    if let Some(types) = value.get("types") {
        return Some(types);
    }
    Some(value)
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
        if provider_requires_api_key(&config.provider_id) && config.api_key.trim().is_empty() {
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
    if provider_requires_api_key(&config.provider_id) && config.api_key.trim().is_empty() {
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
