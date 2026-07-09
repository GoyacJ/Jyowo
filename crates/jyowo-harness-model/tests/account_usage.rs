use std::sync::Arc;

use chrono::{TimeZone, Utc};
use harness_contracts::{OfficialQuotaScope, OfficialQuotaStatus};
use harness_model::{
    auth_required_snapshot, compute_expires_at, compute_is_stale, default_account_usage_registry,
    failed_snapshot, fetch_official_quota, unsupported_snapshot, AccountUsageError,
    ProviderAccountUsageRegistry, ProviderAccountUsageRequest, DEFAULT_QUOTA_CACHE_TTL,
};
#[cfg(any(
    feature = "anthropic",
    feature = "deepseek",
    feature = "km",
    feature = "openai",
    feature = "openrouter"
))]
use wiremock::matchers::{method, path_regex};
#[cfg(any(
    feature = "anthropic",
    feature = "deepseek",
    feature = "km",
    feature = "openai",
    feature = "openrouter"
))]
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_request(provider_id: &str, api_key: &str) -> ProviderAccountUsageRequest {
    ProviderAccountUsageRequest {
        config_id: "cfg-test".to_owned(),
        provider_id: provider_id.to_owned(),
        model_id: Some("model-a".to_owned()),
        api_key: api_key.to_owned(),
        official_quota_api_key: None,
        base_url: None,
    }
}

#[test]
fn unsupported_provider_returns_unsupported_with_source_url() {
    let registry = ProviderAccountUsageRegistry::new();
    let request = sample_request("gemini", "secret");
    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fetch_official_quota(&registry, request));

    assert_eq!(snapshot.status, OfficialQuotaStatus::Unsupported);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
}

#[test]
fn missing_api_key_returns_not_configured() {
    let registry = default_account_usage_registry();
    let request = sample_request("openrouter", "  ");
    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fetch_official_quota(&registry, request));

    assert_eq!(snapshot.status, OfficialQuotaStatus::NotConfigured);
    assert!(snapshot.source_url.is_empty());
}

#[test]
#[cfg(feature = "anthropic")]
fn anthropic_returns_auth_required_without_network() {
    let registry = default_account_usage_registry();
    let request = sample_request("anthropic", "sk-ant-test");
    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fetch_official_quota(&registry, request));

    assert_eq!(snapshot.status, OfficialQuotaStatus::AuthRequired);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
}

#[tokio::test]
#[cfg(feature = "anthropic")]
async fn anthropic_quota_rejects_non_official_base_url_before_sending_admin_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("anthropic", "inference-key");
    request.official_quota_api_key = Some("admin-key".to_owned());
    request.base_url = Some(format!("{}/v1", server.uri()));

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
    let requests = server.received_requests().await.unwrap();
    assert!(requests.is_empty());
}

#[test]
#[cfg(feature = "openai")]
fn openai_returns_auth_required_without_network() {
    let registry = default_account_usage_registry();
    let request = sample_request("openai", "sk-test");
    let snapshot = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fetch_official_quota(&registry, request));

    assert_eq!(snapshot.status, OfficialQuotaStatus::AuthRequired);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
}

#[tokio::test]
#[cfg(feature = "openai")]
async fn openai_quota_rejects_non_official_base_url_before_sending_admin_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("openai", "inference-key");
    request.official_quota_api_key = Some("admin-key".to_owned());
    request.base_url = Some(format!("{}/v1", server.uri()));

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
    let requests = server.received_requests().await.unwrap();
    assert!(requests.is_empty());
}

#[tokio::test]
#[cfg(feature = "openrouter")]
async fn openrouter_quota_rejects_non_official_base_url_before_sending_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("openrouter", "provider-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
    let requests = server.received_requests().await.unwrap();
    assert!(requests.is_empty());
}

#[tokio::test]
#[cfg(feature = "deepseek")]
async fn deepseek_quota_rejects_non_official_base_url_before_sending_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("deepseek", "provider-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
    let requests = server.received_requests().await.unwrap();
    assert!(requests.is_empty());
}

#[tokio::test]
#[cfg(feature = "km")]
async fn kimi_quota_rejects_non_official_base_url_before_sending_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("km", "provider-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
    let requests = server.received_requests().await.unwrap();
    assert!(requests.is_empty());
}

#[test]
fn staleness_computed_from_expires_at() {
    let fetched_at = Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap();
    let expires_at = compute_expires_at(fetched_at, DEFAULT_QUOTA_CACHE_TTL);
    assert!(compute_is_stale(
        fetched_at,
        expires_at,
        Utc.with_ymd_and_hms(2026, 6, 30, 0, 16, 0).unwrap()
    ));
    assert!(!compute_is_stale(
        fetched_at,
        expires_at,
        Utc.with_ymd_and_hms(2026, 6, 30, 0, 5, 0).unwrap()
    ));
}

#[test]
fn unsupported_snapshot_preserves_scope_and_messages() {
    let request = sample_request("gemini", "key");
    let snapshot = unsupported_snapshot(
        &request,
        "https://ai.google.dev/gemini-api/docs/rate-limits",
        "unsupported",
    );
    assert_eq!(snapshot.scope, OfficialQuotaScope::Account);
    assert_eq!(snapshot.status, OfficialQuotaStatus::Unsupported);
    assert!(snapshot.safe_message.is_some());
}

#[test]
fn auth_required_and_failed_snapshots_include_safe_message() {
    let request = sample_request("openai", "key");
    let auth = auth_required_snapshot(
        &request,
        "https://platform.openai.com/docs/api-reference/usage",
        "need admin key",
    );
    assert_eq!(auth.status, OfficialQuotaStatus::AuthRequired);
    assert!(auth.safe_message.is_some());

    let failed = failed_snapshot(
        &request,
        "https://platform.openai.com/docs/api-reference/usage",
        "provider unavailable",
    );
    assert_eq!(failed.status, OfficialQuotaStatus::Failed);
    assert!(failed.safe_message.is_some());
}

struct AlwaysFailClient;

#[async_trait::async_trait]
impl harness_model::ProviderAccountUsageClient for AlwaysFailClient {
    fn provider_id(&self) -> &str {
        "test-provider"
    }

    fn source_url(&self) -> &'static str {
        "https://example.com/quota"
    }

    fn cache_ttl(&self) -> std::time::Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        _request: ProviderAccountUsageRequest,
    ) -> Result<harness_contracts::OfficialQuotaSnapshot, AccountUsageError> {
        Err(AccountUsageError::Failed {
            safe_message: "adapter failed".to_owned(),
        })
    }
}

#[tokio::test]
async fn registry_client_failure_maps_to_failed_snapshot() {
    let mut registry = ProviderAccountUsageRegistry::new();
    registry.register(Arc::new(AlwaysFailClient));
    let request = sample_request("test-provider", "key");
    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert_eq!(snapshot.source_url, "https://example.com/quota");
}
