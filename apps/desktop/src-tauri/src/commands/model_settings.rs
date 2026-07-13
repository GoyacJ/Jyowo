use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    Event, ModelRef, ModelUsagePeriod, ModelUsageSummary, ModelUsageWindow, TaskEventHistoryPage,
    UsageAccumulatedEvent, UsageSnapshot,
};
use harness_model::{
    fetch_official_quota, provider_requires_api_key, with_staleness, ProviderAccountUsageRequest,
};
use harness_observability::{
    normalize_usage_activity, summarize_model_usage, IanaTimezoneResolver, LocalTimezoneResolver,
    WorkspaceTimezoneResolver,
};

use jyowo_harness_sdk::ext::{inventory_from_models_api_json, runnable_inventory_models};

use super::contracts::{
    ConversationModelCapabilityRecord, ModelCatalogEntry, ModelLifecyclePayload,
    ModelRuntimeStatusPayload, ModelSettingsCatalogSnapshotPayload, ModelSettingsPageResponse,
    ModelSettingsPageSlice, ModelUsageDayModelRecord, ModelUsageDayRecord, ModelUsageRollupRecord,
    ModelUsageRollupStore, ProviderCatalogSnapshotRecord, ProviderModelModalityRecord,
    ProviderProbeSnapshotPayload, RefreshModelProviderCatalogResponse,
    RefreshOfficialQuotaResponse,
};
use super::error::{invalid_payload, runtime_operation_failed, CommandErrorPayload};
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
use crate::daemon_client::DaemonClient;

pub(crate) const DEFAULT_PROBE_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MIN_PROBE_TIMEOUT_MS: u64 = 1_000;
pub(crate) const MAX_PROBE_TIMEOUT_MS: u64 = 60_000;

pub(crate) fn normalize_probe_timeout_ms(timeout_ms: Option<u64>) -> u64 {
    timeout_ms
        .unwrap_or(DEFAULT_PROBE_TIMEOUT_MS)
        .clamp(MIN_PROBE_TIMEOUT_MS, MAX_PROBE_TIMEOUT_MS)
}

const MODEL_USAGE_ROLLUP_SCHEMA_VERSION: u32 = 3;
const MODEL_USAGE_HISTORY_PAGE_LIMIT: u16 = 500;
const MODEL_USAGE_HISTORY_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OPENROUTER_MODELS_API_BYTES: usize = 2 * 1024 * 1024;
static MODEL_USAGE_ROLLUP_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

#[async_trait]
pub trait ModelUsageHistorySource: Send + Sync {
    async fn load_events(
        &self,
        after_global_offset: u64,
        limit: u16,
    ) -> Result<TaskEventHistoryPage, CommandErrorPayload>;
}

#[async_trait]
impl ModelUsageHistorySource for DaemonClient {
    async fn load_events(
        &self,
        after_global_offset: u64,
        limit: u16,
    ) -> Result<TaskEventHistoryPage, CommandErrorPayload> {
        tokio::time::timeout(
            MODEL_USAGE_HISTORY_REQUEST_TIMEOUT,
            DaemonClient::load_events(self, after_global_offset, limit),
        )
        .await
        .map_err(|_| runtime_operation_failed("model usage history request timed out".to_owned()))?
        .map_err(|error| {
            runtime_operation_failed(format!("model usage history request failed: {error}"))
        })
    }
}

pub async fn get_model_settings_page_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<ModelSettingsPageResponse, CommandErrorPayload> {
    get_model_settings_page_with_optional_history(runtime_state, None).await
}

pub async fn get_model_settings_page_with_history_source(
    runtime_state: &DesktopRuntimeState,
    source: &dyn ModelUsageHistorySource,
) -> Result<ModelSettingsPageResponse, CommandErrorPayload> {
    get_model_settings_page_with_optional_history(runtime_state, Some(source)).await
}

async fn get_model_settings_page_with_optional_history(
    runtime_state: &DesktopRuntimeState,
    source: Option<&dyn ModelUsageHistorySource>,
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
    let usage_summary = match source {
        Some(source) => usage_summary_slice_from_history(runtime_state, source, now).await,
        None => usage_summary_slice_from_rollup(runtime_state, now).await,
    };
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

async fn usage_summary_slice_from_history(
    runtime_state: &DesktopRuntimeState,
    source: &dyn ModelUsageHistorySource,
    now: DateTime<Utc>,
) -> ModelSettingsPageSlice<GetModelUsageSummaryResponse> {
    let _guard = MODEL_USAGE_ROLLUP_LOCK.lock().await;
    let timezone = workspace_timezone_resolver();
    match project_model_usage_with_source(
        runtime_state.model_usage_rollup_store.as_ref(),
        source,
        now,
        &timezone,
    )
    .await
    {
        Ok(record) if record.dirty || record.rebuilding => {
            ModelSettingsPageSlice::rebuilding("model usage summary is rebuilding")
        }
        Ok(record) => ModelSettingsPageSlice::ready(record.summary.into()),
        Err(error) => ModelSettingsPageSlice::error(error.message),
    }
}

pub async fn refresh_model_provider_catalog_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<RefreshModelProviderCatalogResponse, CommandErrorPayload> {
    let now = Utc::now();
    let models_api_json = fetch_openrouter_models_api_json().await?;
    let anthropic_models_api_json =
        fetch_anthropic_models_api_json_for_runtime_state(runtime_state).await?;
    let deepseek_models_api_json = fetch_deepseek_models_api_json().await.ok();
    let bytes = serde_json::to_vec(&models_api_json).map_err(|error| {
        runtime_operation_failed(format!(
            "provider catalog refresh serialization failed: {error}"
        ))
    })?;
    inventory_from_models_api_json(&bytes).map_err(|error| {
        runtime_operation_failed(format!("provider catalog refresh parse failed: {error}"))
    })?;
    if let Some(deepseek_models_api_json) = &deepseek_models_api_json {
        validate_deepseek_models_api_json(deepseek_models_api_json)?;
    }
    runtime_state
        .provider_catalog_snapshot_store
        .save_record(&ProviderCatalogSnapshotRecord {
            openrouter_models_api_json: models_api_json,
            anthropic_models_api_json,
            deepseek_models_api_json,
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

async fn fetch_deepseek_models_api_json() -> Result<serde_json::Value, CommandErrorPayload> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| {
            runtime_operation_failed(format!("provider catalog client failed: {error}"))
        })?;
    let mut response = client
        .get("https://api.deepseek.com/models")
        .send()
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("DeepSeek catalog fetch failed: {error}"))
        })?
        .error_for_status()
        .map_err(|error| {
            runtime_operation_failed(format!("DeepSeek catalog fetch failed: {error}"))
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OPENROUTER_MODELS_API_BYTES as u64)
    {
        return Err(runtime_operation_failed(
            "DeepSeek catalog response is too large".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        runtime_operation_failed(format!("DeepSeek catalog read failed: {error}"))
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OPENROUTER_MODELS_API_BYTES {
            return Err(runtime_operation_failed(
                "DeepSeek catalog response is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        runtime_operation_failed(format!("DeepSeek catalog parse failed: {error}"))
    })
}

fn validate_deepseek_models_api_json(value: &serde_json::Value) -> Result<(), CommandErrorPayload> {
    let Some(models) = value.get("data").and_then(serde_json::Value::as_array) else {
        return Err(runtime_operation_failed(
            "DeepSeek catalog response did not include data".to_owned(),
        ));
    };
    for model in models {
        if model
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            return Err(runtime_operation_failed(
                "DeepSeek catalog response included a model without id".to_owned(),
            ));
        }
    }
    Ok(())
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
        supported_protocols: existing
            .map(|model| model.supported_protocols.clone())
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| vec![ModelProtocol::Messages]),
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

async fn usage_summary_slice_from_rollup(
    runtime_state: &DesktopRuntimeState,
    now: DateTime<Utc>,
) -> ModelSettingsPageSlice<GetModelUsageSummaryResponse> {
    let _guard = MODEL_USAGE_ROLLUP_LOCK.lock().await;
    match load_or_create_usage_rollup(runtime_state, now).await {
        Ok(record) if record.dirty || record.rebuilding => {
            ModelSettingsPageSlice::rebuilding("model usage summary is rebuilding")
        }
        Ok(record) => ModelSettingsPageSlice::ready(record.summary.into()),
        Err(error) => ModelSettingsPageSlice::error(error.message),
    }
}

async fn load_or_create_usage_rollup(
    runtime_state: &DesktopRuntimeState,
    now: DateTime<Utc>,
) -> Result<ModelUsageRollupRecord, CommandErrorPayload> {
    let timezone = workspace_timezone_resolver();
    let Some(mut record) = runtime_state.model_usage_rollup_store.load_record()? else {
        let record = new_usage_rollup(now, &timezone, false);
        runtime_state
            .model_usage_rollup_store
            .save_record(&record)?;
        return Ok(record);
    };

    if record.schema_version != MODEL_USAGE_ROLLUP_SCHEMA_VERSION {
        return reset_usage_rollup(runtime_state, now);
    }

    if record.dirty {
        return Ok(record);
    }

    if usage_summary_timezone_changed(&record.summary, now, &timezone) {
        return reset_usage_rollup(runtime_state, now);
    }

    if normalize_usage_windows(&mut record.summary, now, &timezone) {
        runtime_state
            .model_usage_rollup_store
            .save_record(&record)?;
    }

    Ok(record)
}

fn reset_usage_rollup(
    runtime_state: &DesktopRuntimeState,
    now: DateTime<Utc>,
) -> Result<ModelUsageRollupRecord, CommandErrorPayload> {
    let timezone = workspace_timezone_resolver();
    let record = new_usage_rollup(now, &timezone, false);
    runtime_state
        .model_usage_rollup_store
        .save_record(&record)?;
    Ok(record)
}

fn new_usage_rollup(
    now: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
    rebuilding: bool,
) -> ModelUsageRollupRecord {
    ModelUsageRollupRecord {
        schema_version: MODEL_USAGE_ROLLUP_SCHEMA_VERSION,
        dirty: rebuilding,
        rebuilding,
        last_global_offset: 0,
        timezone_id: timezone.timezone_id().map(str::to_owned),
        timezone_offset_minutes: timezone.offset_minutes_at(now),
        day_buckets: BTreeMap::new(),
        summary: empty_usage_summary(now, timezone),
        pending_run_starts: BTreeMap::new(),
        longest_completed_duration_ms: 0,
    }
}

pub async fn project_model_usage_with_source(
    store: &dyn ModelUsageRollupStore,
    source: &dyn ModelUsageHistorySource,
    now: DateTime<Utc>,
    timezone: &(dyn WorkspaceTimezoneResolver + Sync),
) -> Result<ModelUsageRollupRecord, CommandErrorPayload> {
    let timezone_id = timezone.timezone_id().map(str::to_owned);
    let timezone_offset_minutes = timezone.offset_minutes_at(now);
    let record = match store.load_record()? {
        Some(record)
            if record.schema_version == MODEL_USAGE_ROLLUP_SCHEMA_VERSION
                && record.timezone_id == timezone_id
                && record.timezone_offset_minutes == timezone_offset_minutes =>
        {
            record
        }
        _ => new_usage_rollup(now, timezone, true),
    };

    let requested_after = record.last_global_offset;
    let page = source
        .load_events(requested_after, MODEL_USAGE_HISTORY_PAGE_LIMIT)
        .await?;
    validate_model_usage_history_page(&page, requested_after)?;

    let mut candidate = record;
    for envelope in &page.events {
        fold_model_usage_event(&mut candidate, envelope, timezone);
        candidate.last_global_offset = envelope.global_offset;
    }
    candidate.last_global_offset = page.next_after_global_offset;

    let caught_up = !page.has_more && candidate.last_global_offset >= page.latest_global_offset;
    if caught_up {
        candidate.summary = usage_summary_from_day_buckets(&candidate, now, timezone);
        candidate.dirty = false;
        candidate.rebuilding = false;
    } else {
        if candidate.last_global_offset <= requested_after {
            return Err(runtime_operation_failed(
                "model usage history pagination made no progress".to_owned(),
            ));
        }
        candidate.dirty = true;
        candidate.rebuilding = true;
    }
    store.save_record(&candidate)?;
    Ok(candidate)
}

fn validate_model_usage_history_page(
    page: &TaskEventHistoryPage,
    requested_after: u64,
) -> Result<(), CommandErrorPayload> {
    let invalid = |reason: &str| {
        runtime_operation_failed(format!(
            "model usage history returned invalid page: {reason}"
        ))
    };
    if page.after_global_offset != requested_after {
        return Err(invalid("response cursor does not match request"));
    }
    if page.latest_global_offset < requested_after {
        return Err(invalid("latest offset precedes request cursor"));
    }
    if page.next_after_global_offset > page.latest_global_offset {
        return Err(invalid("next cursor exceeds latest offset"));
    }
    if page.has_more != (page.next_after_global_offset < page.latest_global_offset) {
        return Err(invalid("hasMore does not match pagination offsets"));
    }
    if page.events.is_empty() {
        if page.next_after_global_offset != requested_after {
            return Err(invalid("empty page advances the cursor"));
        }
        if page.latest_global_offset != requested_after {
            return Err(invalid("empty page omits available events"));
        }
        return Ok(());
    }
    let Some(mut expected_offset) = requested_after.checked_add(1) else {
        return Err(invalid("event follows the maximum cursor"));
    };
    for (index, event) in page.events.iter().enumerate() {
        if event.global_offset != expected_offset {
            return Err(invalid("event offsets are not contiguous"));
        }
        if index + 1 < page.events.len() {
            let Some(next_expected_offset) = expected_offset.checked_add(1) else {
                return Err(invalid("event offsets exceed the maximum cursor"));
            };
            expected_offset = next_expected_offset;
        }
    }
    if page.next_after_global_offset
        != page
            .events
            .last()
            .expect("non-empty history page checked above")
            .global_offset
    {
        return Err(invalid("next cursor does not match the last event"));
    }
    Ok(())
}

fn fold_model_usage_event(
    record: &mut ModelUsageRollupRecord,
    envelope: &harness_contracts::TaskEventEnvelope,
    timezone: &dyn WorkspaceTimezoneResolver,
) {
    let Some(event) = envelope.payload.get("event") else {
        return;
    };
    let Ok(event) = serde_json::from_value::<Event>(event.clone()) else {
        return;
    };
    match event {
        Event::UsageAccumulated(event) if !event.diagnostic => {
            let date = timezone.local_datetime(event.at).date();
            let (provider_id, model_id) = event.model_ref.map_or((None, None), |model| {
                (Some(model.provider_id), Some(model.model_id))
            });
            let key = serde_json::to_string(&(provider_id.as_deref(), model_id.as_deref()))
                .expect("model usage identity must serialize");
            let bucket = record
                .day_buckets
                .entry(date)
                .or_insert_with(ModelUsageDayRecord::default)
                .by_model
                .entry(key)
                .or_insert_with(|| ModelUsageDayModelRecord {
                    provider_id,
                    model_id,
                    usage: UsageSnapshot::default(),
                    last_used_at: event.at,
                });
            merge_usage_snapshot(&mut bucket.usage, &event.delta);
            bucket.last_used_at = bucket.last_used_at.max(event.at);
        }
        Event::RunStarted(event) => {
            record
                .pending_run_starts
                .insert(event.run_id.to_string(), event.started_at);
        }
        Event::RunEnded(event) => {
            if let Some(started_at) = record.pending_run_starts.remove(&event.run_id.to_string()) {
                if let Ok(duration) = event.ended_at.signed_duration_since(started_at).to_std() {
                    let duration_ms = duration.as_millis().min(u128::from(u64::MAX)) as u64;
                    record.longest_completed_duration_ms =
                        record.longest_completed_duration_ms.max(duration_ms);
                }
            }
        }
        _ => {}
    }
}

fn usage_summary_from_day_buckets(
    record: &ModelUsageRollupRecord,
    now: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> ModelUsageSummary {
    let usage_events = record
        .day_buckets
        .values()
        .flat_map(|day| day.by_model.values())
        .map(|bucket| UsageAccumulatedEvent {
            session_id: harness_contracts::SessionId::new(),
            run_id: None,
            delta: bucket.usage.clone(),
            model_ref: match (&bucket.provider_id, &bucket.model_id) {
                (Some(provider_id), Some(model_id)) => Some(ModelRef {
                    provider_id: provider_id.clone(),
                    model_id: model_id.clone(),
                }),
                _ => None,
            },
            pricing_snapshot_id: None,
            at: bucket.last_used_at,
            diagnostic: false,
        })
        .collect::<Vec<_>>();
    let mut summary = summarize_model_usage(usage_events.iter(), now, timezone);
    summary.activity.longest_task_duration_ms = record.longest_completed_duration_ms;
    summary
}

fn merge_usage_snapshot(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
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
        activity: summarize_model_usage(
            std::iter::empty::<&UsageAccumulatedEvent>(),
            now,
            timezone,
        )
        .activity,
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

fn usage_summary_timezone_changed(
    summary: &ModelUsageSummary,
    now: DateTime<Utc>,
    timezone: &dyn WorkspaceTimezoneResolver,
) -> bool {
    summary.timezone_id.as_deref() != timezone.timezone_id()
        || summary.timezone_offset_minutes != timezone.offset_minutes_at(now)
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
    changed |= normalize_usage_activity(summary, now, timezone);
    changed
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

pub async fn get_model_usage_summary_with_runtime_state(
    runtime_state: &DesktopRuntimeState,
) -> Result<GetModelUsageSummaryResponse, CommandErrorPayload> {
    let _guard = MODEL_USAGE_ROLLUP_LOCK.lock().await;
    let record = load_or_create_usage_rollup(runtime_state, Utc::now()).await?;
    if record.dirty || record.rebuilding {
        return Err(runtime_operation_failed(
            "model usage summary is rebuilding".to_owned(),
        ));
    }
    Ok(record.summary.into())
}

pub async fn get_model_usage_summary_with_history_source(
    runtime_state: &DesktopRuntimeState,
    source: &dyn ModelUsageHistorySource,
) -> Result<GetModelUsageSummaryResponse, CommandErrorPayload> {
    get_model_usage_summary_with_history_store(
        runtime_state.model_usage_rollup_store.as_ref(),
        source,
    )
    .await
}

pub(crate) async fn get_model_usage_summary_with_history_store(
    store: &dyn ModelUsageRollupStore,
    source: &dyn ModelUsageHistorySource,
) -> Result<GetModelUsageSummaryResponse, CommandErrorPayload> {
    let _guard = MODEL_USAGE_ROLLUP_LOCK.lock().await;
    let now = Utc::now();
    let timezone = workspace_timezone_resolver();
    let record = project_model_usage_with_source(store, source, now, &timezone).await?;
    if record.dirty || record.rebuilding {
        return Err(runtime_operation_failed(
            "model usage summary is rebuilding".to_owned(),
        ));
    }
    Ok(record.summary.into())
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
