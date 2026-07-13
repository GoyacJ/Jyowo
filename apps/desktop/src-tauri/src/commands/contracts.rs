#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
pub use harness_contracts::{
    McpHeaderEnvRecord, McpNameValueRecord, McpServerConfigRecord, McpServerTransportConfig,
};
use harness_contracts::{
    ModelUsageActivity, ModelUsageActivityDay, ModelUsageBucket, ModelUsagePeriod,
    ModelUsageSummary, ModelUsageWindow, ProviderProbeErrorKind, ProviderProbeSnapshot,
    ProviderProbeStatus, UsageSnapshot,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub type McpDiagnosticBatchEmitter =
    Arc<dyn Fn(McpDiagnosticBatchPayload) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoPayload {
    pub name: &'static str,
    pub version: &'static str,
    pub shell: &'static str,
    pub harness: HarnessInfoPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessInfoPayload {
    pub sdk_crate: &'static str,
    pub mode: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessHealthcheckPayload {
    pub status: &'static str,
    pub sdk_crate: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRuntimeToolsResponse {
    pub generation: u64,
    pub tools: Vec<RuntimeToolSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolSummary {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub group: String,
    pub group_label: String,
    pub origin_kind: String,
    pub origin_id: Option<String>,
    pub access: String,
    pub execution_channel: String,
    pub required_capabilities: Vec<String>,
    pub defer_policy: String,
    pub long_running: bool,
    pub service_binding: Option<RuntimeToolServiceBindingSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolServiceBindingSummary {
    pub provider_id: String,
    pub operation_id: String,
    pub route_kind: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsRequest {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub config_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub model_id: String,
    #[serde(default)]
    pub model_options: Option<harness_contracts::ModelRequestOptions>,
    #[serde(default)]
    pub official_quota_api_key: Option<String>,
    pub provider_id: String,
    #[serde(default)]
    pub protocol: Option<ModelProtocol>,
    #[serde(default)]
    pub provider_defaults: Option<ProviderDefaultsRecord>,
    #[serde(default = "default_true")]
    pub set_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderDefaultsRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateProviderSettingsRequest {
    pub model_id: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateProviderSettingsResponse {
    pub model_id: String,
    pub provider_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderSettingsResponse {
    pub config: ProviderConfigPayload,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProviderCapabilityRoutesResponse {
    pub version: u32,
    pub routes: Vec<ProviderCapabilityRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveProviderCapabilityRouteRequest {
    pub route: ProviderCapabilityRoute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderCapabilityRouteResponse {
    pub version: u32,
    pub routes: Vec<ProviderCapabilityRoute>,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteProviderCapabilityRouteRequest {
    pub kind: CapabilityRouteKind,
    pub config_id: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProviderCapabilityRouteResponse {
    pub version: u32,
    pub routes: Vec<ProviderCapabilityRoute>,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestProviderConfigApiKeyRevealRequest {
    pub config_id: String,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestProviderConfigApiKeyRevealResponse {
    pub config_id: String,
    pub expires_in_seconds: u64,
    pub reveal_token: String,
    pub status: &'static str,
}

impl std::fmt::Debug for RequestProviderConfigApiKeyRevealResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestProviderConfigApiKeyRevealResponse")
            .field("config_id", &self.config_id)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .field("reveal_token", &"[REDACTED]")
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderConfigApiKeyRequest {
    pub config_id: String,
    pub reveal_token: String,
}

impl std::fmt::Debug for GetProviderConfigApiKeyRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetProviderConfigApiKeyRequest")
            .field("config_id", &self.config_id)
            .field("reveal_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderConfigApiKeyResponse {
    pub api_key: String,
    pub config_id: String,
}

impl std::fmt::Debug for GetProviderConfigApiKeyResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetProviderConfigApiKeyResponse")
            .field("api_key", &"[REDACTED]")
            .field("config_id", &self.config_id)
            .finish()
    }
}

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderConfigRecord {
    pub api_key: String,
    pub protocol: ModelProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub display_name: String,
    pub id: String,
    pub model_id: String,
    #[serde(
        default,
        skip_serializing_if = "harness_contracts::ModelRequestOptions::is_empty"
    )]
    pub model_options: harness_contracts::ModelRequestOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_quota_api_key: Option<String>,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_defaults: Option<ProviderDefaultsRecord>,
    pub model_descriptor: ProviderModelDescriptorRecord,
}

impl std::fmt::Debug for ProviderConfigRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfigRecord")
            .field("api_key", &"[REDACTED]")
            .field("protocol", &self.protocol)
            .field("base_url", &self.base_url)
            .field("display_name", &self.display_name)
            .field("id", &self.id)
            .field("model_id", &self.model_id)
            .field(
                "official_quota_api_key",
                &self.official_quota_api_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("provider_id", &self.provider_id)
            .field("provider_defaults", &self.provider_defaults)
            .field("model_descriptor", &self.model_descriptor)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderModelDescriptorRecord {
    pub protocol: ModelProtocol,
    pub conversation_capability: ConversationModelCapabilityRecord,
    pub context_window: u32,
    pub display_name: String,
    pub lifecycle: ProviderModelLifecycleRecord,
    pub max_output_tokens: u32,
    pub model_id: String,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_semantics: Option<harness_contracts::ProviderRuntimeSemanticsDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversationModelCapabilityRecord {
    pub input_modalities: Vec<ProviderModelModalityRecord>,
    pub output_modalities: Vec<ProviderModelModalityRecord>,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub streaming: bool,
    pub tool_calling: bool,
    pub reasoning: bool,
    pub prompt_cache: bool,
    pub structured_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum ProviderModelLifecycleRecord {
    Stable,
    Preview,
    Retiring { retirement_date: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderModelModalityRecord {
    Text,
    Image,
    Audio,
    Video,
    File,
    Embedding,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_config_id: Option<String>,
    pub configs: Vec<ProviderConfigRecord>,
}

impl std::fmt::Debug for ProviderSettingsRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSettingsRecord")
            .field("default_config_id", &self.default_config_id)
            .field("configs", &self.configs)
            .finish()
    }
}

impl<'de> Deserialize<'de> for ProviderSettingsRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct WireRecord {
            #[serde(default)]
            default_config_id: Option<String>,
            configs: Vec<ProviderConfigRecord>,
        }

        let record = WireRecord::deserialize(deserializer)?;
        if record.configs.is_empty() {
            if record.default_config_id.is_some() {
                return Err(serde::de::Error::custom(
                    "defaultConfigId requires at least one provider config",
                ));
            }
            return Ok(Self {
                default_config_id: None,
                configs: Vec::new(),
            });
        }

        let Some(default_config_id) = record
            .default_config_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Err(serde::de::Error::custom(
                "defaultConfigId is required when provider configs exist",
            ));
        };
        if !record
            .configs
            .iter()
            .any(|config| config.id == default_config_id)
        {
            return Err(serde::de::Error::custom(
                "defaultConfigId must reference an existing provider config",
            ));
        }
        if record
            .configs
            .iter()
            .any(|config| config.api_key.trim().is_empty())
        {
            return Err(serde::de::Error::custom(
                "apiKey is required for every provider config",
            ));
        }

        Ok(Self {
            default_config_id: Some(default_config_id.to_owned()),
            configs: record.configs,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigPayload {
    pub protocol: ModelProtocol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub display_name: String,
    pub has_api_key: bool,
    pub has_official_quota_api_key: bool,
    pub id: String,
    pub is_default: bool,
    pub model_id: String,
    #[serde(skip_serializing_if = "harness_contracts::ModelRequestOptions::is_empty")]
    pub model_options: harness_contracts::ModelRequestOptions,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_defaults: Option<ProviderDefaultsRecord>,
    pub model_descriptor: ModelCatalogEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsScope {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProviderSettingsResponse {
    pub default_config_id: Option<String>,
    pub selection_scope: SettingsScope,
    pub configs: Vec<ProviderConfigPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderCatalogResponse {
    pub providers: Vec<ModelProviderCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderCatalogEntry {
    pub default_base_url: String,
    pub display_name: String,
    pub models: Vec<ModelCatalogEntry>,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_defaults: Option<ProviderDefaultsRecord>,
    pub runtime_capability: ProviderRuntimeCapabilityPayload,
    pub service_capabilities: Vec<ProviderServiceCapabilityPayload>,
    pub source_url: String,
    pub verified_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimeCapabilityPayload {
    pub auth_scheme: &'static str,
    pub base_url_regions: Vec<ProviderBaseUrlRegionPayload>,
    pub supports_live_validation: bool,
    pub supports_streaming_validation: bool,
    pub secret_reveal_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBaseUrlRegionPayload {
    pub id: String,
    pub label: String,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderServiceCapabilityPayload {
    pub operation_id: String,
    pub category: &'static str,
    pub input_modalities: Vec<ProviderModelModalityRecord>,
    pub output_artifact: ProviderModelModalityRecord,
    pub execution: &'static str,
    pub requires_polling: bool,
    pub permission_subject: String,
    pub cost_risk: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogEntry {
    pub protocol: ModelProtocol,
    pub supported_protocols: Vec<ModelProtocol>,
    pub supported_parameters: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_capability_metadata: Option<serde_json::Value>,
    pub conversation_capability: ConversationModelCapabilityRecord,
    pub context_window: u32,
    pub display_name: String,
    pub lifecycle: ModelLifecyclePayload,
    pub max_output_tokens: u32,
    pub model_id: String,
    pub runtime_status: ModelRuntimeStatusPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLifecyclePayload {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retirement_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRuntimeStatusPayload {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Default for ProviderSettingsRecord {
    fn default() -> Self {
        Self {
            default_config_id: None,
            configs: Vec::new(),
        }
    }
}

pub(crate) fn default_true() -> bool {
    true
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveMcpServerRequest {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub display_name: String,
    pub id: String,
    pub scope: String,
    pub transport: SaveMcpServerTransportConfig,
}

impl std::fmt::Debug for SaveMcpServerRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveMcpServerRequest")
            .field("enabled", &self.enabled)
            .field("display_name", &self.display_name)
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("transport", &self.transport)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpNameValueSaveRecord {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub preserve_existing: bool,
}

impl std::fmt::Debug for McpNameValueSaveRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpNameValueSaveRecord")
            .field("key", &self.key)
            .field("value", &self.value.as_ref().map(|_| "[REDACTED]"))
            .field("preserve_existing", &self.preserve_existing)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum SaveMcpServerTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<McpNameValueSaveRecord>,
        #[serde(default)]
        inherit_env: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token_env_var: Option<String>,
        #[serde(default)]
        headers: Vec<McpNameValueSaveRecord>,
        #[serde(default)]
        headers_from_env: Vec<McpHeaderEnvRecord>,
    },
    InProcess,
}

impl std::fmt::Debug for SaveMcpServerTransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio {
                command,
                args,
                env,
                inherit_env,
                working_dir,
            } => f
                .debug_struct("Stdio")
                .field("command", command)
                .field("args", args)
                .field("env", env)
                .field("inherit_env", inherit_env)
                .field("working_dir", working_dir)
                .finish(),
            Self::Http {
                url,
                bearer_token_env_var,
                headers,
                headers_from_env,
            } => f
                .debug_struct("Http")
                .field("url", url)
                .field("bearer_token_env_var", bearer_token_env_var)
                .field("headers", headers)
                .field("headers_from_env", headers_from_env)
                .finish(),
            Self::InProcess => f.write_str("InProcess"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpNameValueConfigPayload {
    pub has_value: bool,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum McpServerConfigTransportPayload {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<McpNameValueConfigPayload>,
        #[serde(default)]
        inherit_env: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token_env_var: Option<String>,
        #[serde(default)]
        headers: Vec<McpNameValueConfigPayload>,
        #[serde(default)]
        headers_from_env: Vec<McpHeaderEnvRecord>,
    },
    InProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigPayload {
    pub enabled: bool,
    pub display_name: String,
    pub id: String,
    pub scope: String,
    pub transport: McpServerConfigTransportPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteMcpServerRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetMcpServerEnabledRequest {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RestartMcpServerRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetMcpServerConfigRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ListMcpDiagnosticsRequest {
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClearMcpDiagnosticsRequest {
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SubscribeMcpDiagnosticsRequest {
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UnsubscribeMcpDiagnosticsRequest {
    pub subscription_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum McpDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiagnosticRecord {
    pub event_type: String,
    pub id: String,
    pub server_id: String,
    pub severity: McpDiagnosticSeverity,
    pub summary: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSummaryPayload {
    pub display_name: String,
    pub enabled: bool,
    pub exposed_tool_count: u32,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_diagnostic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_diagnostic_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_diagnostic_severity: Option<McpDiagnosticSeverity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub manageable: bool,
    pub origin: &'static str,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_plugin_id: Option<String>,
    pub status: &'static str,
    pub transport: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMcpServersResponse {
    pub servers: Vec<McpServerSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMcpServerResponse {
    pub server: McpServerSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMcpServerConfigResponse {
    pub server: McpServerConfigPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMcpServerResponse {
    pub id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMcpServerEnabledResponse {
    pub server: McpServerSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartMcpServerResponse {
    pub server: McpServerSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMcpDiagnosticsResponse {
    pub events: Vec<McpDiagnosticRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearMcpDiagnosticsResponse {
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeMcpDiagnosticsResponse {
    pub subscription_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub replay_events: Vec<McpDiagnosticRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeMcpDiagnosticsResponse {
    pub subscription_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiagnosticBatchPayload {
    pub subscription_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub events: Vec<McpDiagnosticRecord>,
    pub phase: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserMcpPresetId {
    Playwright,
    ChromeDevtools,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveBrowserMcpPresetRequest {
    pub preset_id: BrowserMcpPresetId,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMcpPresetSummaryPayload {
    pub description: &'static str,
    pub display_name: &'static str,
    pub enabled: bool,
    pub id: BrowserMcpPresetId,
    pub server_id: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListBrowserMcpPresetsResponse {
    pub presets: Vec<BrowserMcpPresetSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBrowserMcpPresetResponse {
    pub preset: BrowserMcpPresetSummaryPayload,
    pub server: McpServerSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ImportSkillRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetSkillDetailRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetSkillFileRequest {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetSkillEnabledRequest {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteSkillRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillStoreRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package_dir: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_name: String,
    pub imported_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_validation_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<SkillInstallOriginRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummaryPayload {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source_kind: String,
    pub enabled: bool,
    pub manageable: bool,
    pub status: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<SkillInstallOriginRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_plugin_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillParameterPayload {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFilePayload {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileContentPayload {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetailPayload {
    pub summary: SkillSummaryPayload,
    pub parameters: Vec<SkillParameterPayload>,
    pub config_keys: Vec<String>,
    pub files: Vec<SkillFilePayload>,
    pub body_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillsResponse {
    pub skills: Vec<SkillSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSkillDetailResponse {
    pub skill: SkillDetailPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSkillFileResponse {
    pub file: SkillFileContentPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSkillResponse {
    pub skill: SkillSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogInstallProgressPayload {
    pub operation_id: String,
    pub source_id: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub stage: &'static str,
    pub percent: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub type SkillCatalogInstallProgressEmitter =
    Arc<dyn Fn(SkillCatalogInstallProgressPayload) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogInstallTaskPayload {
    pub operation_id: String,
    pub source_id: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub stage: String,
    pub percent: u8,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillCatalogInstallTasksResponse {
    pub tasks: Vec<SkillCatalogInstallTaskPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSkillFromCatalogResponse {
    pub task: SkillCatalogInstallTaskPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SkillCatalogInstallTaskKey {
    pub(crate) source_id: String,
    pub(crate) entry_id: String,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSkillEnabledResponse {
    pub skill: SkillSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSkillResponse {
    pub id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ValidatePluginFromPathRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstallPluginFromPathRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetPluginDetailRequest {
    pub plugin_id: PluginId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetPluginEnabledRequest {
    pub plugin_id: PluginId,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetProjectPluginsEnabledRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetProjectPluginsEnabledResponse {
    pub allow_project_plugins: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UpdatePluginConfigRequest {
    pub plugin_id: PluginId,
    pub values: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UninstallPluginRequest {
    pub plugin_id: PluginId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReloadPluginRequest {
    pub plugin_id: PluginId,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPluginsResponse {
    pub allow_project_plugins: bool,
    pub plugins: Vec<PluginSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPluginDetailResponse {
    pub plugin: PluginDetail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginStoreRecord {
    pub plugin_id: PluginId,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub package_dir: String,
    pub source_path: String,
    pub content_hash: String,
    pub imported_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_validation_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginSettingsRecord {
    #[serde(default)]
    pub allow_project_plugins: bool,
    #[serde(default)]
    pub records: Vec<PluginStoreRecord>,
}

impl Default for PluginSettingsRecord {
    fn default() -> Self {
        Self {
            allow_project_plugins: false,
            records: Vec::new(),
        }
    }
}

pub trait ProviderSettingsStore: Send + Sync {
    fn selection_scope(&self) -> SettingsScope {
        SettingsScope::Global
    }

    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload>;
    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload>;

    fn compare_and_swap_record(
        &self,
        expected: Option<&ProviderSettingsRecord>,
        record: &ProviderSettingsRecord,
    ) -> Result<ProviderSettingsSaveOutcome, CommandErrorPayload> {
        if self.load_record()?.as_ref() != expected {
            return Ok(ProviderSettingsSaveOutcome::Conflict);
        }
        self.save_record(record)?;
        Ok(ProviderSettingsSaveOutcome::Saved)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSettingsSaveOutcome {
    Saved,
    Conflict,
}

#[derive(Debug)]
pub struct ProviderCapabilityRouteValidationToken {
    pub(crate) _private: (),
}

pub(crate) mod provider_capability_route_store_seal {
    pub trait Sealed {}
}

pub trait ProviderCapabilityRouteStore:
    provider_capability_route_store_seal::Sealed + Send + Sync
{
    fn project_scope_available(&self) -> bool {
        true
    }

    fn load_record(&self) -> Result<Option<ProviderCapabilityRouteSettings>, CommandErrorPayload>;
    fn save_record(
        &self,
        record: &ProviderCapabilityRouteSettings,
        validation: ProviderCapabilityRouteValidationToken,
    ) -> Result<(), CommandErrorPayload>;
}

pub trait ConversationMetadataStore: Send + Sync {
    fn load_record(&self) -> Result<ConversationMetadataFile, CommandErrorPayload>;
    fn save_record(&self, record: &ConversationMetadataFile) -> Result<(), CommandErrorPayload>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationMetadataState {
    Draft,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMetadataRecord {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub default_model_config_id: Option<String>,
    pub state: ConversationMetadataState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMetadataFile {
    pub version: u32,
    pub conversations: HashMap<String, ConversationMetadataRecord>,
}

impl Default for ConversationMetadataFile {
    fn default() -> Self {
        Self {
            version: 1,
            conversations: HashMap::new(),
        }
    }
}

pub trait McpServerStore: Send + Sync {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload>;
    fn save_record(&self, record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload>;
    fn delete_record(&self, id: &str) -> Result<(), CommandErrorPayload>;
}

pub trait McpDiagnosticStore: Send + Sync {
    fn load_records(&self) -> Result<Vec<McpDiagnosticRecord>, CommandErrorPayload>;
    fn append_record(&self, record: &McpDiagnosticRecord) -> Result<(), CommandErrorPayload>;
    fn clear_records(&self, server_id: Option<&str>) -> Result<(), CommandErrorPayload>;
}

pub trait SkillStore: Send + Sync {
    fn enabled_dir(&self) -> PathBuf;
    fn load_records(&self) -> Result<Vec<SkillStoreRecord>, CommandErrorPayload>;
    fn save_records(&self, records: &[SkillStoreRecord]) -> Result<(), CommandErrorPayload>;
    fn write_skill_package(
        &self,
        id: &str,
        enabled: bool,
        source_path: &Path,
    ) -> Result<String, CommandErrorPayload>;
    fn read_skill_entry_file(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<String, CommandErrorPayload>;
    fn list_skill_package_files(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<Vec<SkillFilePayload>, CommandErrorPayload>;
    fn read_skill_package_file(
        &self,
        record: &SkillStoreRecord,
        relative_path: &str,
    ) -> Result<SkillFileContentPayload, CommandErrorPayload>;
    fn move_skill_package(&self, id: &str, enabled: bool) -> Result<(), CommandErrorPayload>;
    fn delete_skill_package(&self, id: &str) -> Result<(), CommandErrorPayload>;
}

pub trait PluginStore: Send + Sync {
    fn package_root(&self) -> PathBuf;
    fn cargo_extension_root(&self) -> PathBuf;
    fn workspace_plugin_root(&self) -> PathBuf;
    fn load_record(&self) -> Result<PluginSettingsRecord, CommandErrorPayload>;
    fn save_record(&self, record: &PluginSettingsRecord) -> Result<(), CommandErrorPayload>;
    fn write_plugin_package(
        &self,
        package_dir: &str,
        source_path: &Path,
    ) -> Result<String, CommandErrorPayload>;
    fn delete_plugin_package(&self, package_dir: &str) -> Result<(), CommandErrorPayload>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummaryPayload {
    pub id: String,
    pub is_empty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_preview: Option<String>,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessagePayload {
    pub author: &'static str,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    pub id: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPayload {
    pub id: String,
    pub messages: Vec<ConversationMessagePayload>,
    pub model_config_id: Option<String>,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListConversationsResponse {
    pub conversations: Vec<ConversationSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConversationGroupPayload {
    pub project: crate::project_registry::ProjectRecord,
    pub conversations: Vec<ConversationSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProjectConversationGroupsResponse {
    pub active_path: Option<String>,
    pub groups: Vec<ProjectConversationGroupPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConversationResponse {
    pub conversation: ConversationSummaryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationRequest {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationResponse {
    pub conversation: ConversationPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteConversationRequest {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteConversationResponse {
    pub conversation_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StartRunRequest {
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentReferencePayload>>,
    #[serde(default)]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub context_references: Option<Vec<ContextReferencePayload>>,
    pub conversation_id: String,
    #[serde(default)]
    pub model_config_id: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextReferencePayload {
    WorkspaceFile { path: String, label: String },
    Artifact { id: String, label: String },
    Conversation { id: String, label: String },
    Memory { id: String, label: String },
    Skill { id: String, label: String },
    Tool { id: String, label: String },
    McpServer { id: String, label: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentReferencePayload {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub blob_ref: AttachmentBlobRefPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentBlobRefPayload {
    pub id: String,
    pub size: u64,
    pub content_hash: [u8; 32],
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunResponse {
    pub run_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentPayload {
    pub background_agent_id: String,
    pub conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_input_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_permission_request_id: Option<String>,
    pub state: BackgroundAgentState,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListBackgroundAgentsRequest {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListBackgroundAgentsResponse {
    pub agents: Vec<BackgroundAgentPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBackgroundAgentRequest {
    pub background_agent_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentIdRequest {
    pub background_agent_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendBackgroundAgentInputRequest {
    pub background_agent_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub request_id: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBackgroundAgentResponse {
    pub agent: BackgroundAgentPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentActionResponse {
    pub agent: BackgroundAgentPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentDeleteResponse {
    pub background_agent_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAttachmentFromPathRequest {
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAttachmentFromPathResponse {
    pub attachment: AttachmentReferencePayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceCandidatePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReferenceCandidatesResponse {
    pub artifacts: Vec<ReferenceCandidatePayload>,
    pub conversations: Vec<ReferenceCandidatePayload>,
    pub files: Vec<ReferenceCandidatePayload>,
    pub memories: Vec<ReferenceCandidatePayload>,
    pub mcp_servers: Vec<ReferenceCandidatePayload>,
    pub skills: Vec<ReferenceCandidatePayload>,
    pub tools: Vec<ReferenceCandidatePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReferenceCandidatesRequest {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRunRequest {
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRunResponse {
    pub run_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePermissionRequest {
    pub conversation_id: String,
    pub decision: PermissionDecision,
    pub option_id: String,
    pub request_id: String,
    #[serde(default)]
    pub confirmation_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePermissionResponse {
    pub decision: PermissionDecision,
    pub request_id: String,
    pub status: &'static str,
}

// ── Evidence fetch commands ──

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationCommandOutputRequest {
    pub conversation_id: String,
    pub full_output_ref: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationCommandOutputResponse {
    pub ref_id: String,
    pub kind: String,
    pub output: String,
    pub content_type: String,
    pub byte_length: u64,
    pub content_bytes: u64,
    pub offset_bytes: u64,
    pub limit_bytes: u64,
    pub total_bytes: u64,
    pub returned_bytes: u64,
    pub max_bytes: u64,
    pub truncated: bool,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub content_hash: String,
    pub hash_algorithm: String,
    pub redaction_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationDiffPatchRequest {
    pub conversation_id: String,
    pub full_patch_ref: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationDiffPatchResponse {
    pub ref_id: String,
    pub kind: String,
    pub patch: String,
    pub content_type: String,
    pub byte_length: u64,
    pub content_bytes: u64,
    pub offset_bytes: u64,
    pub limit_bytes: u64,
    pub total_bytes: u64,
    pub returned_bytes: u64,
    pub max_bytes: u64,
    pub truncated: bool,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub content_hash: String,
    pub hash_algorithm: String,
    pub redaction_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArtifactRevisionContentRequest {
    pub conversation_id: String,
    pub content_ref: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArtifactRevisionContentResponse {
    pub ref_id: String,
    pub kind: String,
    pub content: String,
    pub content_type: String,
    pub byte_length: u64,
    pub content_bytes: u64,
    pub offset_bytes: u64,
    pub limit_bytes: u64,
    pub total_bytes: u64,
    pub returned_bytes: u64,
    pub max_bytes: u64,
    pub truncated: bool,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub content_hash: String,
    pub hash_algorithm: String,
    pub redaction_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportConversationEvidenceRequest {
    pub conversation_id: String,
    pub kind: String,
    pub ref_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportConversationEvidenceResponse {
    pub ref_id: String,
    pub kind: String,
    pub path: String,
    pub content_type: String,
    pub byte_length: u64,
    pub exported_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListActivityRequest {
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventPayload {
    pub id: String,
    pub conversation_sequence: u64,
    pub payload: Value,
    pub run_id: String,
    pub sequence: u64,
    pub source: &'static str,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub visibility: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RunEventBodyPayload {
    PermissionRequested(PermissionRequestedRunEventPayload),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestedRunEventPayload {
    pub actor_source: PermissionActorSourceRunEventPayload,
    pub action_plan_hash: String,
    pub auto_resolved: bool,
    pub decision_options: Vec<serde_json::Value>,
    pub decision_scope: String,
    pub effective_mode: &'static str,
    pub exposure: String,
    pub operation: String,
    pub reason: String,
    pub review: serde_json::Value,
    pub request_id: String,
    pub sandbox_policy: serde_json::Value,
    pub severity: &'static str,
    pub target: String,
    pub tool_use_id: String,
    pub workspace_boundary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PermissionActorSourceRunEventPayload {
    ParentRun,
    Subagent {
        subagent_id: String,
        parent_session_id: String,
        parent_run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        team_member_profile_id: Option<String>,
    },
    TeamMember {
        team_id: String,
        agent_id: String,
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_run_id: Option<String>,
    },
    BackgroundAgent {
        background_agent_id: String,
        conversation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        attempt_id: Option<String>,
    },
    Automation {
        automation_id: String,
        conversation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
    McpServer {
        server_id: String,
        origin: ManifestOriginRunEventPayload,
        scope: McpServerScopeRunEventPayload,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ManifestOriginRunEventPayload {
    File { path: String },
    CargoExtension { binary: String },
    RemoteRegistry { endpoint: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum McpServerScopeRunEventPayload {
    Global,
    Session { conversation_id: String },
    Agent { agent_id: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListActivityResponse {
    pub events: Vec<RunEventPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayTimelineRequest {
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayTimelineResponse {
    pub events: Vec<RunEventPayload>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConversationInspectorItemRequest {
    pub conversation_id: String,
    pub selection: ConversationInspectorSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSupportBundleRequest {
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSupportBundleResponse {
    pub bundle_path: String,
    pub event_count: u32,
    pub exported_at: String,
    pub jsonl_path: String,
    pub markdown_path: String,
    pub redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRevisionPayload {
    pub revision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<String>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_ref: Option<String>,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummaryPayload {
    pub action_label: String,
    pub description: String,
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revisions: Vec<ArtifactRevisionPayload>,
    #[serde(skip_serializing)]
    pub source_message_id: Option<String>,
    #[serde(skip_serializing)]
    pub source_run_id: String,
    pub status: &'static str,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArtifactsRequest {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArtifactsResponse {
    pub artifacts: Vec<ArtifactSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArtifactMediaPreviewRequest {
    pub conversation_id: String,
    pub artifact_id: String,
    #[serde(default)]
    pub content_ref: Option<String>,
    #[serde(default)]
    pub revision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArtifactMediaPreviewResponse {
    pub data_url: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAttachmentMediaPreviewRequest {
    pub conversation_id: String,
    pub attachment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAttachmentMediaPreviewResponse {
    pub data_url: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContextSnapshotRequest {
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDecisionPayload {
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextFilePayload {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContextSnapshotResponse {
    pub active_artifact: Option<String>,
    pub decisions: Vec<ContextDecisionPayload>,
    pub files: Vec<ContextFilePayload>,
    pub next_actions: Vec<String>,
    pub path: String,
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExecutionSettingsResponse {
    pub permission_mode: PermissionMode,
    pub tool_profile: ToolProfile,
    pub context_compression_trigger_ratio: f32,
    pub scope: SettingsScope,
    pub auto_mode_available: bool,
    pub agent_capabilities: AgentCapabilitiesPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExecutionSettingsRequest {
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExecutionSettingsRequest {
    pub permission_mode: PermissionMode,
    pub tool_profile: ToolProfile,
    pub context_compression_trigger_ratio: f32,
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExecutionSettingsResponse {
    pub permission_mode: PermissionMode,
    pub tool_profile: ToolProfile,
    pub context_compression_trigger_ratio: f32,
    pub scope: SettingsScope,
    pub auto_mode_available: bool,
    pub agent_capabilities: AgentCapabilitiesPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveAutomationRequest {
    pub automation: AutomationSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAutomationResponse {
    pub automation: AutomationSpec,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetAutomationEnabledRequest {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutomationEnabledResponse {
    pub automation: AutomationSpec,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteAutomationRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAutomationResponse {
    pub id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAutomationsResponse {
    pub automations: Vec<AutomationSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunAutomationNowRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAutomationNowResponse {
    pub record: AutomationRunRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ListAutomationRunsRequest {
    #[serde(default)]
    pub automation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAutomationRunsResponse {
    pub runs: Vec<AutomationRunRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalLastRunPayload {
    pub completed_at: Option<&'static str>,
    pub failed: u32,
    pub passed: u32,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalCasePayload {
    pub id: String,
    pub last_run: Option<EvalLastRunPayload>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEvalCasesResponse {
    pub cases: Vec<EvalCasePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvalCaseRequest {
    pub case_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvalCaseResponse {
    pub case: EvalCasePayload,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProbeProviderConfigRequest {
    pub config_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentProfilesResponse {
    pub profiles: Vec<AgentProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshotPayload {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_micros: u64,
    pub tool_calls: u64,
}

impl From<UsageSnapshot> for UsageSnapshotPayload {
    fn from(value: UsageSnapshot) -> Self {
        Self {
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
            cost_micros: value.cost_micros,
            tool_calls: value.tool_calls,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbeSnapshotPayload {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub status: ProviderProbeStatusPayload,
    pub timeout_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub checked_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ProviderProbeErrorKindPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeStatusPayload {
    Online,
    Timeout,
    Unauthenticated,
    RateLimited,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeErrorKindPayload {
    Timeout,
    Auth,
    RateLimit,
    Network,
    Provider,
    Unsupported,
    InvalidConfig,
    Unknown,
}

impl From<ProviderProbeStatus> for ProviderProbeStatusPayload {
    fn from(value: ProviderProbeStatus) -> Self {
        match value {
            ProviderProbeStatus::Online => Self::Online,
            ProviderProbeStatus::Timeout => Self::Timeout,
            ProviderProbeStatus::Unauthenticated => Self::Unauthenticated,
            ProviderProbeStatus::RateLimited => Self::RateLimited,
            ProviderProbeStatus::Unsupported => Self::Unsupported,
            ProviderProbeStatus::Failed => Self::Failed,
            _ => Self::Failed,
        }
    }
}

impl From<ProviderProbeErrorKind> for ProviderProbeErrorKindPayload {
    fn from(value: ProviderProbeErrorKind) -> Self {
        match value {
            ProviderProbeErrorKind::Timeout => Self::Timeout,
            ProviderProbeErrorKind::Auth => Self::Auth,
            ProviderProbeErrorKind::RateLimit => Self::RateLimit,
            ProviderProbeErrorKind::Network => Self::Network,
            ProviderProbeErrorKind::Provider => Self::Provider,
            ProviderProbeErrorKind::Unsupported => Self::Unsupported,
            ProviderProbeErrorKind::InvalidConfig => Self::InvalidConfig,
            ProviderProbeErrorKind::Unknown => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

impl From<ProviderProbeSnapshot> for ProviderProbeSnapshotPayload {
    fn from(snapshot: ProviderProbeSnapshot) -> Self {
        Self {
            config_id: snapshot.config_id,
            provider_id: snapshot.provider_id,
            model_id: snapshot.model_id,
            status: snapshot.status.into(),
            timeout_ms: snapshot.timeout_ms,
            latency_ms: snapshot.latency_ms,
            checked_at: snapshot.checked_at.to_rfc3339(),
            error_kind: snapshot.error_kind.map(Into::into),
            safe_message: snapshot.safe_message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeProviderConfigResponse {
    pub snapshot: ProviderProbeSnapshotPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_usage: Option<UsageSnapshotPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProviderProbeSnapshotsResponse {
    pub snapshots: Vec<ProviderProbeSnapshotPayload>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelUsagePeriodPayload {
    Today,
    MonthToDate,
    AllTime,
}

impl From<ModelUsagePeriod> for ModelUsagePeriodPayload {
    fn from(value: ModelUsagePeriod) -> Self {
        match value {
            ModelUsagePeriod::Today => Self::Today,
            ModelUsagePeriod::MonthToDate => Self::MonthToDate,
            ModelUsagePeriod::AllTime => Self::AllTime,
            _ => Self::AllTime,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageBucketPayload {
    pub key: String,
    pub provider_id: String,
    pub model_id: String,
    pub usage: UsageSnapshotPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

impl From<ModelUsageBucket> for ModelUsageBucketPayload {
    fn from(value: ModelUsageBucket) -> Self {
        Self {
            key: value.key,
            provider_id: value.provider_id,
            model_id: value.model_id,
            usage: value.usage.into(),
            last_used_at: value.last_used_at.map(|at| at.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageWindowPayload {
    pub period: ModelUsagePeriodPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_end: Option<String>,
    pub total: UsageSnapshotPayload,
    pub by_model: Vec<ModelUsageBucketPayload>,
}

impl From<ModelUsageWindow> for ModelUsageWindowPayload {
    fn from(value: ModelUsageWindow) -> Self {
        Self {
            period: value.period.into(),
            period_start: value.period_start.map(|at| at.to_rfc3339()),
            period_end: value.period_end.map(|at| at.to_rfc3339()),
            total: value.total.into(),
            by_model: value.by_model.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageActivityDayPayload {
    pub date: String,
    pub usage: UsageSnapshotPayload,
}

impl From<ModelUsageActivityDay> for ModelUsageActivityDayPayload {
    fn from(value: ModelUsageActivityDay) -> Self {
        Self {
            date: value.date.to_string(),
            usage: value.usage.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageActivityPayload {
    pub range_start: String,
    pub range_end: String,
    pub days: Vec<ModelUsageActivityDayPayload>,
    pub peak_day_tokens: u64,
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    pub longest_task_duration_ms: u64,
}

impl From<ModelUsageActivity> for ModelUsageActivityPayload {
    fn from(value: ModelUsageActivity) -> Self {
        Self {
            range_start: value.range_start.to_string(),
            range_end: value.range_end.to_string(),
            days: value.days.into_iter().map(Into::into).collect(),
            peak_day_tokens: value.peak_day_tokens,
            current_streak_days: value.current_streak_days,
            longest_streak_days: value.longest_streak_days,
            longest_task_duration_ms: value.longest_task_duration_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetModelUsageSummaryResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone_id: Option<String>,
    pub timezone_offset_minutes: i32,
    pub today: ModelUsageWindowPayload,
    pub month_to_date: ModelUsageWindowPayload,
    pub all_time: ModelUsageWindowPayload,
    pub activity: ModelUsageActivityPayload,
    pub generated_at: String,
}

impl From<ModelUsageSummary> for GetModelUsageSummaryResponse {
    fn from(value: ModelUsageSummary) -> Self {
        Self {
            timezone_id: value.timezone_id,
            timezone_offset_minutes: value.timezone_offset_minutes,
            today: value.today.into(),
            month_to_date: value.month_to_date.into(),
            all_time: value.all_time.into(),
            activity: value.activity.into(),
            generated_at: value.generated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsPageSlice<T> {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_message: Option<String>,
}

impl<T> ModelSettingsPageSlice<T> {
    pub fn ready(data: T) -> Self {
        Self {
            status: "ready",
            data: Some(data),
            safe_message: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: "error",
            data: None,
            safe_message: Some(message.into()),
        }
    }

    pub fn rebuilding(message: impl Into<String>) -> Self {
        Self {
            status: "rebuilding",
            data: None,
            safe_message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsCatalogSnapshotPayload {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful_refresh_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsPageResponse {
    pub catalog: ModelProviderCatalogResponse,
    pub catalog_snapshot: ModelSettingsCatalogSnapshotPayload,
    pub provider_settings: ListProviderSettingsResponse,
    pub probe_snapshots: ModelSettingsPageSlice<ListProviderProbeSnapshotsResponse>,
    pub usage_summary: ModelSettingsPageSlice<GetModelUsageSummaryResponse>,
    pub quota_snapshots: ModelSettingsPageSlice<ListOfficialQuotaSnapshotsResponse>,
    pub capability_routes: ModelSettingsPageSlice<ListProviderCapabilityRoutesResponse>,
    pub capability_route_options:
        ModelSettingsPageSlice<ListProviderCapabilityRouteOptionsResponse>,
    pub generated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshModelProviderCatalogResponse {
    pub catalog: ModelProviderCatalogResponse,
    pub catalog_snapshot: ModelSettingsCatalogSnapshotPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogSnapshotRecord {
    pub openrouter_models_api_json: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_models_api_json: Option<serde_json::Value>,
    pub last_successful_refresh_at: chrono::DateTime<chrono::Utc>,
    pub last_attempt_at: chrono::DateTime<chrono::Utc>,
}

pub trait ProviderCatalogSnapshotStore: Send + Sync {
    fn load_record(&self) -> Result<Option<ProviderCatalogSnapshotRecord>, CommandErrorPayload>;
    fn save_record(
        &self,
        record: &ProviderCatalogSnapshotRecord,
    ) -> Result<(), CommandErrorPayload>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUsageRollupRecord {
    pub schema_version: u32,
    pub dirty: bool,
    #[serde(default)]
    pub rebuilding: bool,
    #[serde(default)]
    pub last_global_offset: u64,
    #[serde(default)]
    pub timezone_id: Option<String>,
    #[serde(default)]
    pub timezone_offset_minutes: i32,
    #[serde(default)]
    pub day_buckets: BTreeMap<chrono::NaiveDate, ModelUsageDayRecord>,
    pub summary: ModelUsageSummary,
    #[serde(default)]
    pub pending_run_starts: BTreeMap<String, chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub longest_completed_duration_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelUsageDayRecord {
    pub by_model: BTreeMap<String, ModelUsageDayModelRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUsageDayModelRecord {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub usage: UsageSnapshot,
    pub last_used_at: chrono::DateTime<chrono::Utc>,
}

pub trait ModelUsageRollupStore: Send + Sync {
    fn load_record(&self) -> Result<Option<ModelUsageRollupRecord>, CommandErrorPayload>;
    fn save_record(&self, record: &ModelUsageRollupRecord) -> Result<(), CommandErrorPayload>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDiagnosticsRecord {
    pub snapshots: Vec<ProviderProbeSnapshot>,
}

pub trait ProviderDiagnosticsStore: Send + Sync {
    fn load_record(&self) -> Result<ProviderDiagnosticsRecord, CommandErrorPayload>;
    fn upsert_snapshot(&self, snapshot: &ProviderProbeSnapshot) -> Result<(), CommandErrorPayload>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaScopePayload {
    Account,
    Project,
    Provider,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfficialQuotaStatusPayload {
    Supported,
    Unsupported,
    NotConfigured,
    AuthRequired,
    Failed,
}

impl From<harness_contracts::OfficialQuotaScope> for OfficialQuotaScopePayload {
    fn from(value: harness_contracts::OfficialQuotaScope) -> Self {
        match value {
            harness_contracts::OfficialQuotaScope::Account => Self::Account,
            harness_contracts::OfficialQuotaScope::Project => Self::Project,
            harness_contracts::OfficialQuotaScope::Provider => Self::Provider,
            harness_contracts::OfficialQuotaScope::Model => Self::Model,
            _ => Self::Account,
        }
    }
}

impl From<harness_contracts::OfficialQuotaStatus> for OfficialQuotaStatusPayload {
    fn from(value: harness_contracts::OfficialQuotaStatus) -> Self {
        match value {
            harness_contracts::OfficialQuotaStatus::Supported => Self::Supported,
            harness_contracts::OfficialQuotaStatus::Unsupported => Self::Unsupported,
            harness_contracts::OfficialQuotaStatus::NotConfigured => Self::NotConfigured,
            harness_contracts::OfficialQuotaStatus::AuthRequired => Self::AuthRequired,
            harness_contracts::OfficialQuotaStatus::Failed => Self::Failed,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialQuotaSnapshotPayload {
    pub config_id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub scope: OfficialQuotaScopePayload,
    pub status: OfficialQuotaStatusPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_remaining: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_label: Option<String>,
    pub source_url: String,
    pub fetched_at: String,
    pub expires_at: String,
    pub is_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_message: Option<String>,
}

impl From<harness_contracts::OfficialQuotaSnapshot> for OfficialQuotaSnapshotPayload {
    fn from(value: harness_contracts::OfficialQuotaSnapshot) -> Self {
        Self {
            config_id: value.config_id,
            provider_id: value.provider_id,
            model_id: value.model_id,
            scope: value.scope.into(),
            status: value.status.into(),
            period_start: value.period_start.map(|at| at.to_rfc3339()),
            period_end: value.period_end.map(|at| at.to_rfc3339()),
            quota_used: value.quota_used,
            quota_total: value.quota_total,
            quota_remaining: value.quota_remaining,
            unit: value.unit,
            billing_label: value.billing_label,
            source_url: value.source_url,
            fetched_at: value.fetched_at.to_rfc3339(),
            expires_at: value.expires_at.to_rfc3339(),
            is_stale: value.is_stale,
            safe_message: value.safe_message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAgentProfileResponse {
    pub profile: AgentProfile,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshOfficialQuotaRequest {
    pub config_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteAgentProfileRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOfficialQuotaResponse {
    pub snapshot: OfficialQuotaSnapshotPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOfficialQuotaSnapshotsResponse {
    pub snapshots: Vec<OfficialQuotaSnapshotPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQuotaCacheRecord {
    pub snapshots: Vec<harness_contracts::OfficialQuotaSnapshot>,
}

pub trait ProviderQuotaCacheStore: Send + Sync {
    fn load_record(&self) -> Result<ProviderQuotaCacheRecord, CommandErrorPayload>;
    fn upsert_snapshot(
        &self,
        snapshot: &harness_contracts::OfficialQuotaSnapshot,
    ) -> Result<(), CommandErrorPayload>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAgentProfileResponse {
    pub id: String,
    pub status: &'static str,
}

#[cfg(test)]
mod debug_redaction_tests {
    use super::*;

    fn provider_config() -> ProviderConfigRecord {
        ProviderConfigRecord {
            api_key: "provider-secret-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: Some("https://gateway.example.com".to_owned()),
            display_name: "OpenAI Work".to_owned(),
            id: "openai-work".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: harness_contracts::ModelRequestOptions::default(),
            official_quota_api_key: Some("quota-secret-token".to_owned()),
            provider_id: "openai".to_owned(),
            provider_defaults: None,
            model_descriptor: ProviderModelDescriptorRecord {
                protocol: ModelProtocol::Responses,
                conversation_capability: ConversationModelCapabilityRecord {
                    input_modalities: vec![ProviderModelModalityRecord::Text],
                    output_modalities: vec![ProviderModelModalityRecord::Text],
                    context_window: 128_000,
                    max_output_tokens: 16_384,
                    streaming: true,
                    tool_calling: true,
                    reasoning: true,
                    prompt_cache: true,
                    structured_output: true,
                },
                context_window: 128_000,
                display_name: "GPT".to_owned(),
                lifecycle: ProviderModelLifecycleRecord::Stable,
                max_output_tokens: 16_384,
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                runtime_semantics: None,
            },
        }
    }

    #[test]
    fn provider_settings_debug_redacts_secret_fields() {
        let settings = ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![provider_config()],
        };

        let debug = format!("{settings:?}");

        assert!(!debug.contains("provider-secret-token"));
        assert!(!debug.contains("quota-secret-token"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn provider_reveal_debug_redacts_tokens_and_raw_secret() {
        let reveal_response = RequestProviderConfigApiKeyRevealResponse {
            config_id: "openai-work".to_owned(),
            expires_in_seconds: 30,
            reveal_token: "reveal-token-secret".to_owned(),
            status: "ready",
        };
        let get_request = GetProviderConfigApiKeyRequest {
            config_id: "openai-work".to_owned(),
            reveal_token: "reveal-token-secret".to_owned(),
        };
        let get_response = GetProviderConfigApiKeyResponse {
            api_key: "provider-secret-token".to_owned(),
            config_id: "openai-work".to_owned(),
        };

        let debug = format!("{reveal_response:?}\n{get_request:?}\n{get_response:?}");

        assert!(!debug.contains("reveal-token-secret"));
        assert!(!debug.contains("provider-secret-token"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn mcp_debug_redacts_inline_env_and_headers() {
        let request = SaveMcpServerRequest {
            enabled: true,
            display_name: "Server".to_owned(),
            id: "server".to_owned(),
            scope: "workspace".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com".to_owned(),
                bearer_token_env_var: Some("MCP_TOKEN".to_owned()),
                headers: vec![McpNameValueSaveRecord {
                    key: "Authorization".to_owned(),
                    value: Some("Bearer leaked-token".to_owned()),
                    preserve_existing: false,
                }],
                headers_from_env: vec![McpHeaderEnvRecord {
                    key: "X-Api-Key".to_owned(),
                    env_var: "MCP_API_KEY".to_owned(),
                }],
            },
        };
        let record = McpServerConfigRecord {
            enabled: request.enabled,
            display_name: request.display_name.clone(),
            id: request.id.clone(),
            scope: request.scope.clone(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["server.js".to_owned()],
                env: vec![McpNameValueRecord {
                    key: "TOKEN".to_owned(),
                    value: "stdio-secret-token".to_owned(),
                }],
                inherit_env: vec!["PATH".to_owned()],
                working_dir: None,
            },
        };

        let debug = format!("{request:?}\n{record:?}");

        assert!(!debug.contains("Bearer leaked-token"));
        assert!(!debug.contains("stdio-secret-token"));
        assert!(debug.contains("[REDACTED]"));
    }
}
