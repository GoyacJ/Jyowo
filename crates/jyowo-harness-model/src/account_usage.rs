//! Official provider account quota adapters and registry.
//!
//! SPEC: docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{OfficialQuotaScope, OfficialQuotaSnapshot, OfficialQuotaStatus};
#[cfg(any(
    feature = "anthropic",
    feature = "codex",
    feature = "deepseek",
    feature = "openai",
    feature = "openrouter"
))]
use serde::Deserialize;

/// Default cache TTL when a provider response does not include expiry metadata.
pub const DEFAULT_QUOTA_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[cfg(feature = "openrouter")]
const OPENROUTER_KEY_SOURCE: &str =
    "https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key";
#[cfg(feature = "deepseek")]
const DEEPSEEK_BALANCE_SOURCE: &str = "https://api-docs.deepseek.com/api/get-user-balance";
#[cfg(any(feature = "openai", feature = "codex"))]
const OPENAI_USAGE_SOURCE: &str = "https://platform.openai.com/docs/api-reference/usage";
#[cfg(feature = "anthropic")]
const ANTHROPIC_USAGE_SOURCE: &str =
    "https://platform.claude.com/docs/en/api/admin/analytics/usage/list";
#[cfg(feature = "anthropic")]
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountUsageRequest {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: Option<String>,
    pub api_key: String,
    pub official_quota_api_key: Option<String>,
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
        self.clients.insert(client.provider_id().to_owned(), client);
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
    register_default_account_usage_clients(&mut registry);
    registry
}

fn register_default_account_usage_clients(_registry: &mut ProviderAccountUsageRegistry) {
    #[cfg(feature = "openrouter")]
    _registry.register(Arc::new(OpenRouterAccountUsageClient));
    #[cfg(feature = "deepseek")]
    _registry.register(Arc::new(DeepSeekAccountUsageClient));
    #[cfg(feature = "openai")]
    _registry.register(Arc::new(OpenAiAccountUsageClient));
    #[cfg(feature = "codex")]
    _registry.register(Arc::new(CodexAccountUsageClient));
    #[cfg(feature = "anthropic")]
    _registry.register(Arc::new(AnthropicAccountUsageClient));
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
pub fn with_staleness(
    mut snapshot: OfficialQuotaSnapshot,
    now: DateTime<Utc>,
) -> OfficialQuotaSnapshot {
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

#[cfg(any(feature = "deepseek", feature = "openrouter"))]
#[must_use]
fn decimal_to_micros(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1_000_000.0).round() as u64
}

#[cfg(any(feature = "deepseek", feature = "openrouter"))]
#[must_use]
fn decimal_string_to_micros(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().map(decimal_to_micros)
}

#[cfg(any(
    feature = "anthropic",
    feature = "codex",
    feature = "deepseek",
    feature = "openai",
    feature = "openrouter"
))]
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

#[cfg(feature = "openai")]
struct OpenAiAccountUsageClient;

#[cfg(feature = "openai")]
#[async_trait]
impl ProviderAccountUsageClient for OpenAiAccountUsageClient {
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
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        fetch_openai_usage_quota(&request, self.source_url(), self.cache_ttl(), "OpenAI").await
    }
}

#[cfg(feature = "codex")]
struct CodexAccountUsageClient;

#[cfg(feature = "codex")]
#[async_trait]
impl ProviderAccountUsageClient for CodexAccountUsageClient {
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
        fetch_openai_usage_quota(&request, self.source_url(), self.cache_ttl(), "Codex").await
    }
}

#[cfg(any(feature = "openai", feature = "codex"))]
async fn fetch_openai_usage_quota(
    request: &ProviderAccountUsageRequest,
    source_url: &'static str,
    ttl: Duration,
    label: &str,
) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
    let Some(admin_key) = request
        .official_quota_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(AccountUsageError::AuthRequired {
            safe_message: format!(
                "{label} organization usage requires a separate admin API key for official quota."
            ),
        });
    };
    let url = openai_official_usage_url(request.base_url.as_deref(), label, Utc::now())?;

    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(admin_key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| AccountUsageError::Failed {
            safe_message: format!("{label} quota request failed due to a network error."),
        })?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AccountUsageError::AuthRequired {
            safe_message: format!("{label} rejected the configured official quota admin key."),
        });
    }
    if !status.is_success() {
        return Err(AccountUsageError::Failed {
            safe_message: format!(
                "{label} quota request failed with status {}.",
                status.as_u16()
            ),
        });
    }

    let body: OpenAiUsageResponse =
        response
            .json()
            .await
            .map_err(|_| AccountUsageError::Failed {
                safe_message: format!("{label} quota response could not be parsed."),
            })?;
    let input_tokens = body
        .data
        .iter()
        .map(|bucket| {
            bucket
                .results
                .iter()
                .map(|result| result.input_tokens)
                .sum::<u64>()
        })
        .sum::<u64>();
    let output_tokens = body
        .data
        .iter()
        .map(|bucket| {
            bucket
                .results
                .iter()
                .map(|result| result.output_tokens)
                .sum::<u64>()
        })
        .sum::<u64>();
    let total_tokens = input_tokens.saturating_add(output_tokens);

    let billing_label = format!("{label} organization usage");
    Ok(supported_snapshot(
        request,
        source_url,
        ttl,
        Some(total_tokens),
        None,
        None,
        "tokens",
        Some(&billing_label),
    ))
}

#[cfg(any(feature = "openai", feature = "codex"))]
fn openai_official_usage_url(
    base_url: Option<&str>,
    label: &str,
    generated_at: DateTime<Utc>,
) -> Result<String, AccountUsageError> {
    let base_url = base_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://api.openai.com")
        .trim_end_matches('/');
    let parsed = reqwest::Url::parse(base_url).map_err(|_| AccountUsageError::Failed {
        safe_message: format!("{label} official quota endpoint is not a valid URL."),
    })?;
    let is_official_origin = parsed.scheme() == "https"
        && parsed.host_str() == Some("api.openai.com")
        && matches!(parsed.port_or_known_default(), Some(443));
    let path = parsed.path().trim_end_matches('/');
    if !is_official_origin || !matches!(path, "" | "/" | "/v1") {
        return Err(AccountUsageError::Failed {
            safe_message: format!(
                "{label} official quota requires the official OpenAI API endpoint."
            ),
        });
    }
    let end_time = generated_at.timestamp().max(0);
    let start_time = (generated_at - chrono::TimeDelta::days(31))
        .timestamp()
        .max(0);
    let mut usage_url = reqwest::Url::parse(
        "https://api.openai.com/v1/organization/usage/completions",
    )
    .map_err(|_| AccountUsageError::Failed {
        safe_message: format!("{label} official quota endpoint is not a valid URL."),
    })?;
    usage_url
        .query_pairs_mut()
        .append_pair("start_time", &start_time.to_string())
        .append_pair("end_time", &end_time.to_string())
        .append_pair("bucket_width", "1d");
    Ok(usage_url.to_string())
}

#[cfg(any(feature = "openai", feature = "codex"))]
#[derive(Debug, Deserialize)]
struct OpenAiUsageResponse {
    #[serde(default)]
    data: Vec<OpenAiUsageBucket>,
}

#[cfg(any(feature = "openai", feature = "codex"))]
#[derive(Debug, Deserialize)]
struct OpenAiUsageBucket {
    #[serde(default)]
    results: Vec<OpenAiUsageResult>,
}

#[cfg(any(feature = "openai", feature = "codex"))]
#[derive(Debug, Deserialize)]
struct OpenAiUsageResult {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[cfg(feature = "anthropic")]
struct AnthropicAccountUsageClient;

#[cfg(feature = "anthropic")]
#[async_trait]
impl ProviderAccountUsageClient for AnthropicAccountUsageClient {
    fn provider_id(&self) -> &str {
        "anthropic"
    }

    fn source_url(&self) -> &'static str {
        ANTHROPIC_USAGE_SOURCE
    }

    fn cache_ttl(&self) -> Duration {
        DEFAULT_QUOTA_CACHE_TTL
    }

    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> Result<OfficialQuotaSnapshot, AccountUsageError> {
        let Some(admin_key) = request
            .official_quota_api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Err(AccountUsageError::AuthRequired {
                safe_message:
                    "Anthropic usage analytics requires a separate admin API key for official quota."
                        .to_owned(),
            });
        };
        let url = anthropic_official_usage_url(request.base_url.as_deref())?;

        let response = reqwest::Client::new()
            .get(url)
            .header("x-api-key", admin_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| AccountUsageError::Failed {
                safe_message: "Anthropic quota request failed due to a network error.".to_owned(),
            })?;

        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(AccountUsageError::AuthRequired {
                safe_message: "Anthropic rejected the configured official quota admin key."
                    .to_owned(),
            });
        }
        if !status.is_success() {
            return Err(AccountUsageError::Failed {
                safe_message: format!(
                    "Anthropic quota request failed with status {}.",
                    status.as_u16()
                ),
            });
        }

        let body: AnthropicUsageResponse =
            response
                .json()
                .await
                .map_err(|_| AccountUsageError::Failed {
                    safe_message: "Anthropic quota response could not be parsed.".to_owned(),
                })?;
        let total_tokens = anthropic_usage_tokens(&body);

        Ok(supported_snapshot(
            &request,
            self.source_url(),
            self.cache_ttl(),
            Some(total_tokens),
            None,
            None,
            "tokens",
            Some("Anthropic organization usage"),
        ))
    }
}

#[cfg(feature = "anthropic")]
fn anthropic_official_usage_url(base_url: Option<&str>) -> Result<String, AccountUsageError> {
    let base_url = base_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://api.anthropic.com")
        .trim_end_matches('/');
    let parsed = reqwest::Url::parse(base_url).map_err(|_| AccountUsageError::Failed {
        safe_message: "Anthropic official quota endpoint is not a valid URL.".to_owned(),
    })?;
    let is_official_origin = parsed.scheme() == "https"
        && parsed.host_str() == Some("api.anthropic.com")
        && matches!(parsed.port_or_known_default(), Some(443));
    let path = parsed.path().trim_end_matches('/');
    if !is_official_origin || !matches!(path, "" | "/" | "/v1") {
        return Err(AccountUsageError::Failed {
            safe_message: "Anthropic official quota requires the official Anthropic API endpoint."
                .to_owned(),
        });
    }

    let mut url =
        reqwest::Url::parse("https://api.anthropic.com/v1/organizations/usage_report/messages")
            .map_err(|_| AccountUsageError::Failed {
                safe_message: "Anthropic official quota endpoint is not a valid URL.".to_owned(),
            })?;
    let ending_at = Utc::now();
    let starting_at = ending_at - chrono::Duration::days(7);
    url.query_pairs_mut()
        .append_pair("starting_at", &starting_at.to_rfc3339())
        .append_pair("ending_at", &ending_at.to_rfc3339())
        .append_pair("bucket_width", "1d");
    Ok(url.into())
}

#[cfg(feature = "anthropic")]
fn anthropic_usage_tokens(body: &AnthropicUsageResponse) -> u64 {
    body.data
        .iter()
        .map(|bucket| {
            bucket
                .results
                .iter()
                .map(AnthropicUsageResult::total_tokens)
                .sum::<u64>()
        })
        .sum()
}

#[cfg(feature = "anthropic")]
#[derive(Debug, Deserialize)]
struct AnthropicUsageResponse {
    #[serde(default)]
    data: Vec<AnthropicUsageBucket>,
}

#[cfg(feature = "anthropic")]
#[derive(Debug, Deserialize)]
struct AnthropicUsageBucket {
    #[serde(default)]
    results: Vec<AnthropicUsageResult>,
}

#[cfg(feature = "anthropic")]
#[derive(Debug, Deserialize)]
struct AnthropicUsageResult {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    uncached_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

#[cfg(feature = "anthropic")]
impl AnthropicUsageResult {
    fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.uncached_input_tokens)
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.cache_read_input_tokens)
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
        let url = openrouter_official_key_url(request.base_url.as_deref())?;

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

        let body: OpenRouterKeyResponse =
            response
                .json()
                .await
                .map_err(|_| AccountUsageError::Failed {
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
fn openrouter_official_key_url(base_url: Option<&str>) -> Result<String, AccountUsageError> {
    let base_url = base_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://openrouter.ai/api")
        .trim_end_matches('/');
    let parsed = reqwest::Url::parse(base_url).map_err(|_| AccountUsageError::Failed {
        safe_message: "OpenRouter official quota endpoint is not a valid URL.".to_owned(),
    })?;
    let is_official_origin = parsed.scheme() == "https"
        && parsed.host_str() == Some("openrouter.ai")
        && matches!(parsed.port_or_known_default(), Some(443));
    let path = parsed.path().trim_end_matches('/');
    if !is_official_origin || !matches!(path, "" | "/" | "/api") {
        return Err(AccountUsageError::Failed {
            safe_message:
                "OpenRouter official quota requires the official OpenRouter API endpoint."
                    .to_owned(),
        });
    }
    Ok("https://openrouter.ai/api/v1/key".to_owned())
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
        let url = deepseek_official_balance_url(request.base_url.as_deref())?;

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

        let body: DeepSeekBalanceResponse =
            response
                .json()
                .await
                .map_err(|_| AccountUsageError::Failed {
                    safe_message: "DeepSeek quota response could not be parsed.".to_owned(),
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
fn deepseek_official_balance_url(base_url: Option<&str>) -> Result<String, AccountUsageError> {
    let base_url = base_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://api.deepseek.com")
        .trim_end_matches('/');
    let parsed = reqwest::Url::parse(base_url).map_err(|_| AccountUsageError::Failed {
        safe_message: "DeepSeek official quota endpoint is not a valid URL.".to_owned(),
    })?;
    let is_official_origin = parsed.scheme() == "https"
        && parsed.host_str() == Some("api.deepseek.com")
        && matches!(parsed.port_or_known_default(), Some(443));
    let path = parsed.path().trim_end_matches('/');
    if !is_official_origin || !matches!(path, "" | "/" | "/v1") {
        return Err(AccountUsageError::Failed {
            safe_message: "DeepSeek official quota requires the official DeepSeek API endpoint."
                .to_owned(),
        });
    }
    Ok("https://api.deepseek.com/user/balance".to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    #[cfg(any(feature = "openai", feature = "codex"))]
    fn openai_official_usage_url_accepts_only_official_origin() {
        let generated_at = Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap();
        let default_url = openai_official_usage_url(None, "OpenAI", generated_at).unwrap();
        assert_eq!(
            reqwest::Url::parse(&default_url).unwrap().path(),
            "/v1/organization/usage/completions"
        );

        let v1_url =
            openai_official_usage_url(Some("https://api.openai.com/v1"), "OpenAI", generated_at)
                .unwrap();
        assert_eq!(v1_url, default_url);

        let custom =
            openai_official_usage_url(Some("http://127.0.0.1:8080/v1"), "OpenAI", generated_at);
        assert!(matches!(custom, Err(AccountUsageError::Failed { .. })));
    }

    #[test]
    #[cfg(any(feature = "openai", feature = "codex"))]
    fn openai_official_usage_url_includes_required_usage_window() {
        let url = openai_official_usage_url(
            None,
            "OpenAI",
            Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap(),
        )
        .unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        let params = parsed
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<HashMap<_, _>>();

        assert_eq!(params.get("bucket_width"), Some(&"1d".to_owned()));
        assert_eq!(params.get("start_time"), Some(&"1780099200".to_owned()));
        assert_eq!(params.get("end_time"), Some(&"1782777600".to_owned()));
    }

    #[test]
    #[cfg(feature = "openrouter")]
    fn openrouter_official_key_url_accepts_only_official_origin() {
        let default_url = openrouter_official_key_url(None).unwrap();
        assert_eq!(default_url, "https://openrouter.ai/api/v1/key");

        let configured_url =
            openrouter_official_key_url(Some("https://openrouter.ai/api")).unwrap();
        assert_eq!(configured_url, default_url);

        let custom = openrouter_official_key_url(Some("https://gateway.example.com/api"));
        assert!(matches!(custom, Err(AccountUsageError::Failed { .. })));
    }

    #[test]
    #[cfg(feature = "deepseek")]
    fn deepseek_official_balance_url_accepts_only_official_origin() {
        let default_url = deepseek_official_balance_url(None).unwrap();
        assert_eq!(default_url, "https://api.deepseek.com/user/balance");

        let configured_url =
            deepseek_official_balance_url(Some("https://api.deepseek.com/v1")).unwrap();
        assert_eq!(configured_url, default_url);

        let custom = deepseek_official_balance_url(Some("https://gateway.example.com/v1"));
        assert!(matches!(custom, Err(AccountUsageError::Failed { .. })));
    }

    #[test]
    #[cfg(feature = "anthropic")]
    fn anthropic_usage_response_maps_token_fields() {
        let body: AnthropicUsageResponse = serde_json::from_value(serde_json::json!({
            "data": [
                {
                    "results": [
                        {
                            "input_tokens": 1,
                            "uncached_input_tokens": 2,
                            "cache_creation_input_tokens": 3,
                            "cache_read_input_tokens": 4,
                            "output_tokens": 5
                        }
                    ]
                }
            ]
        }))
        .unwrap();

        assert_eq!(anthropic_usage_tokens(&body), 15);
    }

    #[test]
    #[cfg(feature = "anthropic")]
    fn anthropic_official_usage_url_accepts_only_official_origin() {
        let default_url = anthropic_official_usage_url(None).unwrap();
        assert!(default_url
            .starts_with("https://api.anthropic.com/v1/organizations/usage_report/messages?"));

        let v1_url = anthropic_official_usage_url(Some("https://api.anthropic.com/v1")).unwrap();
        assert!(
            v1_url.starts_with("https://api.anthropic.com/v1/organizations/usage_report/messages?")
        );

        let custom = anthropic_official_usage_url(Some("http://127.0.0.1:8080/v1"));
        assert!(matches!(custom, Err(AccountUsageError::Failed { .. })));
    }
}
