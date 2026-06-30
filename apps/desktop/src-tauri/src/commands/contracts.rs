#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::conversations::*;
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
use super::memory::*;
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

pub type ConversationEventBatchEmitter =
    Arc<dyn Fn(ConversationEventBatchPayload) -> Result<(), String> + Send + Sync>;
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

#[derive(Deserialize)]
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
    pub provider_id: String,
    #[serde(default = "default_true")]
    pub set_default: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestProviderConfigApiKeyRevealResponse {
    pub config_id: String,
    pub expires_in_seconds: u64,
    pub reveal_token: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderConfigApiKeyRequest {
    pub config_id: String,
    pub reveal_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderConfigApiKeyResponse {
    pub api_key: String,
    pub config_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderConfigRecord {
    pub api_key: String,
    pub protocol: ModelProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub display_name: String,
    pub id: String,
    pub model_id: String,
    pub provider_id: String,
    pub model_descriptor: ProviderModelDescriptorRecord,
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
    Deprecated { retirement_date: String },
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_config_id: Option<String>,
    pub configs: Vec<ProviderConfigRecord>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigPayload {
    pub protocol: ModelProtocol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub display_name: String,
    pub has_api_key: bool,
    pub id: String,
    pub is_default: bool,
    pub model_id: String,
    pub provider_id: String,
    pub model_descriptor: ModelCatalogEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProviderSettingsResponse {
    pub default_config_id: Option<String>,
    pub configs: Vec<ProviderConfigPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderCatalogResponse {
    pub providers: Vec<ModelProviderCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderCatalogEntry {
    pub default_base_url: String,
    pub display_name: String,
    pub models: Vec<ModelCatalogEntry>,
    pub provider_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveMcpServerRequest {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub display_name: String,
    pub id: String,
    pub scope: String,
    pub transport: McpServerTransportConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpNameValueRecord {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpHeaderEnvRecord {
    pub key: String,
    pub env_var: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum McpServerTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<McpNameValueRecord>,
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
        headers: Vec<McpNameValueRecord>,
        #[serde(default)]
        headers_from_env: Vec<McpHeaderEnvRecord>,
    },
    InProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpServerConfigRecord {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub display_name: String,
    pub id: String,
    pub scope: String,
    pub transport: McpServerTransportConfig,
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
    pub server: McpServerConfigRecord,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetMemoryItemRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UpdateMemoryItemRequest {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteMemoryItemRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItemSummaryPayload {
    pub content_preview: String,
    pub id: String,
    pub kind: String,
    pub source: String,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub visibility: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItemPayload {
    pub access_count: u32,
    pub confidence: f32,
    pub content: String,
    pub created_at: String,
    pub id: String,
    pub kind: String,
    pub source: String,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub visibility: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMemoryItemsResponse {
    pub items: Vec<MemoryItemSummaryPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMemoryItemResponse {
    pub item: MemoryItemPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemoryItemResponse {
    pub item: MemoryItemPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMemoryItemResponse {
    pub id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMemoryItemsResponse {
    pub exported_at: String,
    pub format: &'static str,
    pub item_count: u32,
    pub path: String,
}

pub trait PermissionResolver: Send + Sync {
    fn resolve_permission<'a>(
        &'a self,
        request_id: RequestId,
        decision: Decision,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandErrorPayload>> + Send + 'a>>;
}

pub trait ProviderSettingsStore: Send + Sync {
    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload>;
    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload>;
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
    fn load_record(&self) -> Result<Option<ProviderCapabilityRouteSettings>, CommandErrorPayload>;
    fn save_record(
        &self,
        record: &ProviderCapabilityRouteSettings,
        validation: ProviderCapabilityRouteValidationToken,
    ) -> Result<(), CommandErrorPayload>;
}

pub trait ConversationModelConfigStore: Send + Sync {
    fn load_records(&self) -> Result<HashMap<String, String>, CommandErrorPayload>;
    fn save_records(&self, records: &HashMap<String, String>) -> Result<(), CommandErrorPayload>;
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

pub trait AutomationStore: Send + Sync {
    fn load_automations(&self) -> Result<Vec<AutomationSpec>, CommandErrorPayload>;
    fn save_automations(&self, records: &[AutomationSpec]) -> Result<(), CommandErrorPayload>;
    fn load_run_records(&self) -> Result<Vec<AutomationRunRecord>, CommandErrorPayload>;
    fn append_run_record(&self, record: &AutomationRunRecord) -> Result<(), CommandErrorPayload>;
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
    ) -> Result<(), CommandErrorPayload>;
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
    ) -> Result<(), CommandErrorPayload>;
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
pub struct SetConversationModelConfigRequest {
    pub conversation_id: String,
    pub model_config_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetConversationModelConfigResponse {
    pub conversation_id: String,
    pub model_config_id: String,
    pub status: &'static str,
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
#[serde(rename_all = "camelCase")]
pub struct StartRunRequest {
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentReferencePayload>>,
    #[serde(default)]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub context_references: Option<Vec<ContextReferencePayload>>,
    pub conversation_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAttachmentFromPathRequest {
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
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePermissionResponse {
    pub decision: PermissionDecision,
    pub request_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListActivityRequest {
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeConversationEventsRequest {
    pub conversation_id: String,
    pub after_cursor: Option<ConversationCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeConversationEventsResponse {
    pub subscription_id: String,
    pub conversation_id: String,
    pub replay_events: Vec<RunEventPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
    pub gap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeConversationEventsRequest {
    pub subscription_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeConversationEventsResponse {
    pub subscription_id: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEventBatchPayload {
    pub subscription_id: String,
    pub conversation_id: String,
    pub events: Vec<RunEventPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
    pub gap: bool,
    pub phase: &'static str,
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
    pub auto_resolved: bool,
    pub decision_scope: String,
    pub exposure: String,
    pub operation: String,
    pub reason: String,
    pub request_id: String,
    pub severity: &'static str,
    pub target: String,
    pub tool_use_id: String,
    pub workspace_boundary: String,
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
pub struct PageConversationTimelineRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub after_cursor: Option<ConversationCursor>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageConversationWorktreeDirection {
    Before,
    After,
}

impl From<PageConversationWorktreeDirection> for ConversationTurnPageDirection {
    fn from(value: PageConversationWorktreeDirection) -> Self {
        match value {
            PageConversationWorktreeDirection::Before => ConversationTurnPageDirection::Before,
            PageConversationWorktreeDirection::After => ConversationTurnPageDirection::After,
        }
    }
}

pub(crate) fn default_worktree_direction() -> PageConversationWorktreeDirection {
    PageConversationWorktreeDirection::After
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageConversationWorktreeRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub page_cursor: Option<ConversationTurnCursor>,
    #[serde(default = "default_worktree_direction")]
    pub direction: PageConversationWorktreeDirection,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageConversationTimelineResponse {
    pub events: Vec<RunEventPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
    pub gap: bool,
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
pub struct ArtifactSummaryPayload {
    pub action_label: String,
    pub description: String,
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing)]
    pub source_message_id: Option<String>,
    #[serde(skip_serializing)]
    pub source_run_id: String,
    pub status: &'static str,
    pub title: String,
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
