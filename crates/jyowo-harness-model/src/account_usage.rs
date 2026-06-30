//! Official provider account quota adapters and registry.
//!
//! SPEC: docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    OfficialQuotaScope, OfficialQuotaSnapshot, OfficialQuotaStatus,
};
use serde::Deserialize;

/// Default cache TTL when a provider response does not include expiry metadata.
pub const DEFAULT_QUOTA_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

const OPENROUTER_KEY_SOURCE: &str =
    "https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key";
const DEEPSEEK_BALANCE_SOURCE: &str = "https://api-docs.deepseek.com/api/get-user-balance";
const OPENAI_USAGE_SOURCE: &str = "https://platform.openai.com/docs/api-reference/usage";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountUsageRequest {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: Option<String>,
    pub api_key: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountUsageError {
    AuthRequired { safe_message: String },
    Failed { safe_message: String },
}

impl AccountUsageError {
    pub fn safe_message(&self) -> &str {
        match self {
            Self::AuthRequired { safe_message } | Self::Failed { safe_message } => safe_message,
        }
    }
}

#[async_trait]
pub trait ProviderAccountUsageClient: Send + Sync {
    fn provider_id(&self) -> &str;
    fn source_url(&self) -> &'static str;
    fn cache_ttl(&self) -> Duration;
    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError>;
}

pub struct ProviderAccountUsageRegistry {
    clients: HashMap<String, Arc<dyn ProviderAccountUsageClient>>,
}

impl ProviderAccountUsageRegistry {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    pub fn register(&mut self, client: Arc<dyn ProviderAccountUsageClient>) {
        self.clients
            .insert(client.provider_id().to_owned(), client);
    }

    pub fn get(&self, provider_id: &str) -> Option<Arc<dyn ProviderAccountUsageClient>> {
        self.clients.get(provider_id).cloned()
    }
}

impl Default for ProviderAccountUsageRegistry {
    fn default() -> Self {
        default_account_usage_registry()
    }
}

#[must_use]
pub fn default_account_usage_registry() -> ProviderAccountUsageRegistry {
    let mut registry = ProviderAccountUsageRegistry::new();
    #[cfg(feature = "openrouter")]
    registry.register(Arc::new(OpenRouterAccountUsageClient));
    #[cfg(feature = "deepseek")]
    registry.register(Arc::new(DeepSeekAccountUsageClient));
    registry.register(Arc::new(OpenAiAdminRequiredClient));
    registry.register(Arc::new(CodexAdminRequiredClient));
    registry
}

#[must_use]
pub fn compute_is_stale(
    fetched_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    now >= expires_at || now < fetched_at
}

#[must_use]
pub fn compute_expires_at(fetched_at: DateTime<Utc>, ttl: Duration) -> DateTime<Utc> {
    fetched_at + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::hours(1))
}

#[must_use]
pub fn with_staleness(mut snapshot: OfficialQuotaSnapshot, now: DateTime<Utc>) -> OfficialQuotaSnapshot {
    snapshot.is_stale = compute_is_stale(snapshot.fetched_at, snapshot.expires_at, now);
    snapshot
}

#[must_use]
pub fn not_configured_snapshot(request: &ProviderAccountUsageRequest) -> OfficialQuotaSnapshot {
    let fetched_at = Utc::now();
    OfficialQuotaSnapshot {
        config_id: request.config_id.clone(),
        provider_id: request.provider_id.clone(),
        model_id: request.model_id.clone(),
        scope: OfficialQuotaScope::Account,
        status: OfficialQuotaStatus::NotConfigured,
        period_start: None,
        period_end: None,
        quota_used: None,
        quota_total: None,
        quota_remaining: None,
        unit: None,
        billing_label: None,
        source_url: String::new(),
        fetched_at,
        expires_at: compute_expires_at(fetched_at, DEFAULT_QUOTA_CACHE_TTL),
        is_stale: false,
        safe_message: None,
    }
}

#[must_use]
pub fn unsupported_snapshot(
    request: &ProviderAccountUsageRequest,
    source_url: &'static str,
    reason: &'static str,
) -> OfficialQuotaSnapshot {
    let fetched_at = Utc::now();
    OfficialQuotaSnapshot {
        config_id: request.config_id.clone(),
        provider_id: request.provider_id.clone(),
        model_id: request.model_id.clone(),
        scope: OfficialQuotaScope::Account,
        status: OfficialQuotaStatus::Unsupported,
        period_start: None,
        period_end: None,
        quota_used: None,
        quota_total: None,
        quota_remaining: None,
        unit: None,
        billing_label: None,
        source_url: source_url.to_owned(),
        fetched_at,
        expires_at: compute_expires_at(fetched_at, DEFAULT_QUOTA_CACHE_TTL),
        is_stale: false,
        safe_message: Some(reason.to_owned()),
    }
}

#[must_use]
pub fn auth_required_snapshot(
    request: &ProviderAccountUsageRequest,
    source_url: &'static str,
    safe_message: impl Into<String>,
) -> OfficialQuotaSnapshot {
    let fetched_at = Utc::now();
    OfficialQuotaSnapshot {
        config_id: request.config_id.clone(),
        provider_id: request.provider_id.clone(),
        model_id: request.model_id.clone(),
        scope: OfficialQuotaScope::Account,
        status: OfficialQuotaStatus::AuthRequired,
        period_start: None,
        period_end: None,
        quota_used: None,
        quota_total: None,
        quota_remaining: None,
        unit: None,
        billing_label: None,
        source_url: source_url.to_owned(),
        fetched_at,
        expires_at: compute_expires_at(fetched_at, DEFAULT_QUOTA_CACHE_TTL),
        is_stale: false,
        safe_message: Some(safe_message.into()),
    }
}

#[must_use]
pub fn failed_snapshot(
    request: &ProviderAccountUsageRequest,
    source_url: &'static str,
    safe_message: impl Into<String>,
) -> OfficialQuotaSnapshot {
    let fetched_at = Utc::now();
    OfficialQuotaSnapshot {
        config_id: request.config_id.clone(),
        provider_id: request.provider_id.clone(),
        model_id: request.model_id.clone(),
        scope: OfficialQuotaScope::Account,
        status: OfficialQuotaStatus::Failed,
        period_start: None,
        period_end: None,
        quota_used: None,
        quota_total: None,
        quota_remaining: None,
        unit: None,
        billing_label: None,
        source_url: source_url.to_owned(),
        fetched_at,
        expires_at: compute_expires_at(fetched_at, DEFAULT_QUOTA_CACHE_TTL),
        is_stale: false,
        safe_message: Some(safe_message.into()),
    }
}

#[must_use]
pub fn unsupported_reason(provider_id: &str) -> (&'static str, &'static str) {
    match provider_id {
        "anthropic" => (
            "https://docs.anthropic.com/en/api/getting-started",
            "Anthropic does not expose an official account balance API for the stored API key.",
        ),
        "doubao" => (
            "https://www.volcengine.com/docs/82379/1494384",
            "Doubao does not expose an official account balance API for the stored API key.",
        ),
        "gemini" => (
            "https://ai.google.dev/gemini-api/docs/rate-limits",
            "Gemini does not expose an official account balance API for the stored API key.",
        ),
        "km" => (
            "https://platform.moonshot.ai/docs",
            "Kimi does not expose an official account balance API for the stored API key.",
        ),
        "local-llama" => (
            "https://ollama.com/library",
            "Local Llama has no account quota API.",
        ),
        "minimax" => (
            "https://platform.minimax.io/docs/faq/about-account",
            "MiniMax does not expose an official account balance API for the stored API key.",
        ),
        "qwen" => (
            "https://help.aliyun.com/zh/model-studio/models",
            "DashScope does not expose an official account balance API for the stored API key.",
        ),
        "zhipu" => (
            "https://docs.bigmodel.cn/api-reference/模型-api/对话补全",
            "Zhipu does not expose an official account balance API for the stored API key.",
        ),
        _ => (
            "https://jyowo.local/provider-catalog",
            "This provider does not expose an official account quota API.",
        ),
    }
}

pub async fn fetch_official_quota(
    registry: &ProviderAccountUsageRegistry,
    request: ProviderAccountUsageRequest,
) -> OfficialQuotaSnapshot {
    if request.api_key.trim().is_empty() {
        return not_configured_snapshot(&request);
    }

    let Some(client) = registry.get(&request.provider_id) else {
        let (source_url, reason) = unsupported_reason(&request.provider_id);
        return unsupported_snapshot(&request, source_url, reason);
    };

    let source_url = client.source_url();
    match client.fetch_quota(request.clone()).await {
        Ok(snapshot) => snapshot,
        Err(AccountUsageError::AuthRequired { safe_message }) => {
            auth_required_snapshot(&request, source_url, safe_message)
        }
        Err(AccountUsageError::Failed { safe_message }) => {
            failed_snapshot(&request, source_url, safe_message)
        }
    }
}

#[must_use]
fn decimal_to_micros(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1_000_000.0).round() as u64
}

#[must_use]
fn decimal_string_to_micros(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().map(decimal_to_micros)
}

fn supported_snapshot(
    request: &ProviderAccountUsageRequest,
    source_url: &'static str,
    ttl: Duration,
    quota_used: Option<u64>,
    quota_total: Option<u64>,
    quota_remaining: Option<u64>,
    unit: &str,
    billing_label: Option<&str>,
) -> OfficialQuotaSnapshot {
    let fetched_at = Utc::now();
    OfficialQuotaSnapshot {
        config_id: request.config_id.clone(),
        provider_id: request.provider_id.clone(),
        model_id: request.model_id.clone(),
        scope: OfficialQuotaScope::Account,
        status: OfficialQuotaStatus::Supported,
        period_start: None,
        period_end: None,
        quota_used,
        quota_total,
        quota_remaining,
        unit: Some(unit.to_owned()),
        billing_label: billing_label.map(str::to_owned),
        source_url: source_url.to_owned(),
        fetched_at,
        expires_at: compute_expires_at(fetched_at, ttl),
        is_stale: false,
        safe_message: None,
    }
}

struct OpenAiAdminRequiredClient;

#[async_trait]
impl ProviderAccountUsageClient for OpenAiAdminRequiredClient {
    fn provider_id(&self) -> &str {
        "openai"
    }

    fn source_url(&self) -> &'static str {
        OPENAI_USAGE_SOURCE
    }

    fn cache_ttl(&self) -> Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        _request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        Err(AccountUsageError::AuthRequired {
            safe_message: "OpenAI organization usage requires an admin API key that is not configured in provider settings.".to_owned(),
        })
    }
}

struct CodexAdminRequiredClient;

#[async_trait]
impl ProviderAccountUsageClient for CodexAdminRequiredClient {
    fn provider_id(&self) -> &str {
        "codex"
    }

    fn source_url(&self) -> &'static str {
        OPENAI_USAGE_SOURCE
    }

    fn cache_ttl(&self) -> Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        let _ = request;
        Err(AccountUsageError::AuthRequired {
            safe_message: "OpenAI organization usage requires an admin API key that is not configured in provider settings.".to_owned(),
        })
    }
}

#[cfg(feature = "openrouter")]
struct OpenRouterAccountUsageClient;

#[cfg(feature = "openrouter")]
#[async_trait]
impl ProviderAccountUsageClient for OpenRouterAccountUsageClient {
    fn provider_id(&self) -> &str {
        "openrouter"
    }

    fn source_url(&self) -> &'static str {
        OPENROUTER_KEY_SOURCE
    }

    fn cache_ttl(&self) -> Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        let base_url = request
            .base_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("https://openrouter.ai/api");
        let url = format!("{}/v1/key", base_url.trim_end_matches('/'));

        let response = reqwest::Client::new()
            .get(url)
            .bearer_auth(&request.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| AccountUsageError::Failed {
                safe_message: "OpenRouter quota request failed due to a network error.".to_owned(),
            })?;

        let status = response.status();
        if status.as_u16() == 401 {
            return Err(AccountUsageError::AuthRequired {
                safe_message: "OpenRouter rejected the configured API key.".to_owned(),
            });
        }
        if !status.is_success() {
            return Err(AccountUsageError::Failed {
                safe_message: format!(
                    "OpenRouter quota request failed with status {}.",
                    status.as_u16()
                ),
            });
        }

        let body: OpenRouterKeyResponse = response.json().await.map_err(|_| AccountUsageError::Failed {
            safe_message: "OpenRouter quota response could not be parsed.".to_owned(),
        })?;

        let data = body.data;
        let quota_used = Some(decimal_to_micros(data.usage));
        let quota_total = data.limit.map(decimal_to_micros);
        let quota_remaining = data.limit_remaining.map(decimal_to_micros);

        Ok(supported_snapshot(
            &request,
            self.source_url(),
            self.cache_ttl(),
            quota_used,
            quota_total,
            quota_remaining,
            "usd_micro",
            Some("OpenRouter credits"),
        ))
    }
}

#[cfg(feature = "openrouter")]
#[derive(Debug, Deserialize)]
struct OpenRouterKeyResponse {
    data: OpenRouterKeyData,
}

#[cfg(feature = "openrouter")]
#[derive(Debug, Deserialize)]
struct OpenRouterKeyData {
    usage: f64,
    limit: Option<f64>,
    limit_remaining: Option<f64>,
}

#[cfg(feature = "deepseek")]
struct DeepSeekAccountUsageClient;

#[cfg(feature = "deepseek")]
#[async_trait]
impl ProviderAccountUsageClient for DeepSeekAccountUsageClient {
    fn provider_id(&self) -> &str {
        "deepseek"
    }

    fn source_url(&self) -> &'static str {
        DEEPSEEK_BALANCE_SOURCE
    }

    fn cache_ttl(&self) -> Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        let base_url = request
            .base_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("https://api.deepseek.com");
        let url = format!("{}/user/balance", base_url.trim_end_matches('/'));

        let response = reqwest::Client::new()
            .get(url)
            .bearer_auth(&request.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| AccountUsageError::Failed {
                safe_message: "DeepSeek quota request failed due to a network error.".to_owned(),
            })?;

        let status = response.status();
        if status.as_u16() == 401 {
            return Err(AccountUsageError::AuthRequired {
                safe_message: "DeepSeek rejected the configured API key.".to_owned(),
            });
        }
        if !status.is_success() {
            return Err(AccountUsageError::Failed {
                safe_message: format!(
                    "DeepSeek quota request failed with status {}.",
                    status.as_u16()
                ),
            });
        }

        let body: DeepSeekBalanceResponse = response.json().await.map_err(|_| {
            AccountUsageError::Failed {
                safe_message: "DeepSeek quota response could not be parsed.".to_owned(),
            }
        })?;

        let preferred = body
            .balance_infos
            .iter()
            .find(|entry| entry.currency.eq_ignore_ascii_case("USD"))
            .or_else(|| body.balance_infos.first());

        let Some(entry) = preferred else {
            return Err(AccountUsageError::Failed {
                safe_message: "DeepSeek quota response did not include balance information."
                    .to_owned(),
            });
        };

        let quota_remaining = decimal_string_to_micros(&entry.total_balance);
        let granted = decimal_string_to_micros(&entry.granted_balance);
        let topped_up = decimal_string_to_micros(&entry.topped_up_balance);
        let quota_total = match (granted, topped_up) {
            (Some(granted), Some(topped_up)) => Some(granted.saturating_add(topped_up)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => quota_remaining,
        };

        Ok(supported_snapshot(
            &request,
            self.source_url(),
            self.cache_ttl(),
            None,
            quota_total,
            quota_remaining,
            &format!("{}_micro", entry.currency.to_lowercase()),
            Some("DeepSeek account balance"),
        ))
    }
}

#[cfg(feature = "deepseek")]
#[derive(Debug, Deserialize)]
struct DeepSeekBalanceResponse {
    balance_infos: Vec<DeepSeekBalanceInfo>,
}

#[cfg(feature = "deepseek")]
#[derive(Debug, Deserialize)]
struct DeepSeekBalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}
