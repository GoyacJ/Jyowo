use std::sync::Arc;

use chrono::{TimeZone, Utc};
use harness_contracts::{OfficialQuotaScope, OfficialQuotaStatus};
use harness_model::{
    auth_required_snapshot, compute_expires_at, compute_is_stale, default_account_usage_registry,
    failed_snapshot, fetch_official_quota, unsupported_snapshot,
    AccountUsageError, DEFAULT_QUOTA_CACHE_TTL, ProviderAccountUsageRegistry,
    ProviderAccountUsageRequest,
};
use wiremock::matchers::{bearer_token, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_request(provider_id: &str, api_key: &str) -> ProviderAccountUsageRequest {
    ProviderAccountUsageRequest {
        config_id: "cfg-test".to_owned(),
        provider_id: provider_id.to_owned(),
        model_id: Some("model-a".to_owned()),
        api_key: api_key.to_owned(),
        base_url: None,
    }
}

#[test]
fn unsupported_provider_returns_unsupported_with_source_url() {
    let registry = ProviderAccountUsageRegistry::new();
    let request = sample_request("anthropic", "secret");
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
#[cfg(feature = "openrouter")]
async fn openrouter_auth_failure_returns_auth_required() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/key"))
        .and(bearer_token("bad-key"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("openrouter", "bad-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::AuthRequired);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
}

#[tokio::test]
#[cfg(feature = "openrouter")]
async fn openrouter_provider_failure_returns_failed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/key"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("openrouter", "good-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Failed);
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_some());
}

#[tokio::test]
#[cfg(feature = "openrouter")]
async fn openrouter_supported_response_maps_without_native_payload() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "usage": 1.25,
                "limit": 10.0,
                "limit_remaining": 8.75
            }
        })))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("openrouter", "good-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Supported);
    assert_eq!(snapshot.scope, OfficialQuotaScope::Account);
    assert_eq!(snapshot.quota_used, Some(1_250_000));
    assert_eq!(snapshot.quota_total, Some(10_000_000));
    assert_eq!(snapshot.quota_remaining, Some(8_750_000));
    assert!(!snapshot.source_url.is_empty());
    assert!(snapshot.safe_message.is_none());
}

#[tokio::test]
#[cfg(feature = "deepseek")]
async fn deepseek_supported_response_maps_account_balance() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "USD",
                "total_balance": "42.50",
                "granted_balance": "2.50",
                "topped_up_balance": "40.00"
            }]
        })))
        .mount(&server)
        .await;

    let registry = default_account_usage_registry();
    let mut request = sample_request("deepseek", "good-key");
    request.base_url = Some(server.uri());

    let snapshot = fetch_official_quota(&registry, request).await;
    assert_eq!(snapshot.status, OfficialQuotaStatus::Supported);
    assert_eq!(snapshot.scope, OfficialQuotaScope::Account);
    assert_eq!(snapshot.quota_remaining, Some(42_500_000));
    assert_eq!(snapshot.quota_total, Some(42_500_000));
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
    let auth = auth_required_snapshot(&request, "https://platform.openai.com/docs/api-reference/usage", "need admin key");
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
