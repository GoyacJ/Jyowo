use super::{openai_descriptor_record, unique_workspace};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use harness_contracts::OfficialQuotaStatus;
use jyowo_desktop_shell::commands::{
    list_official_quota_snapshots_with_runtime_state, refresh_official_quota_with_runtime_state,
    DesktopProviderQuotaCacheStore, DesktopProviderSettingsStore, DesktopRuntimeState,
    ProviderConfigRecord, ProviderQuotaCacheStore, ProviderSettingsRecord, ProviderSettingsStore,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_openrouter_config(api_key: &str) -> ProviderConfigRecord {
    ProviderConfigRecord {
        api_key: api_key.to_owned(),
        protocol: harness_contracts::ModelProtocol::ChatCompletions,
        base_url: None,
        display_name: "OpenRouter Work".to_owned(),
        id: "openrouter-work".to_owned(),
        model_id: "openai/gpt-5.5".to_owned(),
        provider_id: "openrouter".to_owned(),
        model_descriptor: openai_descriptor_record("openai/gpt-5.5"),
    }
}

fn prepare_workspace(name: &str) -> std::path::PathBuf {
    let workspace = unique_workspace(name);
    std::fs::create_dir_all(&workspace).unwrap();
    workspace.canonicalize().unwrap()
}

#[tokio::test]
async fn official_quota_refresh_rejects_unknown_config_id() {
    let workspace = prepare_workspace("official-quota-unknown-config");
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openrouter-work".to_owned()),
            configs: vec![sample_openrouter_config("provider-test-token")],
        })
        .unwrap();
    let runtime = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();
    let error = refresh_official_quota_with_runtime_state("missing", &runtime)
        .await
        .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn official_quota_refresh_persists_unsupported_snapshot_for_catalog_provider() {
    let workspace = prepare_workspace("official-quota-unsupported");
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("anthropic-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: harness_contracts::ModelProtocol::Messages,
                base_url: None,
                display_name: "Anthropic Work".to_owned(),
                id: "anthropic-work".to_owned(),
                model_id: "claude-sonnet-4.6".to_owned(),
                provider_id: "anthropic".to_owned(),
                model_descriptor: openai_descriptor_record("claude-sonnet-4.6"),
            }],
        })
        .unwrap();
    let runtime = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();
    let response = refresh_official_quota_with_runtime_state("anthropic-work", &runtime)
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
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "usage": 2.0,
                "limit": 20.0,
                "limit_remaining": 18.0
            }
        })))
        .mount(&server)
        .await;

    let workspace = prepare_workspace("official-quota-persist");
    let mut config = sample_openrouter_config("provider-test-token");
    config.base_url = Some(server.uri());
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openrouter-work".to_owned()),
            configs: vec![config],
        })
        .unwrap();
    let runtime = DesktopRuntimeState::with_workspace_for_test(workspace.clone()).unwrap();

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
}

#[tokio::test]
async fn official_quota_refresh_is_single_flight_per_config() {
    let server = MockServer::start().await;
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);
    Mock::given(method("GET"))
        .and(path("/v1/key"))
        .respond_with(move |_: &wiremock::Request| {
            counter.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "usage": 1.0,
                    "limit": 10.0,
                    "limit_remaining": 9.0
                }
            }))
        })
        .mount(&server)
        .await;

    let workspace = prepare_workspace("official-quota-single-flight");
    let mut config = sample_openrouter_config("provider-test-token");
    config.base_url = Some(server.uri());
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openrouter-work".to_owned()),
            configs: vec![config],
        })
        .unwrap();
    let runtime = Arc::new(DesktopRuntimeState::with_workspace_for_test(workspace).unwrap());

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
    std::fs::write(
        runtime_dir.join("provider-quota-cache.json"),
        b"{not-json",
    )
    .unwrap();

    let store = DesktopProviderQuotaCacheStore::new(workspace);
    let record = store.load_record().unwrap();
    assert!(record.snapshots.is_empty());
    assert!(!runtime_dir.join("provider-quota-cache.json").exists());
}
