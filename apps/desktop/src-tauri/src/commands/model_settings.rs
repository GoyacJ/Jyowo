use std::collections::HashMap;
use std::sync::Arc;

use super::contracts::ProviderProbeSnapshotPayload;
use super::error::{invalid_payload, CommandErrorPayload};
use super::providers::{build_provider_for_config, provider_config_by_id};
use super::{
    DesktopRuntimeState, ListProviderProbeSnapshotsResponse, ModelProtocol, ModelProvider,
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

pub(crate) type ProviderProbeFlights =
    Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<ProbeProviderConfigResponse>>>>>;

pub(crate) fn new_provider_probe_flights() -> ProviderProbeFlights {
    Arc::new(tokio::sync::Mutex::new(HashMap::new()))
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
