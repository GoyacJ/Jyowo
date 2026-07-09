use super::{
    openai_descriptor_record, provider_settings_store_for_workspace, unique_workspace,
    use_test_provider_settings_store,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{OfficialQuotaScope, OfficialQuotaSnapshot, OfficialQuotaStatus};
use harness_model::{
    compute_expires_at, AccountUsageError, ProviderAccountUsageClient,
    ProviderAccountUsageRegistry, ProviderAccountUsageRequest, DEFAULT_QUOTA_CACHE_TTL,
};
use jyowo_desktop_shell::commands::{
    list_official_quota_snapshots_with_runtime_state, refresh_official_quota_with_runtime_state,
    DesktopProviderQuotaCacheStore, DesktopProviderSettingsStore, DesktopRuntimeState,
    ProviderConfigRecord, ProviderQuotaCacheStore, ProviderSettingsRecord, ProviderSettingsStore,
};

struct FakeAccountUsageClient {
    call_count: Arc<AtomicUsize>,
    delay: Duration,
}

#[async_trait]
impl ProviderAccountUsageClient for FakeAccountUsageClient {
    fn provider_id(&self) -> &str {
        "openrouter"
    }

    fn source_url(&self) -> &'static str {
        "https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key"
    }

    fn cache_ttl(&self) -> Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        let fetched_at = Utc::now();
        Ok(OfficialQuotaSnapshot {
            config_id: request.config_id,
            provider_id: request.provider_id,
            model_id: request.model_id,
            scope: OfficialQuotaScope::Account,
            status: OfficialQuotaStatus::Supported,
            period_start: None,
            period_end: None,
            quota_used: Some(2_000_000),
            quota_total: Some(20_000_000),
            quota_remaining: Some(18_000_000),
            unit: Some("usd_micro".to_owned()),
            billing_label: None,
            source_url: self.source_url().to_owned(),
            fetched_at,
            expires_at: compute_expires_at(fetched_at, self.cache_ttl()),
            is_stale: false,
            safe_message: None,
        })
    }
}

fn fake_account_usage_registry(
    call_count: Arc<AtomicUsize>,
    delay: Duration,
) -> Arc<ProviderAccountUsageRegistry> {
    let mut registry = ProviderAccountUsageRegistry::new();
    registry.register(Arc::new(FakeAccountUsageClient { call_count, delay }));
    Arc::new(registry)
}

fn sample_openrouter_config(api_key: &str) -> ProviderConfigRecord {
    ProviderConfigRecord {
        api_key: api_key.to_owned(),
        protocol: harness_contracts::ModelProtocol::ChatCompletions,
        base_url: None,
        display_name: "OpenRouter Work".to_owned(),
        id: "openrouter-work".to_owned(),
        model_id: "openai/gpt-5.5".to_owned(),
        model_options: harness_contracts::ModelRequestOptions::default(),
        official_quota_api_key: None,
        provider_id: "openrouter".to_owned(),
        provider_defaults: None,
        model_descriptor: openai_descriptor_record("openai/gpt-5.5"),
    }
}

fn prepare_workspace(name: &str) -> std::path::PathBuf {
    let workspace = unique_workspace(name);
    std::fs::create_dir_all(&workspace).unwrap();
    workspace.canonicalize().unwrap()
}

#[test]
fn official_quota_status_payload_serializes_camel_case_for_react() {
    assert_eq!(
        serde_json::to_value(
            jyowo_desktop_shell::commands::OfficialQuotaStatusPayload::AuthRequired
        )
        .unwrap(),
        serde_json::json!("authRequired")
    );
    assert_eq!(
        serde_json::to_value(
            jyowo_desktop_shell::commands::OfficialQuotaStatusPayload::NotConfigured
        )
        .unwrap(),
        serde_json::json!("notConfigured")
    );
}

#[tokio::test]
async fn official_quota_refresh_rejects_unknown_config_id() {
    let workspace = prepare_workspace("official-quota-unknown-config");
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openrouter-work".to_owned()),
            configs: vec![sample_openrouter_config("provider-test-token")],
        })
        .unwrap();
    let mut runtime = DesktopRuntimeState::with_workspace_for_test(workspace.clone()).unwrap();
    use_test_provider_settings_store(&mut runtime, &workspace);
    let error = refresh_official_quota_with_runtime_state("missing", &runtime)
        .await
        .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn official_quota_refresh_persists_unsupported_snapshot_for_catalog_provider() {
    let workspace = prepare_workspace("official-quota-unsupported");
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("gemini-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: harness_contracts::ModelProtocol::Messages,
                base_url: None,
                display_name: "Gemini Work".to_owned(),
                id: "gemini-work".to_owned(),
                model_id: "gemini-2.5-pro".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "gemini".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gemini-2.5-pro"),
            }],
        })
        .unwrap();
    let mut runtime = DesktopRuntimeState::with_workspace_for_test(workspace.clone()).unwrap();
    use_test_provider_settings_store(&mut runtime, &workspace);
    let response = refresh_official_quota_with_runtime_state("gemini-work", &runtime)
        .await
        .unwrap();
    assert_eq!(
        response.snapshot.status,
        jyowo_desktop_shell::commands::OfficialQuotaStatusPayload::Unsupported
    );
    assert!(!response.snapshot.source_url.is_empty());
    assert!(response.snapshot.safe_message.is_some());
}

#[tokio::test]
async fn official_quota_refresh_persists_supported_snapshot() {
    let workspace = prepare_workspace("official-quota-persist");
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openrouter-work".to_owned()),
            configs: vec![sample_openrouter_config("provider-test-token")],
        })
        .unwrap();
    let call_count = Arc::new(AtomicUsize::new(0));
    let mut runtime = DesktopRuntimeState::with_workspace_and_account_usage_registry_for_test(
        workspace.clone(),
        fake_account_usage_registry(Arc::clone(&call_count), Duration::ZERO),
    )
    .unwrap();
    use_test_provider_settings_store(&mut runtime, &workspace);

    let response = refresh_official_quota_with_runtime_state("openrouter-work", &runtime)
        .await
        .unwrap();
    assert_eq!(
        response.snapshot.status,
        jyowo_desktop_shell::commands::OfficialQuotaStatusPayload::Supported
    );
    assert!(!response.snapshot.source_url.is_empty());
    assert!(!response.snapshot.is_stale);

    let listed = list_official_quota_snapshots_with_runtime_state(&runtime).unwrap();
    assert_eq!(listed.snapshots.len(), 1);
    assert_eq!(listed.snapshots[0].config_id, "openrouter-work");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn official_quota_refresh_is_single_flight_per_config() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let workspace = prepare_workspace("official-quota-single-flight");
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openrouter-work".to_owned()),
            configs: vec![sample_openrouter_config("provider-test-token")],
        })
        .unwrap();
    let mut runtime = DesktopRuntimeState::with_workspace_and_account_usage_registry_for_test(
        workspace.clone(),
        fake_account_usage_registry(Arc::clone(&call_count), Duration::from_millis(50)),
    )
    .unwrap();
    use_test_provider_settings_store(&mut runtime, &workspace);
    let runtime = Arc::new(runtime);

    let first = Arc::clone(&runtime);
    let second = Arc::clone(&runtime);
    let (left, right) = tokio::join!(
        refresh_official_quota_with_runtime_state("openrouter-work", &first),
        refresh_official_quota_with_runtime_state("openrouter-work", &second)
    );
    left.unwrap();
    right.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[test]
fn official_quota_cache_rejects_symlink_file() {
    let workspace = prepare_workspace("official-quota-symlink");
    let runtime_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let target = runtime_dir.join("provider-quota-cache-target.json");
    std::fs::write(&target, br#"{"snapshots":[]}"#).unwrap();
    std::os::unix::fs::symlink(&target, runtime_dir.join("provider-quota-cache.json")).unwrap();

    let store = DesktopProviderQuotaCacheStore::new(workspace);
    let error = store.load_record().unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
}

#[test]
fn official_quota_list_recomputes_staleness() {
    let workspace = prepare_workspace("official-quota-stale");
    let store = DesktopProviderQuotaCacheStore::new(workspace.clone());
    let fetched_at = chrono::Utc::now() - chrono::Duration::hours(2);
    let expires_at = fetched_at + chrono::Duration::minutes(15);
    store
        .upsert_snapshot(&harness_contracts::OfficialQuotaSnapshot {
            config_id: "openrouter-work".to_owned(),
            provider_id: "openrouter".to_owned(),
            model_id: None,
            scope: harness_contracts::OfficialQuotaScope::Account,
            status: OfficialQuotaStatus::Supported,
            period_start: None,
            period_end: None,
            quota_used: Some(1),
            quota_total: Some(10),
            quota_remaining: Some(9),
            unit: Some("usd_micro".to_owned()),
            billing_label: None,
            source_url: "https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key"
                .to_owned(),
            fetched_at,
            expires_at,
            is_stale: false,
            safe_message: None,
        })
        .unwrap();

    let runtime = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();
    let listed = list_official_quota_snapshots_with_runtime_state(&runtime).unwrap();
    assert_eq!(listed.snapshots.len(), 1);
    assert!(listed.snapshots[0].is_stale);
}

#[test]
fn official_quota_cache_recovers_from_invalid_json() {
    let workspace = prepare_workspace("official-quota-invalid-json");
    let runtime_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(runtime_dir.join("provider-quota-cache.json"), b"{not-json").unwrap();

    let store = DesktopProviderQuotaCacheStore::new(workspace);
    let record = store.load_record().unwrap();
    assert!(record.snapshots.is_empty());
    assert!(!runtime_dir.join("provider-quota-cache.json").exists());
}
