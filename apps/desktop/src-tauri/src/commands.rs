use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::future::Future;
use std::io::{Cursor, Write};
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use chrono::{NaiveDate, Utc};
use futures::StreamExt;
use harness_contracts::{
    AgentCapabilityKind, AgentCapabilityUnavailableReason, ConversationCursor,
    ConversationMessageAuthor, ConversationTurnCursor, ConversationWorktreePage, LocalIsolationTag,
    PluginCapabilitiesSummary, PluginConfigUpdate, PluginDetail, PluginId, PluginInstallReport,
    PluginOperationResult, PluginOperationStatus, PluginSummary, RejectionReason, SandboxMode,
    TrustLevel, UiSafeText,
};
use harness_plugin::{
    CargoExtensionManifestLoader, CargoExtensionRuntimeLoader, DiscoverySource, FileManifestLoader,
    InlineManifestLoader, ManifestOrigin, PluginConfig, PluginName, PluginRegistry,
};
use harness_sandbox::{LocalIsolation, SandboxBackend};
use image::{ImageFormat, ImageReader, Limits};
use jyowo_harness_sdk::builtin::{
    DefaultRedactor, FileBlobStore, InMemoryMemoryProvider, JsonlEventStore, LocalLlamaProvider,
    LocalSandbox,
};
use jyowo_harness_sdk::ext::inventory_from_models_api_json;
use jyowo_harness_sdk::ext::{
    build_provider, now, provider_catalog_entries, resolve_model_descriptor,
    runnable_inventory_models, AgentId, BlobMeta, BlobRef, BlobRetention, BlobStore,
    ConversationModelCapability, Decision, DecisionScope, DeltaChunk, DirectorySourceKind,
    EndReason, Event, EventId, HttpTransport, InteractivityLevel, McpConnectionState, McpEventSink,
    McpRegistry, McpServerId, McpServerScope, McpServerSource, McpServerSpec, MemoryId, MemoryKind,
    MemoryRecord, MemorySource, MemorySummary, MemoryVisibility, MessageContent, MessagePart,
    ModelDescriptor, ModelInventoryEntry, ModelLifecycle, ModelModality, ModelProtocol,
    ModelProvider, ModelRuntimeStatus, PendingPermissionRequest, PermissionMode, PermissionSubject,
    ProviderAuthScheme, ProviderBaseUrlRegion, ProviderBuildConfig, ProviderCredential,
    ProviderCredentialResolveContext, ProviderCredentialResolverCap, ProviderRegistryError,
    ProviderRuntimeCapability, ProviderServiceCapability, ProviderServiceCategory,
    ProviderServiceCostRisk, ProviderServiceExecution, RedactPatternSet, RedactRules, RedactScope,
    Redactor, RequestId, RunId, SessionId, Severity, SkillLoader, SkillSourceConfig, StdioEnv,
    StdioPolicy, StdioTransport, TenantId, ToolCapability, ToolError, ToolUseId, TransportChoice,
};
use jyowo_harness_sdk::{
    ConversationAttachmentReference, ConversationContextReference, ConversationEventsPageRequest,
    ConversationTurnInput, ConversationTurnPageDirection, ConversationTurnRequest, Harness,
    McpConfig, RuntimeSkillSummary, RuntimeSkillView, SessionOptions, StreamPermissionRuntime,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::project_registry::{ProjectRecord, ProjectRegistry};
use crate::skill_catalog::{
    get_skill_catalog_entry as get_catalog_entry_payload,
    get_skill_catalog_file as get_catalog_file_payload,
    list_skill_catalog_entries as list_catalog_entries_payload,
    list_skill_catalog_sources as list_catalog_sources_payload, mark_catalog_entry_name_conflict,
    materialize_skill_from_catalog_with_progress, GetSkillCatalogEntryRequest,
    GetSkillCatalogEntryResponse, GetSkillCatalogFileRequest, GetSkillCatalogFileResponse,
    InstallSkillFromCatalogRequest, ListSkillCatalogEntriesRequest,
    ListSkillCatalogEntriesResponse, ListSkillCatalogSourcesResponse, SkillInstallOriginRecord,
};

const START_RUN_STARTED_TIMEOUT: Duration = Duration::from_secs(5);
const WORKSPACE_ROOT_ENV: &str = "JYOWO_WORKSPACE_ROOT";
const MAX_MEMORY_CONTENT_BYTES: usize = 64 * 1024;
const MAX_ARTIFACT_PREVIEW_BYTES: usize = 16 * 1024;
const MAX_ARTIFACT_MEDIA_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ATTACHMENT_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ATTACHMENT_PREVIEW_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ATTACHMENT_PREVIEW_DIMENSION: u32 = 8192;
const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_OPENROUTER_MODELS_API_BYTES: usize = 4 * 1024 * 1024;
const MAX_SKILL_MARKDOWN_BYTES: u64 = 256 * 1024;
const MAX_SKILL_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_SKILL_PACKAGE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SKILL_PACKAGE_FILES: usize = 200;
const MAX_PLUGIN_PACKAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PLUGIN_PACKAGE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PLUGIN_PACKAGE_FILES: usize = 300;
const SKILL_PACKAGE_ENTRY_FILE: &str = "SKILL.md";
const CONVERSATION_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CONVERSATION_SUBSCRIPTION_BATCH_LIMIT: usize = 50;
const MCP_DIAGNOSTIC_RETENTION_LIMIT: usize = 500;
const MCP_DIAGNOSTIC_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MCP_DIAGNOSTIC_SUBSCRIPTION_BATCH_LIMIT: usize = 50;
const PROVIDER_API_KEY_REVEAL_TTL: Duration = Duration::from_secs(60);
const PLUGIN_RUNTIME_RUN_ID: &str = "plugin-runtime";
const PLUGIN_FAILURE_WITHHELD_MESSAGE: &str = "Plugin failure withheld from conversation timeline.";
const PLUGIN_REPORT_SOURCE_PATH_WITHHELD: &str = "<local-plugin>";
const LOCAL_PLUGIN_SIDECAR_REQUIRED_REASON: &str =
    "local plugin package must include a jyowo-plugin-* sidecar executable";

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

fn default_true() -> bool {
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
struct SkillCatalogInstallTaskKey {
    source_id: String,
    entry_id: String,
    version: Option<String>,
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

#[derive(Clone)]
struct DesktopProviderCredentialResolver {
    conversation_model_config_store: Arc<dyn ConversationModelConfigStore>,
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
}

impl DesktopProviderCredentialResolver {
    fn new(
        conversation_model_config_store: Arc<dyn ConversationModelConfigStore>,
        provider_settings_store: Arc<dyn ProviderSettingsStore>,
    ) -> Self {
        Self {
            conversation_model_config_store,
            provider_settings_store,
        }
    }
}

impl ProviderCredentialResolverCap for DesktopProviderCredentialResolver {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> futures::future::BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        Box::pin(async move {
            let record = self
                .provider_settings_store
                .load_record()
                .map_err(|error| ToolError::PermissionDenied(error.message))?
                .ok_or_else(|| {
                    ToolError::PermissionDenied(
                        "MiniMax provider config is not configured".to_owned(),
                    )
                })?;
            let bound_config_id = self
                .conversation_model_config_store
                .load_records()
                .map_err(|error| ToolError::PermissionDenied(error.message))?
                .get(&context.session_id.to_string())
                .cloned();
            let selected = bound_config_id
                .as_deref()
                .and_then(|config_id| {
                    record.configs.iter().find(|config| {
                        config.id == config_id && config.provider_id == context.provider_id
                    })
                })
                .or_else(|| {
                    record.default_config_id.as_deref().and_then(|config_id| {
                        record.configs.iter().find(|config| {
                            config.id == config_id && config.provider_id == context.provider_id
                        })
                    })
                })
                .ok_or_else(|| {
                    ToolError::PermissionDenied(
                        "MiniMax provider config is not configured".to_owned(),
                    )
                })?;
            if selected.api_key.trim().is_empty() {
                return Err(ToolError::PermissionDenied(
                    "MiniMax provider config has no api key".to_owned(),
                ));
            }
            Ok(ProviderCredential {
                provider_id: selected.provider_id.clone(),
                config_id: selected.id.clone(),
                api_key: selected.api_key.clone(),
                base_url: selected.base_url.clone(),
            })
        })
    }
}

#[derive(Clone)]
pub struct DesktopProviderSettingsStore {
    workspace_root: PathBuf,
}

impl DesktopProviderSettingsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("provider-settings.json")
    }
}

impl ProviderSettingsStore for DesktopProviderSettingsStore {
    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        ensure_no_symlink_components(&settings_path, "provider settings file")?;
        match std::fs::read(&settings_path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(record) => Ok(Some(record)),
                Err(_) => {
                    remove_invalid_provider_settings_file(&settings_path)?;
                    Ok(None)
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(runtime_operation_failed(format!(
                "provider settings read failed: {error}"
            ))),
        }
    }

    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
        ensure_provider_settings_record(record)?;
        let settings_path = self.settings_path();
        let parent = settings_path.parent().ok_or_else(|| {
            runtime_operation_failed("provider settings path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "provider settings directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("provider settings directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(parent, "provider settings directory")?;
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
            runtime_operation_failed(format!("provider settings serialization failed: {error}"))
        })?;
        let temp_path = settings_path.with_file_name(format!(
            "{}.{}.tmp",
            settings_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("provider-settings.json"),
            RunId::new()
        ));
        ensure_no_symlink_components(&temp_path, "provider settings temp file")?;
        let mut open_options = std::fs::OpenOptions::new();
        open_options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            open_options.mode(0o600);
        }
        let mut temp_file = open_options.open(&temp_path).map_err(|error| {
            runtime_operation_failed(format!("provider settings temp open failed: {error}"))
        })?;
        if let Err(error) = temp_file.write_all(&bytes) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "provider settings write failed: {error}"
            )));
        }
        if let Err(error) = temp_file.sync_all() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "provider settings sync failed: {error}"
            )));
        }
        drop(temp_file);
        ensure_no_symlink_components(&settings_path, "provider settings file")?;
        std::fs::rename(&temp_path, &settings_path).map_err(|error| {
            let _ = std::fs::remove_file(&temp_path);
            runtime_operation_failed(format!("provider settings commit failed: {error}"))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutionSettingsRecord {
    #[serde(default = "default_permission_mode")]
    pub permission_mode: PermissionMode,
    #[serde(default = "default_context_compression_trigger_ratio")]
    pub context_compression_trigger_ratio: f32,
    #[serde(default)]
    pub subagents_enabled: bool,
    #[serde(default)]
    pub agent_teams_enabled: bool,
    #[serde(default)]
    pub background_agents_enabled: bool,
}

fn default_permission_mode() -> PermissionMode {
    PermissionMode::Default
}

fn default_context_compression_trigger_ratio() -> f32 {
    0.8
}

impl Default for ExecutionSettingsRecord {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::Default,
            context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        }
    }
}

#[derive(Clone)]
pub struct DesktopExecutionSettingsStore {
    workspace_root: PathBuf,
}

impl DesktopExecutionSettingsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("execution-settings.json")
    }

    pub fn load_record(&self) -> Result<ExecutionSettingsRecord, CommandErrorPayload> {
        let settings_path = self.settings_path();
        ensure_no_symlink_components(&settings_path, "execution settings file")?;
        match std::fs::read(&settings_path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(record) => {
                    if ensure_execution_settings_record(&record).is_ok() {
                        Ok(record)
                    } else {
                        remove_invalid_execution_settings_file(&settings_path)?;
                        Ok(ExecutionSettingsRecord::default())
                    }
                }
                Err(_) => {
                    remove_invalid_execution_settings_file(&settings_path)?;
                    Ok(ExecutionSettingsRecord::default())
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ExecutionSettingsRecord::default())
            }
            Err(error) => Err(runtime_operation_failed(format!(
                "execution settings read failed: {error}"
            ))),
        }
    }

    pub fn save_record(&self, record: &ExecutionSettingsRecord) -> Result<(), CommandErrorPayload> {
        ensure_execution_settings_record(record)?;
        let settings_path = self.settings_path();
        let parent = settings_path.parent().ok_or_else(|| {
            runtime_operation_failed("execution settings path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "execution settings directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("execution settings directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(parent, "execution settings directory")?;
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
            runtime_operation_failed(format!("execution settings serialization failed: {error}"))
        })?;
        let temp_path = settings_path.with_file_name(format!(
            "{}.{}.tmp",
            settings_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("execution-settings.json"),
            RunId::new()
        ));
        ensure_no_symlink_components(&temp_path, "execution settings temp file")?;
        let mut temp_file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|error| {
                runtime_operation_failed(format!("execution settings temp open failed: {error}"))
            })?;
        if let Err(error) = temp_file.write_all(&bytes) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "execution settings write failed: {error}"
            )));
        }
        if let Err(error) = temp_file.sync_all() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "execution settings sync failed: {error}"
            )));
        }
        drop(temp_file);
        ensure_no_symlink_components(&settings_path, "execution settings file")?;
        std::fs::rename(&temp_path, &settings_path).map_err(|error| {
            let _ = std::fs::remove_file(&temp_path);
            runtime_operation_failed(format!("execution settings commit failed: {error}"))
        })
    }
}

fn remove_invalid_execution_settings_file(settings_path: &Path) -> Result<(), CommandErrorPayload> {
    match std::fs::remove_file(settings_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(runtime_operation_failed(format!(
            "execution settings cleanup failed: {error}"
        ))),
    }
}

fn ensure_execution_settings_record(
    record: &ExecutionSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    match record.permission_mode {
        PermissionMode::Default | PermissionMode::Auto | PermissionMode::BypassPermissions => {
            Ok(())
        }
        _ => Err(invalid_payload(
            "permissionMode must be default, auto, or bypass_permissions".to_owned(),
        )),
    }?;
    if !(0.5..=0.95).contains(&record.context_compression_trigger_ratio)
        || !record.context_compression_trigger_ratio.is_finite()
    {
        return Err(invalid_payload(
            "contextCompressionTriggerRatio must be between 0.5 and 0.95".to_owned(),
        ));
    }

    ensure_agent_capability_setting_available(
        record.subagents_enabled,
        agent_capabilities_available().subagents_available,
        "subagents",
    )?;
    ensure_agent_capability_setting_available(
        record.agent_teams_enabled,
        agent_capabilities_available().agent_teams_available,
        "agentTeams",
    )?;
    ensure_agent_capability_setting_available(
        record.background_agents_enabled,
        agent_capabilities_available().background_agents_available,
        "backgroundAgents",
    )
}

fn ensure_agent_capability_setting_available(
    enabled: bool,
    available: bool,
    capability: &str,
) -> Result<(), CommandErrorPayload> {
    if enabled && !available {
        return Err(invalid_payload(format!(
            "{capability} cannot be enabled in this desktop build"
        )));
    }
    Ok(())
}

fn auto_mode_available() -> bool {
    // The desktop shell does not currently assemble an AuxLlmBroker-backed
    // permission runtime. Keep Auto fail-closed even if the placeholder feature
    // is enabled.
    false
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesPayload {
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
    pub unavailable_reasons: Vec<AgentCapabilityUnavailableReason>,
}

fn agent_capabilities_payload(record: &ExecutionSettingsRecord) -> AgentCapabilitiesPayload {
    let availability = agent_capabilities_available();
    AgentCapabilitiesPayload {
        subagents_enabled: record.subagents_enabled,
        agent_teams_enabled: record.agent_teams_enabled,
        background_agents_enabled: record.background_agents_enabled,
        subagents_available: availability.subagents_available,
        agent_teams_available: availability.agent_teams_available,
        background_agents_available: availability.background_agents_available,
        unavailable_reasons: availability.unavailable_reasons,
    }
}

fn agent_capabilities_available() -> AgentCapabilitiesPayload {
    let subagents_available = false;
    let agent_teams_available = false;
    let background_agents_available = false;
    let mut unavailable_reasons = Vec::new();
    if !subagents_available {
        unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::Subagents,
        });
    }
    if !agent_teams_available {
        unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::AgentTeams,
        });
    }
    if !background_agents_available {
        unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::BackgroundAgents,
        });
    }

    AgentCapabilitiesPayload {
        subagents_enabled: false,
        agent_teams_enabled: false,
        background_agents_enabled: false,
        subagents_available,
        agent_teams_available,
        background_agents_available,
        unavailable_reasons,
    }
}

fn remove_invalid_provider_settings_file(settings_path: &Path) -> Result<(), CommandErrorPayload> {
    match std::fs::remove_file(settings_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(runtime_operation_failed(format!(
            "provider settings cleanup failed: {error}"
        ))),
    }
}

fn ensure_provider_settings_record(
    record: &ProviderSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    if record.configs.is_empty() {
        if record.default_config_id.is_some() {
            return Err(runtime_operation_failed(
                "defaultConfigId requires at least one provider config".to_owned(),
            ));
        }
        return Ok(());
    }

    let Some(default_config_id) = record
        .default_config_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(runtime_operation_failed(
            "defaultConfigId is required when provider configs exist".to_owned(),
        ));
    };
    if !record
        .configs
        .iter()
        .any(|config| config.id == default_config_id)
    {
        return Err(runtime_operation_failed(
            "defaultConfigId must reference an existing provider config".to_owned(),
        ));
    }
    if record
        .configs
        .iter()
        .any(|config| config.api_key.trim().is_empty())
    {
        return Err(runtime_operation_failed(
            "apiKey is required for every provider config".to_owned(),
        ));
    }

    Ok(())
}

#[derive(Clone)]
pub struct DesktopConversationModelConfigStore {
    workspace_root: PathBuf,
}

impl DesktopConversationModelConfigStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("conversation-model-settings.json")
    }
}

impl ConversationModelConfigStore for DesktopConversationModelConfigStore {
    fn load_records(&self) -> Result<HashMap<String, String>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        ensure_no_symlink_components(&settings_path, "conversation model settings file")?;
        match std::fs::read(&settings_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                runtime_operation_failed(format!(
                    "conversation model settings parse failed: {error}"
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(error) => Err(runtime_operation_failed(format!(
                "conversation model settings read failed: {error}"
            ))),
        }
    }

    fn save_records(&self, records: &HashMap<String, String>) -> Result<(), CommandErrorPayload> {
        let settings_path = self.settings_path();
        let parent = settings_path.parent().ok_or_else(|| {
            runtime_operation_failed("conversation model settings path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "conversation model settings directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!(
                "conversation model settings directory unavailable: {error}"
            ))
        })?;
        ensure_no_symlink_components(parent, "conversation model settings directory")?;
        let bytes = serde_json::to_vec_pretty(records).map_err(|error| {
            runtime_operation_failed(format!(
                "conversation model settings serialization failed: {error}"
            ))
        })?;
        let temp_path = settings_path.with_file_name(format!(
            "{}.{}.tmp",
            settings_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("conversation-model-settings.json"),
            RunId::new()
        ));
        ensure_no_symlink_components(&temp_path, "conversation model settings temp file")?;
        let mut temp_file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|error| {
                runtime_operation_failed(format!(
                    "conversation model settings temp open failed: {error}"
                ))
            })?;
        if let Err(error) = temp_file.write_all(&bytes) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "conversation model settings write failed: {error}"
            )));
        }
        if let Err(error) = temp_file.sync_all() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "conversation model settings sync failed: {error}"
            )));
        }
        drop(temp_file);
        ensure_no_symlink_components(&settings_path, "conversation model settings file")?;
        std::fs::rename(&temp_path, &settings_path).map_err(|error| {
            let _ = std::fs::remove_file(&temp_path);
            runtime_operation_failed(format!("conversation model settings save failed: {error}"))
        })
    }
}

#[derive(Clone)]
struct DesktopMcpServerStore {
    workspace_root: PathBuf,
}

impl DesktopMcpServerStore {
    fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("mcp-servers.json")
    }
}

impl McpServerStore for DesktopMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        match std::fs::read(&settings_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                runtime_operation_failed(format!("mcp server settings parse failed: {error}"))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(runtime_operation_failed(format!(
                "mcp server settings read failed: {error}"
            ))),
        }
    }

    fn save_record(&self, record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.retain(|existing| existing.id != record.id);
        records.push(record.clone());
        records.sort_by(|left, right| left.id.cmp(&right.id));
        write_mcp_server_records(&self.settings_path(), &records)
    }

    fn delete_record(&self, id: &str) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.retain(|existing| existing.id != id);
        write_mcp_server_records(&self.settings_path(), &records)
    }
}

#[derive(Clone)]
pub struct DesktopMcpDiagnosticStore {
    retention_limit: usize,
    workspace_root: PathBuf,
}

impl DesktopMcpDiagnosticStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::new_with_limit(workspace_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        Self {
            retention_limit,
            workspace_root,
        }
    }

    fn diagnostics_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("mcp-diagnostics.jsonl")
    }
}

impl McpDiagnosticStore for DesktopMcpDiagnosticStore {
    fn load_records(&self) -> Result<Vec<McpDiagnosticRecord>, CommandErrorPayload> {
        let diagnostics_path = self.diagnostics_path();
        let content = match std::fs::read_to_string(&diagnostics_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "mcp diagnostics read failed: {error}"
                )));
            }
        };

        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<McpDiagnosticRecord>(line).map_err(|error| {
                    runtime_operation_failed(format!("mcp diagnostics parse failed: {error}"))
                })
            })
            .collect()
    }

    fn append_record(&self, record: &McpDiagnosticRecord) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.push(record.clone());
        let keep_from = records.len().saturating_sub(self.retention_limit);
        if keep_from > 0 {
            records.drain(0..keep_from);
        }
        write_mcp_diagnostic_records(&self.diagnostics_path(), &records)
    }

    fn clear_records(&self, server_id: Option<&str>) -> Result<(), CommandErrorPayload> {
        let records = match server_id {
            Some(server_id) => self
                .load_records()?
                .into_iter()
                .filter(|record| record.server_id != server_id)
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        write_mcp_diagnostic_records(&self.diagnostics_path(), &records)
    }
}

#[derive(Clone)]
pub struct DesktopPluginStore {
    workspace_root: PathBuf,
}

impl DesktopPluginStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn root_dir(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("plugins")
    }

    fn index_path(&self) -> PathBuf {
        self.root_dir().join("index.json")
    }

    fn package_dir(&self, package_dir: &str) -> PathBuf {
        self.package_root().join(package_dir)
    }
}

impl PluginStore for DesktopPluginStore {
    fn package_root(&self) -> PathBuf {
        self.root_dir().join("user")
    }

    fn cargo_extension_root(&self) -> PathBuf {
        self.root_dir().join("extensions")
    }

    fn workspace_plugin_root(&self) -> PathBuf {
        self.workspace_root.join(".jyowo").join("plugins")
    }

    fn load_record(&self) -> Result<PluginSettingsRecord, CommandErrorPayload> {
        let index_path = self.index_path();
        ensure_no_symlink_components(&index_path, "plugin index file")?;
        match std::fs::read(&index_path) {
            Ok(bytes) => {
                let record =
                    serde_json::from_slice::<PluginSettingsRecord>(&bytes).map_err(|error| {
                        runtime_operation_failed(format!("plugin index parse failed: {error}"))
                    })?;
                ensure_plugin_settings_record(&record)?;
                Ok(record)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PluginSettingsRecord::default())
            }
            Err(error) => Err(runtime_operation_failed(format!(
                "plugin index read failed: {error}"
            ))),
        }
    }

    fn save_record(&self, record: &PluginSettingsRecord) -> Result<(), CommandErrorPayload> {
        ensure_plugin_settings_record(record)?;
        write_plugin_settings_record(&self.index_path(), record)
    }

    fn write_plugin_package(
        &self,
        package_dir: &str,
        source_path: &Path,
    ) -> Result<(), CommandErrorPayload> {
        ensure_plugin_package_dir_name(package_dir)?;
        let destination = self.package_dir(package_dir);
        let parent = destination.parent().ok_or_else(|| {
            runtime_operation_failed("plugin package path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "plugin package directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("plugin package directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(parent, "plugin package directory")?;
        copy_plugin_package(source_path, &destination)
    }

    fn delete_plugin_package(&self, package_dir: &str) -> Result<(), CommandErrorPayload> {
        ensure_plugin_package_dir_name(package_dir)?;
        let path = self.package_dir(package_dir);
        ensure_no_symlink_components(&path, "plugin package")?;
        let root = self.package_root();
        let normalized_root = match root.canonicalize() {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "plugin package root unavailable: {error}"
                )));
            }
        };
        let normalized_path = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "plugin package unavailable: {error}"
                )));
            }
        };
        if normalized_path == normalized_root || !normalized_path.starts_with(&normalized_root) {
            return Err(invalid_payload(
                "plugin package path escaped package root".to_owned(),
            ));
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(runtime_operation_failed(format!(
                "plugin package delete failed: {error}"
            ))),
        }
    }
}

#[derive(Clone)]
pub struct DesktopSkillStore {
    workspace_root: PathBuf,
}

impl DesktopSkillStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn root_dir(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("skills")
    }

    fn index_path(&self) -> PathBuf {
        self.root_dir().join("index.json")
    }

    fn disabled_dir(&self) -> PathBuf {
        self.root_dir().join("disabled")
    }

    fn skill_dir(&self, id: &str, enabled: bool) -> PathBuf {
        let dir = if enabled {
            self.enabled_dir()
        } else {
            self.disabled_dir()
        };
        dir.join(id)
    }

    fn legacy_skill_file_path(&self, id: &str, enabled: bool) -> PathBuf {
        let dir = if enabled {
            self.enabled_dir()
        } else {
            self.disabled_dir()
        };
        dir.join(format!("{id}.md"))
    }
}

impl SkillStore for DesktopSkillStore {
    fn enabled_dir(&self) -> PathBuf {
        self.root_dir().join("enabled")
    }

    fn load_records(&self) -> Result<Vec<SkillStoreRecord>, CommandErrorPayload> {
        let index_path = self.index_path();
        ensure_no_symlink_components(&index_path, "skill index file")?;
        match std::fs::read(&index_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                runtime_operation_failed(format!("skill index parse failed: {error}"))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(runtime_operation_failed(format!(
                "skill index read failed: {error}"
            ))),
        }
    }

    fn save_records(&self, records: &[SkillStoreRecord]) -> Result<(), CommandErrorPayload> {
        write_skill_records(&self.index_path(), records)
    }

    fn write_skill_package(
        &self,
        id: &str,
        enabled: bool,
        source_path: &Path,
    ) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        let path = self.skill_dir(id, enabled);
        let parent = path.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "skill directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("skill directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(parent, "skill directory")?;
        ensure_no_symlink_components(&path, "skill package")?;
        copy_skill_package(source_path, &path)
    }

    fn read_skill_entry_file(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<String, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let path = self
            .skill_dir(&record.id, record.enabled)
            .join(SKILL_PACKAGE_ENTRY_FILE);
        ensure_no_symlink_components(&path, "skill entry file")?;
        if path.exists() {
            return std::fs::read_to_string(&path).map_err(|error| {
                runtime_operation_failed(format!("skill entry file read failed: {error}"))
            });
        }
        let path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&path, "legacy skill file")?;
        std::fs::read_to_string(&path).map_err(|error| {
            runtime_operation_failed(format!("legacy skill file read failed: {error}"))
        })
    }

    fn list_skill_package_files(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<Vec<SkillFilePayload>, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let package_root = self.skill_dir(&record.id, record.enabled);
        ensure_no_symlink_components(&package_root, "skill package")?;
        if package_root.is_dir() {
            return list_skill_package_files(&package_root);
        }
        let legacy_path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&legacy_path, "legacy skill file")?;
        if legacy_path.is_file() {
            let metadata = std::fs::metadata(&legacy_path).map_err(|error| {
                runtime_operation_failed(format!("legacy skill file metadata failed: {error}"))
            })?;
            return Ok(vec![SkillFilePayload {
                path: SKILL_PACKAGE_ENTRY_FILE.to_owned(),
                name: SKILL_PACKAGE_ENTRY_FILE.to_owned(),
                kind: "file",
                depth: 0,
                size_bytes: Some(metadata.len()),
            }]);
        }
        Ok(Vec::new())
    }

    fn read_skill_package_file(
        &self,
        record: &SkillStoreRecord,
        relative_path: &str,
    ) -> Result<SkillFileContentPayload, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let relative_path = normalize_skill_relative_path(relative_path)?;
        let package_root = self.skill_dir(&record.id, record.enabled);
        ensure_no_symlink_components(&package_root, "skill package")?;
        if package_root.is_dir() {
            return read_skill_package_file(&package_root, &relative_path);
        }
        if path_to_workspace_string(&relative_path) != SKILL_PACKAGE_ENTRY_FILE {
            return Err(invalid_payload("skill file not found".to_owned()));
        }
        let legacy_path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&legacy_path, "legacy skill file")?;
        read_skill_package_file_at(&legacy_path, SKILL_PACKAGE_ENTRY_FILE)
    }

    fn move_skill_package(&self, id: &str, enabled: bool) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        let from = self.skill_dir(id, !enabled);
        let to = self.skill_dir(id, enabled);
        let parent = to.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "skill directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("skill directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(&from, "skill package")?;
        ensure_no_symlink_components(&to, "skill package")?;
        if from.exists() {
            std::fs::rename(&from, &to).map_err(|error| {
                runtime_operation_failed(format!("skill package move failed: {error}"))
            })?;
            return Ok(());
        }
        let from = self.legacy_skill_file_path(id, !enabled);
        let to = self.legacy_skill_file_path(id, enabled);
        ensure_no_symlink_components(&from, "legacy skill file")?;
        ensure_no_symlink_components(&to, "legacy skill file")?;
        if from.exists() {
            std::fs::rename(&from, &to).map_err(|error| {
                runtime_operation_failed(format!("legacy skill file move failed: {error}"))
            })?;
        }
        Ok(())
    }

    fn delete_skill_package(&self, id: &str) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        for enabled in [true, false] {
            let path = self.skill_dir(id, enabled);
            ensure_no_symlink_components(&path, "skill package")?;
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(runtime_operation_failed(format!(
                        "skill package delete failed: {error}"
                    )));
                }
            }
            let legacy_path = self.legacy_skill_file_path(id, enabled);
            ensure_no_symlink_components(&legacy_path, "legacy skill file")?;
            match std::fs::remove_file(&legacy_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(runtime_operation_failed(format!(
                        "legacy skill file delete failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}

fn write_mcp_server_records(
    settings_path: &Path,
    records: &[McpServerConfigRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = settings_path.parent().ok_or_else(|| {
        runtime_operation_failed("mcp server settings path has no parent".to_owned())
    })?;
    ensure_no_symlink_components(parent, "mcp server settings directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!(
            "mcp server settings directory unavailable: {error}"
        ))
    })?;
    ensure_no_symlink_components(parent, "mcp server settings directory")?;
    let bytes = serde_json::to_vec_pretty(records).map_err(|error| {
        runtime_operation_failed(format!("mcp server settings serialization failed: {error}"))
    })?;
    let temp_path = settings_path.with_file_name(format!(
        "{}.{}.tmp",
        settings_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mcp-servers.json"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "mcp server settings temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("mcp server settings temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp server settings write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp server settings sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(settings_path, "mcp server settings file")?;
    std::fs::rename(&temp_path, settings_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("mcp server settings commit failed: {error}"))
    })
}

fn write_mcp_diagnostic_records(
    diagnostics_path: &Path,
    records: &[McpDiagnosticRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = diagnostics_path
        .parent()
        .ok_or_else(|| runtime_operation_failed("mcp diagnostics path has no parent".to_owned()))?;
    ensure_no_symlink_components(parent, "mcp diagnostics directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!("mcp diagnostics directory unavailable: {error}"))
    })?;
    ensure_no_symlink_components(parent, "mcp diagnostics directory")?;
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).map_err(|error| {
            runtime_operation_failed(format!("mcp diagnostics serialization failed: {error}"))
        })?;
        bytes.push(b'\n');
    }
    let temp_path = diagnostics_path.with_file_name(format!(
        "{}.{}.tmp",
        diagnostics_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mcp-diagnostics.jsonl"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "mcp diagnostics temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("mcp diagnostics temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp diagnostics write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp diagnostics sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(diagnostics_path, "mcp diagnostics file")?;
    std::fs::rename(&temp_path, diagnostics_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("mcp diagnostics commit failed: {error}"))
    })
}

fn write_skill_records(
    index_path: &Path,
    records: &[SkillStoreRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = index_path
        .parent()
        .ok_or_else(|| runtime_operation_failed("skill index path has no parent".to_owned()))?;
    ensure_no_symlink_components(parent, "skill index directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!("skill index directory unavailable: {error}"))
    })?;
    ensure_no_symlink_components(parent, "skill index directory")?;
    let bytes = serde_json::to_vec_pretty(records).map_err(|error| {
        runtime_operation_failed(format!("skill index serialization failed: {error}"))
    })?;
    let temp_path = index_path.with_file_name(format!(
        "{}.{}.tmp",
        index_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("index.json"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "skill index temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("skill index temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "skill index write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "skill index sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(index_path, "skill index file")?;
    std::fs::rename(&temp_path, index_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("skill index commit failed: {error}"))
    })
}

fn write_plugin_settings_record(
    index_path: &Path,
    record: &PluginSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    let parent = index_path
        .parent()
        .ok_or_else(|| runtime_operation_failed("plugin index path has no parent".to_owned()))?;
    ensure_no_symlink_components(parent, "plugin index directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!("plugin index directory unavailable: {error}"))
    })?;
    ensure_no_symlink_components(parent, "plugin index directory")?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
        runtime_operation_failed(format!("plugin index serialization failed: {error}"))
    })?;
    let temp_path = index_path.with_file_name(format!(
        "{}.{}.tmp",
        index_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("index.json"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "plugin index temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("plugin index temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "plugin index write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "plugin index sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(index_path, "plugin index file")?;
    std::fs::rename(&temp_path, index_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("plugin index commit failed: {error}"))
    })
}

fn ensure_skill_id(id: &str) -> Result<(), CommandErrorPayload> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(invalid_payload("skill id is invalid".to_owned()));
    }
    Ok(())
}

fn ensure_plugin_package_dir_name(value: &str) -> Result<(), CommandErrorPayload> {
    if value.is_empty()
        || value.len() > 128
        || value.starts_with('.')
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(invalid_payload(
            "plugin package directory is invalid".to_owned(),
        ));
    }
    let mut components = Path::new(value).components();
    if !matches!(components.next(), Some(Component::Normal(component)) if component == OsStr::new(value))
        || components.next().is_some()
    {
        return Err(invalid_payload(
            "plugin package directory is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_plugin_store_record(record: &PluginStoreRecord) -> Result<(), CommandErrorPayload> {
    ensure_plugin_package_dir_name(&record.package_dir)?;
    let expected_package_dir = plugin_package_dir_name(&record.plugin_id);
    if record.package_dir != expected_package_dir {
        return Err(invalid_payload(
            "plugin package directory does not match plugin id".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_plugin_settings_record(record: &PluginSettingsRecord) -> Result<(), CommandErrorPayload> {
    for plugin in &record.records {
        ensure_plugin_store_record(plugin)?;
    }
    Ok(())
}

fn ensure_no_symlink_components(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use symlinks"
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} metadata unavailable: {error}"
                )));
            }
        }
    }

    Ok(())
}

fn hash_skill_package(source_path: &Path) -> Result<String, CommandErrorPayload> {
    let mut hasher = blake3::Hasher::new();
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    hash_skill_package_dir(
        source_path,
        source_path,
        &mut hasher,
        &mut file_count,
        &mut total_bytes,
    )?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn hash_skill_package_dir(
    root: &Path,
    dir: &Path,
    hasher: &mut blake3::Hasher,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "skill package directory")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "skill package must not use symlinks".to_owned(),
            ));
        }
        if metadata.is_dir() {
            hash_skill_package_dir(root, &path, hasher, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "skill package may contain only files and directories".to_owned(),
            ));
        }
        *file_count += 1;
        if *file_count > MAX_SKILL_PACKAGE_FILES {
            return Err(invalid_payload(
                "skill package has too many files".to_owned(),
            ));
        }
        if metadata.len() > MAX_SKILL_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_SKILL_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        let bytes = std::fs::read(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package file read failed: {error}"))
        })?;
        hasher.update(path_to_workspace_string(relative_path).as_bytes());
        hasher.update(&[0]);
        hasher.update(&bytes);
        hasher.update(&[0]);
    }
    Ok(())
}

fn copy_skill_package(
    source_path: &Path,
    destination_path: &Path,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(source_path, "skill source package")?;
    ensure_no_symlink_components(destination_path, "skill package")?;
    let _ = hash_skill_package(source_path)?;
    if destination_path.exists() {
        std::fs::remove_dir_all(destination_path).map_err(|error| {
            runtime_operation_failed(format!("skill package cleanup failed: {error}"))
        })?;
    }
    std::fs::create_dir_all(destination_path).map_err(|error| {
        runtime_operation_failed(format!("skill package directory unavailable: {error}"))
    })?;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    match copy_skill_package_dir(
        source_path,
        source_path,
        destination_path,
        &mut file_count,
        &mut total_bytes,
    ) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_dir_all(destination_path);
            Err(error)
        }
    }
}

fn copy_skill_package_dir(
    root: &Path,
    dir: &Path,
    destination_root: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "skill source package")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "skill package must not use symlinks".to_owned(),
            ));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        let destination_path = destination_root.join(relative_path);
        ensure_no_symlink_components(&destination_path, "skill package")?;
        if metadata.is_dir() {
            std::fs::create_dir_all(&destination_path).map_err(|error| {
                runtime_operation_failed(format!("skill package directory copy failed: {error}"))
            })?;
            copy_skill_package_dir(root, &path, destination_root, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "skill package may contain only files and directories".to_owned(),
            ));
        }
        *file_count += 1;
        if *file_count > MAX_SKILL_PACKAGE_FILES {
            return Err(invalid_payload(
                "skill package has too many files".to_owned(),
            ));
        }
        if metadata.len() > MAX_SKILL_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_SKILL_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        let parent = destination_path.parent().ok_or_else(|| {
            runtime_operation_failed("skill package file path has no parent".to_owned())
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("skill package directory copy failed: {error}"))
        })?;
        std::fs::copy(&path, &destination_path).map_err(|error| {
            runtime_operation_failed(format!("skill package file copy failed: {error}"))
        })?;
    }
    Ok(())
}

fn hash_plugin_package(source_path: &Path) -> Result<String, CommandErrorPayload> {
    let mut hasher = blake3::Hasher::new();
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    hash_plugin_package_dir(
        source_path,
        source_path,
        &mut hasher,
        &mut file_count,
        &mut total_bytes,
    )?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn hash_plugin_package_dir(
    root: &Path,
    dir: &Path,
    hasher: &mut blake3::Hasher,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "plugin package directory")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("plugin package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            runtime_operation_failed(format!("plugin package read failed: {error}"))
        })?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("plugin package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "plugin package must not use symlinks".to_owned(),
            ));
        }
        if metadata.is_dir() {
            hash_plugin_package_dir(root, &path, hasher, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "plugin package may contain only files and directories".to_owned(),
            ));
        }
        *file_count += 1;
        if *file_count > MAX_PLUGIN_PACKAGE_FILES {
            return Err(invalid_payload(
                "plugin package has too many files".to_owned(),
            ));
        }
        if metadata.len() > MAX_PLUGIN_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "plugin package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_PLUGIN_PACKAGE_BYTES {
            return Err(invalid_payload("plugin package is too large".to_owned()));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("plugin package path escaped its root".to_owned())
        })?;
        let bytes = std::fs::read(&path).map_err(|error| {
            runtime_operation_failed(format!("plugin package file read failed: {error}"))
        })?;
        hasher.update(path_to_workspace_string(relative_path).as_bytes());
        hasher.update(&[0]);
        hasher.update(&bytes);
        hasher.update(&[0]);
    }
    Ok(())
}

fn copy_plugin_package(
    source_path: &Path,
    destination_path: &Path,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(source_path, "plugin source package")?;
    ensure_no_symlink_components(destination_path, "plugin package")?;
    let _ = hash_plugin_package(source_path)?;
    if destination_path.exists() {
        std::fs::remove_dir_all(destination_path).map_err(|error| {
            runtime_operation_failed(format!("plugin package cleanup failed: {error}"))
        })?;
    }
    std::fs::create_dir_all(destination_path).map_err(|error| {
        runtime_operation_failed(format!("plugin package directory unavailable: {error}"))
    })?;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    match copy_plugin_package_dir(
        source_path,
        source_path,
        destination_path,
        &mut file_count,
        &mut total_bytes,
    ) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_dir_all(destination_path);
            Err(error)
        }
    }
}

fn copy_plugin_package_dir(
    root: &Path,
    dir: &Path,
    destination_root: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "plugin source package")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("plugin package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            runtime_operation_failed(format!("plugin package read failed: {error}"))
        })?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("plugin package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "plugin package must not use symlinks".to_owned(),
            ));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("plugin package path escaped its root".to_owned())
        })?;
        let destination_path = destination_root.join(relative_path);
        ensure_no_symlink_components(&destination_path, "plugin package")?;
        if metadata.is_dir() {
            std::fs::create_dir_all(&destination_path).map_err(|error| {
                runtime_operation_failed(format!("plugin package directory copy failed: {error}"))
            })?;
            copy_plugin_package_dir(root, &path, destination_root, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "plugin package may contain only files and directories".to_owned(),
            ));
        }
        *file_count += 1;
        if *file_count > MAX_PLUGIN_PACKAGE_FILES {
            return Err(invalid_payload(
                "plugin package has too many files".to_owned(),
            ));
        }
        if metadata.len() > MAX_PLUGIN_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "plugin package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_PLUGIN_PACKAGE_BYTES {
            return Err(invalid_payload("plugin package is too large".to_owned()));
        }
        let parent = destination_path.parent().ok_or_else(|| {
            runtime_operation_failed("plugin package file path has no parent".to_owned())
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("plugin package directory copy failed: {error}"))
        })?;
        std::fs::copy(&path, &destination_path).map_err(|error| {
            runtime_operation_failed(format!("plugin package file copy failed: {error}"))
        })?;
    }
    Ok(())
}

fn list_skill_package_files(
    package_root: &Path,
) -> Result<Vec<SkillFilePayload>, CommandErrorPayload> {
    let mut files = Vec::new();
    collect_skill_package_files(package_root, package_root, &mut files)?;
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.cmp(right.kind))
    });
    Ok(files)
}

fn collect_skill_package_files(
    package_root: &Path,
    dir: &Path,
    files: &mut Vec<SkillFilePayload>,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "skill package directory")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "skill package must not use symlinks".to_owned(),
            ));
        }
        let relative_path = path.strip_prefix(package_root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        let normalized_path = path_to_workspace_string(relative_path);
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| normalized_path.clone());
        let depth = relative_path.components().count().saturating_sub(1) as u32;
        if metadata.is_dir() {
            files.push(SkillFilePayload {
                path: normalized_path,
                name,
                kind: "directory",
                depth,
                size_bytes: None,
            });
            collect_skill_package_files(package_root, &path, files)?;
        } else if metadata.is_file() {
            files.push(SkillFilePayload {
                path: normalized_path,
                name,
                kind: "file",
                depth,
                size_bytes: Some(metadata.len()),
            });
        } else {
            return Err(invalid_payload(
                "skill package may contain only files and directories".to_owned(),
            ));
        }
    }
    Ok(())
}

fn read_skill_package_file(
    package_root: &Path,
    relative_path: &Path,
) -> Result<SkillFileContentPayload, CommandErrorPayload> {
    let display_path = path_to_workspace_string(relative_path);
    let path = package_root.join(relative_path);
    ensure_no_symlink_components(&path, "skill package file")?;
    let normalized_root = package_root.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("skill package path unavailable: {error}"))
    })?;
    let normalized_path = path.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("skill package file unavailable: {error}"))
    })?;
    if !normalized_path.starts_with(normalized_root) {
        return Err(invalid_payload(
            "skill file path escaped package".to_owned(),
        ));
    }
    read_skill_package_file_at(&path, &display_path)
}

fn read_skill_package_file_at(
    path: &Path,
    display_path: &str,
) -> Result<SkillFileContentPayload, CommandErrorPayload> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        runtime_operation_failed(format!("skill package file metadata failed: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(runtime_operation_failed(
            "skill package must not use symlinks".to_owned(),
        ));
    }
    if !metadata.is_file() {
        return Err(invalid_payload("skill file not found".to_owned()));
    }
    if metadata.len() > MAX_SKILL_PACKAGE_FILE_BYTES {
        return Err(invalid_payload(
            "skill package file is too large".to_owned(),
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        runtime_operation_failed(format!("skill package file read failed: {error}"))
    })?;
    let content = String::from_utf8(bytes)
        .map_err(|_| invalid_payload("skill package file must be valid UTF-8".to_owned()))?;
    Ok(SkillFileContentPayload {
        path: display_path.to_owned(),
        content,
    })
}

fn normalize_skill_relative_path(value: &str) -> Result<PathBuf, CommandErrorPayload> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_payload("skill file path is required".to_owned()));
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Err(invalid_payload(
            "skill file path must be relative".to_owned(),
        ));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => normalized.push(value),
            _ => return Err(invalid_payload("skill file path is invalid".to_owned())),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(invalid_payload("skill file path is required".to_owned()));
    }
    Ok(normalized)
}

#[derive(Clone)]
pub struct DesktopRuntimeState {
    active_runtime: Arc<RwLock<DesktopActiveRuntime>>,
    conversation_model_config_lock: Arc<tokio::sync::Mutex<()>>,
    conversation_model_config_store: Arc<dyn ConversationModelConfigStore>,
    conversation_event_subscriptions:
        Arc<tokio::sync::Mutex<HashMap<String, ConversationSubscriptionHandle>>>,
    default_conversation_id: SessionId,
    deleted_conversation_ids: Arc<tokio::sync::Mutex<HashSet<SessionId>>>,
    memory_lock: Arc<tokio::sync::Mutex<()>>,
    mcp_diagnostic_store: Arc<dyn McpDiagnosticStore>,
    mcp_diagnostic_subscriptions:
        Arc<tokio::sync::Mutex<HashMap<String, McpDiagnosticSubscriptionHandle>>>,
    mcp_server_lock: Arc<tokio::sync::Mutex<()>>,
    mcp_server_store: Arc<dyn McpServerStore>,
    permission_resolver: Option<Arc<dyn PermissionResolver>>,
    provider_api_key_reveal_tokens:
        Arc<tokio::sync::Mutex<HashMap<String, ProviderConfigRevealTokenRecord>>>,
    plugin_store: Arc<dyn PluginStore>,
    plugin_store_lock: Arc<tokio::sync::Mutex<()>>,
    provider_settings_lock: Arc<tokio::sync::Mutex<()>>,
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
    execution_settings_lock: Arc<tokio::sync::Mutex<()>>,
    execution_settings_store: Arc<DesktopExecutionSettingsStore>,
    skill_catalog_install_tasks:
        Arc<RwLock<HashMap<SkillCatalogInstallTaskKey, SkillCatalogInstallTaskPayload>>>,
    skill_store: Arc<dyn SkillStore>,
    skill_store_lock: Arc<tokio::sync::Mutex<()>>,
    start_run_lock: Arc<tokio::sync::Mutex<()>>,
    stream_permission_runtime: Option<Arc<StreamPermissionRuntime>>,
    workspace_root: PathBuf,
}

#[derive(Clone)]
struct ProviderConfigRevealTokenRecord {
    api_key_fingerprint: [u8; 32],
    config_id: String,
    expires_at: Instant,
}

#[derive(Clone)]
struct DesktopActiveRuntime {
    default_model_id: String,
    default_protocol: ModelProtocol,
    harness: Option<Arc<Harness>>,
}

struct ConversationSubscriptionHandle {
    conversation_id: String,
    task: JoinHandle<()>,
    window_label: String,
}

struct McpDiagnosticSubscriptionHandle {
    task: JoinHandle<()>,
    window_label: String,
}

impl DesktopRuntimeState {
    pub fn with_workspace_for_test(workspace_root: PathBuf) -> Result<Self, CommandErrorPayload> {
        let workspace_root = canonical_workspace_root(workspace_root, "workspace root".to_owned())?;

        Ok(Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_id: "llama3.1".to_owned(),
                default_protocol: ModelProtocol::ChatCompletions,
                harness: None,
            })),
            conversation_model_config_lock: Arc::new(tokio::sync::Mutex::new(())),
            conversation_model_config_store: Arc::new(DesktopConversationModelConfigStore::new(
                workspace_root.clone(),
            )),
            conversation_event_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            default_conversation_id: SessionId::new(),
            deleted_conversation_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.clone())),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::new(workspace_root.clone())),
            permission_resolver: None,
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::new(workspace_root.clone())),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::new(
                workspace_root.clone(),
            )),
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(DesktopExecutionSettingsStore::new(
                workspace_root.clone(),
            )),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::new(workspace_root.clone())),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: None,
            workspace_root,
        })
    }

    pub fn with_harness_and_stream_permission_runtime(
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        Self::with_harness_stream_permission_runtime_for_workspace(
            current_process_workspace_root()?,
            harness,
            stream_permission_runtime,
        )
    }

    pub fn with_harness_and_stream_permission_runtime_for_workspace(
        workspace_root: PathBuf,
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        Self::with_harness_stream_permission_runtime_for_workspace(
            canonical_workspace_root(workspace_root, "workspace root".to_owned())?,
            harness,
            stream_permission_runtime,
        )
    }

    fn with_harness_stream_permission_runtime_for_workspace(
        workspace_root: PathBuf,
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        let provider = harness.model_provider();
        let mut default_model_id = harness.options().model_id.clone();
        let supported_models = provider.supported_models();
        if !supported_models
            .iter()
            .any(|model| model.model_id == default_model_id)
        {
            if let Some(model) = supported_models.first() {
                default_model_id = model.model_id.clone();
            }
        }
        let default_protocol = provider.snapshot_for_model(&default_model_id).protocol;
        Self::with_harness_stream_permission_runtime_and_model_for_workspace(
            workspace_root,
            harness,
            stream_permission_runtime,
            default_model_id,
            default_protocol,
        )
    }

    fn with_harness_stream_permission_runtime_and_model_for_workspace(
        workspace_root: PathBuf,
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
        default_model_id: String,
        default_protocol: ModelProtocol,
    ) -> Result<Self, CommandErrorPayload> {
        let Some(permission_broker) = harness.permission_broker() else {
            return Err(runtime_unavailable(
                "Permission decisions require a Harness PermissionBroker.",
            ));
        };
        if !Arc::ptr_eq(&permission_broker, &stream_permission_runtime.broker()) {
            return Err(runtime_unavailable(
                "Harness PermissionBroker must come from the stream permission runtime.",
            ));
        }
        let permission_resolver: Arc<dyn PermissionResolver> = stream_permission_runtime.clone();

        Ok(Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_id,
                default_protocol,
                harness: Some(harness),
            })),
            conversation_model_config_lock: Arc::new(tokio::sync::Mutex::new(())),
            conversation_model_config_store: Arc::new(DesktopConversationModelConfigStore::new(
                workspace_root.clone(),
            )),
            conversation_event_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            default_conversation_id: SessionId::new(),
            deleted_conversation_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.clone())),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::new(workspace_root.clone())),
            permission_resolver: Some(permission_resolver),
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::new(workspace_root.clone())),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::new(
                workspace_root.clone(),
            )),
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(DesktopExecutionSettingsStore::new(
                workspace_root.clone(),
            )),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::new(workspace_root.clone())),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: Some(stream_permission_runtime),
            workspace_root,
        })
    }

    #[must_use]
    pub fn harness(&self) -> Option<Arc<Harness>> {
        self.active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned")
            .harness
            .as_ref()
            .map(Arc::clone)
    }

    pub fn replace_harness(
        &self,
        harness: Arc<Harness>,
        default_model_id: String,
        default_protocol: ModelProtocol,
    ) {
        *self
            .active_runtime
            .write()
            .expect("desktop active runtime lock should not be poisoned") = DesktopActiveRuntime {
            default_model_id,
            default_protocol,
            harness: Some(harness),
        };
    }

    #[must_use]
    pub fn active_conversation_runtime(
        &self,
        session_id: SessionId,
    ) -> Option<(Arc<Harness>, SessionOptions)> {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        let harness = active_runtime.harness.as_ref().map(Arc::clone)?;
        let options = SessionOptions::new(&self.workspace_root)
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(session_id)
            .with_interactivity(InteractivityLevel::FullyInteractive)
            .with_model_id(active_runtime.default_model_id.clone())
            .with_protocol(active_runtime.default_protocol);
        Some((harness, options))
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.stream_permission_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.pending_permission_requests())
    }

    #[must_use]
    pub fn conversation_session_options(&self, session_id: SessionId) -> SessionOptions {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        self.conversation_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
        )
    }

    #[must_use]
    pub fn conversation_session_options_for_model(
        &self,
        session_id: SessionId,
        model_id: String,
        protocol: ModelProtocol,
    ) -> SessionOptions {
        let execution_settings =
            self.execution_settings_store
                .load_record()
                .unwrap_or(ExecutionSettingsRecord {
                    permission_mode: PermissionMode::Default,
                    context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
                    subagents_enabled: false,
                    agent_teams_enabled: false,
                    background_agents_enabled: false,
                });
        SessionOptions::new(&self.workspace_root)
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(session_id)
            .with_interactivity(InteractivityLevel::FullyInteractive)
            .with_model_id(model_id)
            .with_protocol(protocol)
            .with_context_compression_trigger_ratio(
                execution_settings.context_compression_trigger_ratio,
            )
    }

    #[must_use]
    pub fn default_conversation_id(&self) -> SessionId {
        self.default_conversation_id
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}

pub type ManagedDesktopRuntime = Arc<AsyncRwLock<DesktopRuntimeState>>;

#[must_use]
pub fn managed_runtime_state() -> ManagedDesktopRuntime {
    Arc::new(AsyncRwLock::new(initial_managed_runtime_state()))
}

fn initial_managed_runtime_state() -> DesktopRuntimeState {
    if let Ok(registry) = ProjectRegistry::load() {
        if let Some(active_path) = registry.active_path() {
            if let Ok(state) = tauri::async_runtime::block_on(runtime_state_for_workspace(
                PathBuf::from(active_path),
            )) {
                return state;
            }
        }
    }

    unconfigured_runtime_state()
}

fn unconfigured_runtime_state() -> DesktopRuntimeState {
    let workspace_root = crate::project_registry::unconfigured_workspace_root();
    let _ = std::fs::create_dir_all(&workspace_root);
    DesktopRuntimeState::with_workspace_for_test(workspace_root).unwrap_or_else(|_| {
        tauri::async_runtime::block_on(runtime_state_async())
            .expect("desktop runtime state should initialize")
    })
}

#[must_use]
pub fn runtime_state() -> DesktopRuntimeState {
    tauri::async_runtime::block_on(runtime_state_async())
        .expect("desktop runtime state should initialize")
}

pub async fn runtime_state_async() -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_for_workspace(current_workspace_root()?).await
}

pub async fn runtime_state_for_workspace(
    workspace_root: PathBuf,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::default());
    runtime_state_from_stream_permission_runtime(workspace_root, stream_permission_runtime).await
}

async fn runtime_state_from_stream_permission_runtime(
    workspace_root: PathBuf,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(workspace_root, "workspace root".to_owned())?;
    let (harness, model_id, protocol) = build_desktop_harness(
        &workspace_root,
        Arc::clone(&stream_permission_runtime),
        None,
    )
    .await?;

    DesktopRuntimeState::with_harness_stream_permission_runtime_and_model_for_workspace(
        workspace_root,
        Arc::new(harness),
        stream_permission_runtime,
        model_id,
        protocol,
    )
}

async fn build_desktop_harness(
    workspace_root: &Path,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
    model_config_id: Option<&str>,
) -> Result<(Harness, String, ModelProtocol), CommandErrorPayload> {
    let event_store = JsonlEventStore::open(
        workspace_root.join(".jyowo").join("runtime").join("events"),
        Arc::new(DefaultRedactor::default()),
    )
    .await
    .map_err(|error| runtime_init_failed(format!("event store initialization failed: {error}")))?;
    let mcp_server_store = DesktopMcpServerStore::new(workspace_root.to_path_buf());
    let mcp_diagnostic_store: Arc<dyn McpDiagnosticStore> =
        Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.to_path_buf()));
    let mcp_config = mcp_config_from_records(
        mcp_server_store.load_records()?,
        SessionId::new(),
        AgentId::new(),
        Arc::clone(&mcp_diagnostic_store),
        workspace_root,
    )
    .await?;
    let provider_settings_store = DesktopProviderSettingsStore::new(workspace_root.to_path_buf());
    let conversation_model_config_store =
        DesktopConversationModelConfigStore::new(workspace_root.to_path_buf());
    let (model_provider, model_id, protocol) =
        model_from_provider_settings(&provider_settings_store, model_config_id)?.unwrap_or_else(
            || {
                (
                    Arc::new(LocalLlamaProvider::default()) as Arc<dyn ModelProvider>,
                    "llama3.1".to_owned(),
                    ModelProtocol::ChatCompletions,
                )
            },
        );
    let skill_store = DesktopSkillStore::new(workspace_root.to_path_buf());
    let skill_loader = SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
        path: skill_store.enabled_dir(),
        source_kind: DirectorySourceKind::Workspace,
    });
    let blob_store = FileBlobStore::open(
        workspace_root.join(".jyowo").join("runtime").join("blobs"),
    )
    .map_err(|error| runtime_init_failed(format!("blob store initialization failed: {error}")))?;
    let provider_credential_resolver: Arc<dyn ProviderCredentialResolverCap> =
        Arc::new(DesktopProviderCredentialResolver::new(
            Arc::new(conversation_model_config_store),
            Arc::new(provider_settings_store.clone()),
        ));
    let plugin_store: Arc<dyn PluginStore> =
        Arc::new(DesktopPluginStore::new(workspace_root.to_path_buf()));
    let plugin_registry = build_plugin_registry(workspace_root, plugin_store.as_ref())?;

    let harness = Harness::builder()
        .with_workspace_root(workspace_root)
        .with_model_arc(model_provider)
        .with_model_id(model_id.clone())
        .with_default_session_options(
            SessionOptions::new(workspace_root)
                .with_model_id(model_id.clone())
                .with_protocol(protocol),
        )
        .with_store(event_store)
        .with_sandbox(LocalSandbox::new(workspace_root))
        .with_blob_store(blob_store)
        .with_capability(
            ToolCapability::ProviderCredentialResolver,
            provider_credential_resolver,
        )
        .with_mcp_config(mcp_config)
        .with_plugin_registry(plugin_registry)
        .with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))
        .with_skill_loader(skill_loader)
        .with_stream_permission_broker_arc(
            stream_permission_runtime.broker(),
            stream_permission_runtime.resolver_handle(),
        )
        .build()
        .await
        .map_err(|error| runtime_init_failed(format!("harness initialization failed: {error}")))?;

    Ok((harness, model_id, protocol))
}

fn build_plugin_registry(
    workspace_root: &Path,
    plugin_store: &dyn PluginStore,
) -> Result<PluginRegistry, CommandErrorPayload> {
    let settings = plugin_store.load_record()?;
    let (sidecar_sandbox, sidecar_sandbox_mode) = desktop_plugin_sidecar_sandbox(workspace_root);
    let mut entries = BTreeMap::new();
    let mut disabled_plugins = BTreeSet::new();
    for record in &settings.records {
        if record.enabled {
            verify_installed_plugin_content_hash(record, plugin_store)?;
        }
        let name = PluginName::new(record.name.clone())
            .map_err(|error| runtime_init_failed(format!("plugin record invalid: {error}")))?;
        entries.insert(name.clone(), record.config.clone());
        if !record.enabled {
            disabled_plugins.insert(name);
        }
    }

    let mut builder = PluginRegistry::builder()
        .with_config(plugin_config_from_settings(
            &settings,
            disabled_plugins,
            entries,
        ))
        .with_source(DiscoverySource::User(plugin_store.package_root()))
        .with_source(DiscoverySource::Workspace(
            plugin_store.workspace_plugin_root(),
        ))
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(FileManifestLoader))
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_timeout(Duration::from_secs(5))
                .with_search_paths(desktop_cargo_extension_search_paths(plugin_store))
                .with_sandbox(
                    sidecar_sandbox.clone(),
                    sidecar_sandbox_mode.clone(),
                    workspace_root.to_path_buf(),
                ),
        ))
        .with_runtime_loader(Arc::new(CargoExtensionRuntimeLoader::new().with_sandbox(
            sidecar_sandbox,
            sidecar_sandbox_mode,
            workspace_root.to_path_buf(),
        )));

    if settings.allow_project_plugins {
        builder = builder.with_source(DiscoverySource::Project(workspace_root.to_path_buf()));
    }

    builder.build().map_err(|error| {
        runtime_init_failed(format!("plugin registry initialization failed: {error}"))
    })
}

fn desktop_plugin_sidecar_sandbox(workspace_root: &Path) -> (Arc<dyn SandboxBackend>, SandboxMode) {
    let isolation = LocalIsolation::for_current_platform();
    let mode = SandboxMode::OsLevel(local_isolation_tag(isolation));
    let sandbox = LocalSandbox::new(workspace_root).with_isolation(isolation);
    (Arc::new(sandbox), mode)
}

fn local_isolation_tag(isolation: LocalIsolation) -> LocalIsolationTag {
    match isolation {
        LocalIsolation::None => LocalIsolationTag::None,
        LocalIsolation::Bubblewrap => LocalIsolationTag::Bubblewrap,
        LocalIsolation::Seatbelt => LocalIsolationTag::Seatbelt,
        LocalIsolation::JobObject => LocalIsolationTag::JobObject,
    }
}

fn plugin_config_from_settings(
    settings: &PluginSettingsRecord,
    disabled_plugins: BTreeSet<PluginName>,
    entries: BTreeMap<PluginName, Value>,
) -> PluginConfig {
    let allowed_user_plugins = entries.keys().cloned().collect();
    PluginConfig {
        allow_project_plugins: settings.allow_project_plugins,
        allowed_user_plugins: Some(allowed_user_plugins),
        disabled_plugins,
        entries,
        ..PluginConfig::default()
    }
}

fn desktop_cargo_extension_search_paths(plugin_store: &dyn PluginStore) -> Vec<PathBuf> {
    vec![plugin_store.cargo_extension_root()]
}

fn verify_installed_plugin_content_hash(
    record: &PluginStoreRecord,
    plugin_store: &dyn PluginStore,
) -> Result<(), CommandErrorPayload> {
    ensure_plugin_package_dir_name(&record.package_dir)?;
    let package_path = plugin_store.package_root().join(&record.package_dir);
    let current_hash = hash_plugin_package(&package_path)?;
    if current_hash == record.content_hash {
        return Ok(());
    }
    Err(runtime_operation_failed(format!(
        "plugin package content hash mismatch: {}",
        record.plugin_id.0
    )))
}

async fn reload_desktop_harness_after_plugin_change_locked(
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(stream_permission_runtime) = state.stream_permission_runtime.as_ref() else {
        return Ok(());
    };
    let (harness, model_id, protocol) = build_desktop_harness(
        &state.workspace_root,
        Arc::clone(stream_permission_runtime),
        None,
    )
    .await?;
    if let Some(old_harness) = state.harness() {
        if let Some(registry) = old_harness.plugin_registry() {
            for manifest in registry.list_activated() {
                let _ = registry.deactivate_cascade(&manifest.plugin_id()).await;
            }
        }
    }
    state.replace_harness(Arc::new(harness), model_id, protocol);
    Ok(())
}

fn current_workspace_root() -> Result<PathBuf, CommandErrorPayload> {
    if let Some(value) = std::env::var_os(WORKSPACE_ROOT_ENV) {
        if value.is_empty() {
            return Err(runtime_init_failed(format!(
                "{WORKSPACE_ROOT_ENV} is empty"
            )));
        }

        return canonical_workspace_root(
            PathBuf::from(value),
            format!("{WORKSPACE_ROOT_ENV} workspace root"),
        );
    }

    let current_dir = std::env::current_dir()
        .map_err(|error| runtime_init_failed(format!("workspace root unavailable: {error}")))?;
    canonical_workspace_root(current_dir, "current workspace root".to_owned())
}

fn current_process_workspace_root() -> Result<PathBuf, CommandErrorPayload> {
    let current_dir = std::env::current_dir()
        .map_err(|error| runtime_init_failed(format!("workspace root unavailable: {error}")))?;
    canonical_workspace_root(current_dir, "current workspace root".to_owned())
}

fn canonical_workspace_root(
    workspace_root: PathBuf,
    source: String,
) -> Result<PathBuf, CommandErrorPayload> {
    workspace_root.canonicalize().map_err(|error| {
        runtime_init_failed(format!(
            "{source} unavailable at {}: {error}",
            workspace_root.display()
        ))
    })
}

impl PermissionResolver for StreamPermissionRuntime {
    fn resolve_permission<'a>(
        &'a self,
        request_id: RequestId,
        decision: Decision,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandErrorPayload>> + Send + 'a>> {
        Box::pin(async move {
            self.resolve_permission(request_id, decision)
                .await
                .map_err(|error| CommandErrorPayload {
                    code: "PERMISSION_RESOLVE_FAILED",
                    message: error.to_string(),
                })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorPayload {
    pub code: &'static str,
    pub message: String,
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

fn default_worktree_direction() -> PageConversationWorktreeDirection {
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
    pub context_compression_trigger_ratio: f32,
    pub auto_mode_available: bool,
    pub agent_capabilities: AgentCapabilitiesPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExecutionSettingsRequest {
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExecutionSettingsRequest {
    pub permission_mode: PermissionMode,
    pub context_compression_trigger_ratio: f32,
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExecutionSettingsResponse {
    pub permission_mode: PermissionMode,
    pub context_compression_trigger_ratio: f32,
    pub auto_mode_available: bool,
    pub agent_capabilities: AgentCapabilitiesPayload,
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

#[must_use]
pub fn get_app_info_payload() -> AppInfoPayload {
    AppInfoPayload {
        name: "Jyowo",
        version: env!("CARGO_PKG_VERSION"),
        shell: "tauri2-react",
        harness: HarnessInfoPayload {
            sdk_crate: "jyowo_harness_sdk",
            mode: "in-process",
        },
    }
}

#[must_use]
pub fn harness_healthcheck_payload() -> HarnessHealthcheckPayload {
    let _sdk_marker = std::any::TypeId::of::<jyowo_harness_sdk::Harness>();

    HarnessHealthcheckPayload {
        status: "available",
        sdk_crate: "jyowo_harness_sdk",
    }
}

pub fn list_eval_cases_payload() -> Result<ListEvalCasesResponse, CommandErrorPayload> {
    Err(runtime_unavailable(
        "Listing eval cases requires the eval runtime.",
    ))
}

pub fn list_eval_cases_with_runtime_state(
    _state: &DesktopRuntimeState,
) -> Result<ListEvalCasesResponse, CommandErrorPayload> {
    list_eval_cases_payload()
}

#[must_use]
pub fn list_model_provider_catalog_payload() -> ModelProviderCatalogResponse {
    model_provider_catalog_response(provider_catalog_entries())
}

pub async fn list_model_provider_catalog_payload_with_remote() -> ModelProviderCatalogResponse {
    let mut entries = provider_catalog_entries();
    if let Some(openrouter_inventory) = fetch_openrouter_inventory().await {
        if let Some(openrouter) = entries
            .iter_mut()
            .find(|entry| entry.provider_id == "openrouter")
        {
            openrouter.models = runnable_inventory_models(&openrouter_inventory);
        }
    }
    model_provider_catalog_response(entries)
}

fn model_provider_catalog_response(
    entries: Vec<jyowo_harness_sdk::ext::ProviderCatalogEntry>,
) -> ModelProviderCatalogResponse {
    ModelProviderCatalogResponse {
        providers: entries
            .into_iter()
            .map(|entry| ModelProviderCatalogEntry {
                default_base_url: entry.default_base_url,
                display_name: entry.display_name,
                models: entry
                    .models
                    .into_iter()
                    .map(model_descriptor_catalog_entry)
                    .collect(),
                provider_id: entry.provider_id,
                runtime_capability: runtime_capability_payload(entry.runtime_capability),
                service_capabilities: entry
                    .service_capabilities
                    .into_iter()
                    .map(service_capability_payload)
                    .collect(),
                source_url: entry.source_url,
                verified_date: entry.verified_date.to_string(),
            })
            .collect(),
    }
}

fn runtime_capability_payload(
    capability: ProviderRuntimeCapability,
) -> ProviderRuntimeCapabilityPayload {
    ProviderRuntimeCapabilityPayload {
        auth_scheme: provider_auth_scheme_payload(capability.auth_scheme),
        base_url_regions: capability
            .base_url_regions
            .into_iter()
            .map(base_url_region_payload)
            .collect(),
        supports_live_validation: capability.supports_live_validation,
        supports_streaming_validation: capability.supports_streaming_validation,
        secret_reveal_supported: capability.secret_reveal_supported,
    }
}

fn base_url_region_payload(region: ProviderBaseUrlRegion) -> ProviderBaseUrlRegionPayload {
    ProviderBaseUrlRegionPayload {
        id: region.id,
        label: region.label,
        base_url: region.base_url,
    }
}

fn service_capability_payload(
    capability: ProviderServiceCapability,
) -> ProviderServiceCapabilityPayload {
    ProviderServiceCapabilityPayload {
        operation_id: capability.operation_id,
        category: provider_service_category_payload(capability.category),
        input_modalities: capability
            .input_modalities
            .iter()
            .map(model_modality_record)
            .collect(),
        output_artifact: model_modality_record(&capability.output_artifact),
        execution: provider_service_execution_payload(capability.execution),
        requires_polling: capability.requires_polling,
        permission_subject: capability.permission_subject,
        cost_risk: provider_service_cost_risk_payload(capability.cost_risk),
    }
}

fn provider_auth_scheme_payload(scheme: ProviderAuthScheme) -> &'static str {
    match scheme {
        ProviderAuthScheme::Bearer => "bearer",
        ProviderAuthScheme::ApiKey => "api_key",
        ProviderAuthScheme::XApiKey => "x_api_key",
        ProviderAuthScheme::None => "none",
    }
}

fn provider_service_category_payload(category: ProviderServiceCategory) -> &'static str {
    match category {
        ProviderServiceCategory::Conversation => "conversation",
        ProviderServiceCategory::Image => "image",
        ProviderServiceCategory::Video => "video",
        ProviderServiceCategory::Audio => "audio",
        ProviderServiceCategory::Music => "music",
        ProviderServiceCategory::File => "file",
        ProviderServiceCategory::Model => "model",
    }
}

fn provider_service_execution_payload(execution: ProviderServiceExecution) -> &'static str {
    match execution {
        ProviderServiceExecution::Sync => "sync",
        ProviderServiceExecution::AsyncJob => "async_job",
        ProviderServiceExecution::Websocket => "websocket",
    }
}

fn provider_service_cost_risk_payload(cost_risk: ProviderServiceCostRisk) -> &'static str {
    match cost_risk {
        ProviderServiceCostRisk::Low => "low",
        ProviderServiceCostRisk::Medium => "medium",
        ProviderServiceCostRisk::High => "high",
    }
}

async fn fetch_openrouter_inventory() -> Option<Vec<ModelInventoryEntry>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let mut response = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OPENROUTER_MODELS_API_BYTES as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OPENROUTER_MODELS_API_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    inventory_from_models_api_json(&bytes).ok()
}

pub async fn list_artifacts_with_runtime_state(
    request: ListArtifactsRequest,
    state: &DesktopRuntimeState,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    collect_artifacts_from_runtime_state(state, session_id).await
}

pub async fn get_artifact_media_preview_with_runtime_state(
    request: GetArtifactMediaPreviewRequest,
    state: &DesktopRuntimeState,
) -> Result<GetArtifactMediaPreviewResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("artifactId", &request.artifact_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let record = find_artifact_media_record(state, session_id, &request.artifact_id).await?;
    if !matches!(
        record.status,
        Some(jyowo_harness_sdk::ext::ArtifactStatus::Ready)
    ) {
        return Err(invalid_payload(
            "artifact image preview is not ready".to_owned(),
        ));
    }
    if !is_preview_image_artifact_kind(record.kind.as_deref()) {
        return Err(invalid_payload(
            "artifact media preview is only available for images".to_owned(),
        ));
    }
    let blob_ref = record.blob_ref.ok_or_else(|| {
        runtime_operation_failed("artifact image preview is unavailable".to_owned())
    })?;
    read_artifact_image_blob_preview(state, session_id, &blob_ref).await
}

pub async fn get_attachment_media_preview_with_runtime_state(
    request: GetAttachmentMediaPreviewRequest,
    state: &DesktopRuntimeState,
) -> Result<GetAttachmentMediaPreviewResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("attachmentId", &request.attachment_id)?;
    ensure_attachment_id(&request.attachment_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let attachment =
        find_attachment_media_record(state, session_id, &request.attachment_id).await?;
    let declared_attachment_mime =
        safe_preview_image_mime(&attachment.mime_type).ok_or_else(|| {
            invalid_payload("attachment media preview is only available for images".to_owned())
        })?;
    read_attachment_image_blob_preview(
        state,
        session_id,
        &request.attachment_id,
        &attachment.blob_ref,
        declared_attachment_mime,
    )
    .await
}

async fn collect_artifacts_from_runtime_state(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing artifacts requires the runtime conversation facade.",
        ));
    };
    if !harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }
    let redactor = DefaultRedactor::default();
    let mut after_event_id = None;
    let mut artifacts_by_id = BTreeMap::<String, ArtifactSummaryPayload>::new();
    let mut order = Vec::<String>::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("artifact read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            project_artifact_event(
                envelope.payload,
                session_id,
                &redactor,
                &mut artifacts_by_id,
                &mut order,
            );
        }

        after_event_id = page.next_event_id;
    }

    let mut artifacts = order
        .into_iter()
        .filter_map(|artifact_id| artifacts_by_id.remove(&artifact_id))
        .collect::<Vec<_>>();
    artifacts.reverse();
    Ok(ListArtifactsResponse { artifacts })
}

#[derive(Debug, Clone)]
struct ArtifactMediaRecord {
    blob_ref: Option<BlobRef>,
    kind: Option<String>,
    status: Option<jyowo_harness_sdk::ext::ArtifactStatus>,
}

async fn find_artifact_media_record(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    artifact_id: &str,
) -> Result<ArtifactMediaRecord, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading artifact media requires the runtime conversation facade.",
        ));
    };
    if !harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }

    let mut after_event_id = None;
    let mut record: Option<ArtifactMediaRecord> = None;
    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("artifact media read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            match envelope.payload {
                Event::ArtifactCreated(event) => {
                    if event.session_id == session_id && event.artifact_id == artifact_id {
                        record = Some(ArtifactMediaRecord {
                            blob_ref: event.blob_ref,
                            kind: Some(event.kind),
                            status: Some(event.status),
                        });
                    }
                }
                Event::ArtifactUpdated(event) => {
                    if event.session_id != session_id || event.artifact_id != artifact_id {
                        continue;
                    }
                    let entry = record.get_or_insert_with(|| ArtifactMediaRecord {
                        blob_ref: None,
                        kind: None,
                        status: None,
                    });
                    if let Some(blob_ref) = event.blob_ref {
                        entry.blob_ref = Some(blob_ref);
                    }
                    if let Some(kind) = event.kind {
                        entry.kind = Some(kind);
                    }
                    if let Some(status) = event.status {
                        entry.status = Some(status);
                    }
                }
                _ => {}
            }
        }

        after_event_id = page.next_event_id;
    }

    record.ok_or_else(|| not_found("artifact not found".to_owned()))
}

async fn find_attachment_media_record(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
) -> Result<ConversationAttachmentReference, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading attachment media requires the runtime conversation facade.",
        ));
    };
    if !harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }

    let mut after_event_id = None;
    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("attachment media read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            let Event::UserMessageAppended(event) = envelope.payload else {
                continue;
            };
            for attachment in event.attachments {
                if attachment.id == attachment_id {
                    return Ok(attachment);
                }
            }
        }

        after_event_id = page.next_event_id;
    }

    Err(not_found("attachment not found".to_owned()))
}

async fn read_artifact_image_blob_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    blob_ref: &BlobRef,
) -> Result<GetArtifactMediaPreviewResponse, CommandErrorPayload> {
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .map_err(|_| runtime_operation_failed("artifact image preview is unavailable".to_owned()))?;
    let meta = blob_store
        .head(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| runtime_operation_failed("artifact image preview is unavailable".to_owned()))?
        .ok_or_else(|| {
            runtime_operation_failed("artifact image preview is unavailable".to_owned())
        })?;
    if meta.retention != BlobRetention::SessionScoped(session_id) {
        return Err(invalid_payload(
            "artifact image preview is unavailable for this conversation".to_owned(),
        ));
    }
    let declared_content_type = meta
        .content_type
        .as_deref()
        .or(blob_ref.content_type.as_deref());
    let declared_mime_type = match declared_content_type.and_then(declared_mime_token) {
        Some(mime_type) => match safe_preview_image_mime(mime_type) {
            Some(image_mime_type) => Some(image_mime_type.to_owned()),
            None if safe_artifact_mime_type(mime_type).is_some()
                || mime_type.starts_with("image/") =>
            {
                return Err(invalid_payload(
                    "artifact media preview is only available for images".to_owned(),
                ));
            }
            None => None,
        },
        None => None,
    };
    let size_bytes = meta.size;
    if size_bytes > MAX_ARTIFACT_MEDIA_PREVIEW_BYTES {
        return Err(invalid_payload(
            "artifact image preview is too large".to_owned(),
        ));
    }

    let mut stream = blob_store
        .get(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| {
            runtime_operation_failed("artifact image preview is unavailable".to_owned())
        })?;
    let mut bytes = Vec::with_capacity(size_bytes.min(MAX_ARTIFACT_MEDIA_PREVIEW_BYTES) as usize);
    while let Some(chunk) = stream.next().await {
        let next_len = bytes.len().saturating_add(chunk.len());
        if u64::try_from(next_len).unwrap_or(u64::MAX) > MAX_ARTIFACT_MEDIA_PREVIEW_BYTES {
            return Err(invalid_payload(
                "artifact image preview is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let detected_mime = detect_preview_image_mime(&bytes).ok_or_else(|| {
        invalid_payload("artifact media preview is only available for images".to_owned())
    })?;
    if declared_mime_type
        .as_deref()
        .is_some_and(|mime_type| mime_type != detected_mime)
    {
        return Err(invalid_payload(
            "artifact media preview is only available for images".to_owned(),
        ));
    }
    let mime_type = detected_mime.to_owned();

    Ok(GetArtifactMediaPreviewResponse {
        data_url: format!(
            "data:{mime_type};base64,{}",
            general_purpose::STANDARD.encode(bytes)
        ),
        mime_type,
        size_bytes,
    })
}

async fn read_attachment_image_blob_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
    blob_ref: &BlobRef,
    declared_attachment_mime: &str,
) -> Result<GetAttachmentMediaPreviewResponse, CommandErrorPayload> {
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .map_err(|_| runtime_operation_failed("attachment image preview is unavailable".to_owned()))?;
    let meta = blob_store
        .head(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| {
            runtime_operation_failed("attachment image preview is unavailable".to_owned())
        })?
        .ok_or_else(|| {
            runtime_operation_failed("attachment image preview is unavailable".to_owned())
        })?;
    match meta.retention {
        BlobRetention::TenantScoped => {}
        BlobRetention::SessionScoped(retention_session_id)
            if retention_session_id == session_id => {}
        _ => {
            return Err(invalid_payload(
                "attachment image preview is unavailable for this conversation".to_owned(),
            ));
        }
    }
    let declared_content_type = meta
        .content_type
        .as_deref()
        .or(blob_ref.content_type.as_deref());
    if let Some(mime_type) = declared_content_type.and_then(declared_mime_token) {
        match safe_preview_image_mime(mime_type) {
            Some(image_mime_type) if image_mime_type == declared_attachment_mime => {}
            _ => {
                return Err(invalid_payload(
                    "attachment media preview is only available for images".to_owned(),
                ));
            }
        }
    }
    let size_bytes = meta.size;
    if size_bytes > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }

    let mut stream = blob_store
        .get(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| {
            runtime_operation_failed("attachment image preview is unavailable".to_owned())
        })?;
    let mut bytes = Vec::with_capacity(size_bytes.min(MAX_ATTACHMENT_BYTES) as usize);
    while let Some(chunk) = stream.next().await {
        let next_len = bytes.len().saturating_add(chunk.len());
        if u64::try_from(next_len).unwrap_or(u64::MAX) > MAX_ATTACHMENT_BYTES {
            return Err(invalid_payload(
                "attachment image preview is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let detected_mime = detect_preview_image_mime(&bytes).ok_or_else(|| {
        invalid_payload("attachment media preview is only available for images".to_owned())
    })?;
    if detected_mime != declared_attachment_mime {
        return Err(invalid_payload(
            "attachment media preview is only available for images".to_owned(),
        ));
    }
    let (sanitized_bytes, mime_type) =
        sanitize_attachment_preview_image(&bytes, detected_mime, attachment_id)?;
    let size_bytes = sanitized_bytes.len() as u64;

    Ok(GetAttachmentMediaPreviewResponse {
        data_url: format!(
            "data:{mime_type};base64,{}",
            general_purpose::STANDARD.encode(sanitized_bytes)
        ),
        mime_type: mime_type.to_owned(),
        size_bytes,
    })
}

fn detect_preview_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major_brand = &bytes[8..12];
        if major_brand == b"avif" || major_brand == b"avis" {
            return Some("image/avif");
        }
        if bytes
            .get(16..)
            .unwrap_or_default()
            .chunks_exact(4)
            .any(|brand| brand == b"avif" || brand == b"avis")
        {
            return Some("image/avif");
        }
    }
    None
}

fn safe_preview_image_mime(value: &str) -> Option<&'static str> {
    let mime = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

fn ensure_preview_image_bytes_public(
    bytes: &[u8],
    attachment_id: &str,
) -> Result<(), CommandErrorPayload> {
    for text in printable_ascii_runs(bytes, 16) {
        if preview_text_contains_unsafe_payload(&text, attachment_id) {
            return Err(invalid_payload(
                "attachment image preview contains unsafe metadata".to_owned(),
            ));
        }
    }

    Ok(())
}

fn sanitize_attachment_preview_image(
    bytes: &[u8],
    detected_mime: &str,
    attachment_id: &str,
) -> Result<(Vec<u8>, &'static str), CommandErrorPayload> {
    match detected_mime {
        "image/png" => Ok((
            sanitize_attachment_preview_png(bytes, attachment_id)?,
            "image/png",
        )),
        "image/jpeg" | "image/gif" | "image/webp" => Ok((
            transcode_attachment_preview_to_png(bytes, detected_mime, attachment_id)?,
            "image/png",
        )),
        "image/avif" => Ok((
            sanitize_attachment_preview_avif(bytes, attachment_id)?,
            "image/avif",
        )),
        _ => Err(invalid_payload(
            "attachment media preview is only available for images".to_owned(),
        )),
    }
}

fn transcode_attachment_preview_to_png(
    bytes: &[u8],
    detected_mime: &str,
    attachment_id: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let format = preview_image_format(detected_mime).ok_or_else(|| {
        invalid_payload("attachment media preview is only available for images".to_owned())
    })?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(attachment_preview_decode_limits());
    let image = reader
        .decode()
        .map_err(|_| invalid_payload("attachment image preview is malformed".to_owned()))?;
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| invalid_payload("attachment image preview is malformed".to_owned()))?;
    let sanitized = encoded.into_inner();
    if sanitized.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }
    ensure_preview_image_bytes_public(&sanitized, attachment_id)?;

    Ok(sanitized)
}

fn sanitize_attachment_preview_avif(
    bytes: &[u8],
    attachment_id: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let info = oxideav_avif::inspect(bytes)
        .map_err(|_| invalid_payload("attachment image preview is malformed".to_owned()))?;
    validate_attachment_preview_dimensions(info.width, info.height)?;
    if info.has_descriptive_metadata() {
        return Err(invalid_payload(
            "attachment image preview contains unsafe metadata".to_owned(),
        ));
    }
    // AVIF stays in its original container because this path uses pure Rust
    // container inspection rather than a system AV1 decoder. Descriptive
    // metadata and unsafe printable payloads fail closed before bytes return.
    ensure_preview_image_bytes_public(bytes, attachment_id)?;

    Ok(bytes.to_vec())
}

fn preview_image_format(mime_type: &str) -> Option<ImageFormat> {
    match mime_type {
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

fn attachment_preview_decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_ATTACHMENT_PREVIEW_DIMENSION);
    limits.max_image_height = Some(MAX_ATTACHMENT_PREVIEW_DIMENSION);
    limits.max_alloc = Some(MAX_ATTACHMENT_PREVIEW_DECODED_BYTES);
    limits
}

fn sanitize_attachment_preview_png(
    bytes: &[u8],
    attachment_id: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let Some("image/png") = detect_preview_image_mime(bytes) else {
        return Err(invalid_payload(
            "attachment image preview is unavailable for this image type".to_owned(),
        ));
    };

    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";
    let mut cursor = PNG_SIGNATURE.len();
    let mut sanitized = Vec::with_capacity(bytes.len());
    sanitized.extend_from_slice(PNG_SIGNATURE);
    let mut saw_ihdr = false;
    let mut saw_idat = false;
    let mut saw_iend = false;

    while cursor < bytes.len() {
        let Some(length_bytes) = bytes.get(cursor..cursor + 4) else {
            return Err(invalid_payload(
                "attachment image preview is malformed".to_owned(),
            ));
        };
        let length = u32::from_be_bytes([
            length_bytes[0],
            length_bytes[1],
            length_bytes[2],
            length_bytes[3],
        ]) as usize;
        let chunk_start = cursor;
        let chunk_type_start = cursor + 4;
        let chunk_data_start = chunk_type_start + 4;
        let chunk_crc_start = chunk_data_start.saturating_add(length);
        let chunk_end = chunk_crc_start.saturating_add(4);
        let Some(chunk_type) = bytes.get(chunk_type_start..chunk_data_start) else {
            return Err(invalid_payload(
                "attachment image preview is malformed".to_owned(),
            ));
        };
        let Some(chunk) = bytes.get(chunk_start..chunk_end) else {
            return Err(invalid_payload(
                "attachment image preview is malformed".to_owned(),
            ));
        };

        match chunk_type {
            b"IHDR" if !saw_ihdr && cursor == PNG_SIGNATURE.len() && length == 13 => {
                let Some(dimensions) = bytes.get(chunk_data_start..chunk_data_start + 8) else {
                    return Err(invalid_payload(
                        "attachment image preview is malformed".to_owned(),
                    ));
                };
                let width = u32::from_be_bytes([
                    dimensions[0],
                    dimensions[1],
                    dimensions[2],
                    dimensions[3],
                ]);
                let height = u32::from_be_bytes([
                    dimensions[4],
                    dimensions[5],
                    dimensions[6],
                    dimensions[7],
                ]);
                validate_attachment_preview_dimensions(width, height)?;
                saw_ihdr = true;
                sanitized.extend_from_slice(chunk);
            }
            b"PLTE" if saw_ihdr && !saw_idat => {
                sanitized.extend_from_slice(chunk);
            }
            b"IDAT" if saw_ihdr && !saw_iend => {
                saw_idat = true;
                sanitized.extend_from_slice(chunk);
            }
            b"IEND" if saw_ihdr && saw_idat && !saw_iend && length == 0 => {
                saw_iend = true;
                sanitized.extend_from_slice(chunk);
                cursor = chunk_end;
                break;
            }
            _ if chunk_type.first().is_some_and(u8::is_ascii_lowercase) => {}
            _ => {
                return Err(invalid_payload(
                    "attachment image preview is malformed".to_owned(),
                ));
            }
        }

        cursor = chunk_end;
    }

    if !saw_iend || cursor != bytes.len() {
        return Err(invalid_payload(
            "attachment image preview is malformed".to_owned(),
        ));
    }
    if sanitized.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }
    ensure_preview_image_bytes_public(&sanitized, attachment_id)?;

    Ok(sanitized)
}

fn validate_attachment_preview_dimensions(
    width: u32,
    height: u32,
) -> Result<(), CommandErrorPayload> {
    if width == 0
        || height == 0
        || width > MAX_ATTACHMENT_PREVIEW_DIMENSION
        || height > MAX_ATTACHMENT_PREVIEW_DIMENSION
        || u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4)
            > MAX_ATTACHMENT_PREVIEW_DECODED_BYTES
    {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }

    Ok(())
}

fn printable_ascii_runs(bytes: &[u8], min_len: usize) -> Vec<String> {
    let mut runs = Vec::new();
    let mut run = Vec::new();

    for byte in bytes {
        if byte.is_ascii_graphic() || *byte == b' ' || *byte == b'\t' {
            run.push(*byte);
            continue;
        }
        if run.len() >= min_len {
            runs.push(String::from_utf8_lossy(&run).into_owned());
        }
        run.clear();
    }
    if run.len() >= min_len {
        runs.push(String::from_utf8_lossy(&run).into_owned());
    }

    runs
}

fn preview_text_contains_unsafe_payload(value: &str, attachment_id: &str) -> bool {
    contains_obvious_secret(value)
        || redact_unsafe_display_text(value) != value
        || value.contains(attachment_id)
}

fn declared_mime_token(value: &str) -> Option<&str> {
    value
        .split(|character: char| character == ';' || character.is_whitespace())
        .find(|part| part.contains('/'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

fn is_preview_image_artifact_kind(value: Option<&str>) -> bool {
    value.is_some_and(|kind| kind == "image" || safe_preview_image_mime(kind).is_some())
}

fn project_artifact_event(
    event: Event,
    session_id: SessionId,
    redactor: &dyn Redactor,
    artifacts_by_id: &mut BTreeMap<String, ArtifactSummaryPayload>,
    order: &mut Vec<String>,
) {
    match event {
        Event::ArtifactCreated(event) => {
            if event.session_id != session_id {
                return;
            }
            let artifact_id = event.artifact_id;
            if !artifacts_by_id.contains_key(&artifact_id) {
                order.push(artifact_id.clone());
            }
            artifacts_by_id.insert(
                artifact_id.clone(),
                ArtifactSummaryPayload {
                    action_label: "Open".to_owned(),
                    description: artifact_description_from_source(event.source),
                    id: artifact_id,
                    kind: public_text_display(event.kind, redactor),
                    preview: event.preview.map(|preview| {
                        truncate_utf8(
                            public_text_display(preview, redactor),
                            MAX_ARTIFACT_PREVIEW_BYTES,
                        )
                    }),
                    source_message_id: event
                        .source_message_id
                        .map(|message_id| message_id.to_string()),
                    source_run_id: event.run_id.to_string(),
                    status: artifact_status_label(event.status),
                    title: public_text_display(event.title, redactor),
                },
            );
        }
        Event::ArtifactUpdated(event) => {
            if event.session_id != session_id {
                return;
            }
            let Some(artifact) = artifacts_by_id.get_mut(&event.artifact_id) else {
                return;
            };
            if let Some(kind) = event.kind {
                artifact.kind = public_text_display(kind, redactor);
            }
            if let Some(preview) = event.preview {
                artifact.preview = Some(truncate_utf8(
                    public_text_display(preview, redactor),
                    MAX_ARTIFACT_PREVIEW_BYTES,
                ));
            }
            if let Some(source_message_id) = event.source_message_id {
                artifact.source_message_id = Some(source_message_id.to_string());
            }
            artifact.source_run_id = event.run_id.to_string();
            if let Some(status) = event.status {
                artifact.status = artifact_status_label(status);
            }
            if let Some(title) = event.title {
                artifact.title = public_text_display(title, redactor);
            }
        }
        _ => {}
    }
}

fn artifact_status_label(status: jyowo_harness_sdk::ext::ArtifactStatus) -> &'static str {
    match status {
        jyowo_harness_sdk::ext::ArtifactStatus::Pending => "pending",
        jyowo_harness_sdk::ext::ArtifactStatus::Running => "running",
        jyowo_harness_sdk::ext::ArtifactStatus::Ready => "ready",
        jyowo_harness_sdk::ext::ArtifactStatus::Failed => "failed",
        _ => "ready",
    }
}

fn artifact_source_label(source: jyowo_harness_sdk::ext::ArtifactSource) -> &'static str {
    match source {
        jyowo_harness_sdk::ext::ArtifactSource::Assistant => "assistant",
        jyowo_harness_sdk::ext::ArtifactSource::Tool => "tool",
        jyowo_harness_sdk::ext::ArtifactSource::File => "file",
        jyowo_harness_sdk::ext::ArtifactSource::ModelService => "model_service",
        _ => "assistant",
    }
}

fn artifact_media_payload(blob_ref: Option<&BlobRef>, artifact_kind: &str) -> Option<Value> {
    let blob_ref = blob_ref?;
    let safe_mime_type = blob_ref
        .content_type
        .as_deref()
        .and_then(safe_artifact_mime_type);
    let kind = artifact_media_kind_from_label(artifact_kind).or_else(|| {
        safe_mime_type
            .as_deref()
            .and_then(artifact_media_kind_from_mime)
    })?;
    let mime_type = safe_mime_type
        .filter(|mime_type| {
            kind == "file"
                || artifact_media_kind_from_mime(mime_type)
                    .is_some_and(|mime_kind| mime_kind == kind)
        })
        .unwrap_or_else(|| default_artifact_mime_type(kind).to_owned());
    Some(json!({
        "kind": kind,
        "mimeType": mime_type,
        "sizeBytes": blob_ref.size,
    }))
}

fn artifact_media_kind_from_label(value: &str) -> Option<&'static str> {
    match value {
        "image" => Some("image"),
        "video" => Some("video"),
        "audio" => Some("audio"),
        "file" => Some("file"),
        _ => safe_artifact_mime_type(value)
            .as_deref()
            .and_then(artifact_media_kind_from_mime),
    }
}

fn artifact_media_kind_from_mime(value: &str) -> Option<&'static str> {
    if safe_artifact_image_mime_type(value).is_some() {
        Some("image")
    } else if value.starts_with("video/") {
        Some("video")
    } else if value.starts_with("audio/") {
        Some("audio")
    } else if safe_artifact_mime_type(value).is_some() {
        Some("file")
    } else {
        None
    }
}

fn default_artifact_mime_type(kind: &str) -> &'static str {
    match kind {
        "image" => "image/png",
        "video" => "video/mp4",
        "audio" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

fn safe_artifact_mime_type(value: &str) -> Option<String> {
    let mime_type = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime_type.as_str() {
        "image/png"
        | "image/jpeg"
        | "image/gif"
        | "image/webp"
        | "image/avif"
        | "video/mp4"
        | "video/webm"
        | "video/quicktime"
        | "audio/mpeg"
        | "audio/mp4"
        | "audio/ogg"
        | "audio/wav"
        | "audio/webm"
        | "text/plain"
        | "text/markdown"
        | "text/csv"
        | "application/json"
        | "application/pdf"
        | "application/zip"
        | "application/octet-stream" => Some(mime_type),
        _ => None,
    }
}

fn safe_artifact_image_mime_type(value: &str) -> Option<&'static str> {
    match value {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

fn artifact_description_from_source(source: jyowo_harness_sdk::ext::ArtifactSource) -> String {
    match source {
        jyowo_harness_sdk::ext::ArtifactSource::Assistant => {
            "Generated by the assistant as a durable artifact.".to_owned()
        }
        jyowo_harness_sdk::ext::ArtifactSource::Tool => {
            "Generated by a tool as a durable artifact.".to_owned()
        }
        jyowo_harness_sdk::ext::ArtifactSource::File => {
            "Linked from the workspace as a durable artifact.".to_owned()
        }
        jyowo_harness_sdk::ext::ArtifactSource::ModelService => {
            "Generated by the model service as a durable artifact.".to_owned()
        }
        _ => "Generated as a durable artifact.".to_owned(),
    }
}

pub fn run_eval_case_payload(
    request: RunEvalCaseRequest,
) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    ensure_eval_case_id(&request.case_id)?;

    Err(runtime_unavailable(
        "Running eval cases requires the eval runtime.",
    ))
}

pub fn run_eval_case_with_runtime_state(
    request: RunEvalCaseRequest,
    _state: &DesktopRuntimeState,
) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    run_eval_case_payload(request)
}

pub async fn validate_provider_settings_payload(
    request: ValidateProviderSettingsRequest,
) -> Result<ValidateProviderSettingsResponse, CommandErrorPayload> {
    ensure_provider_model_supported(&request).await?;

    Ok(ValidateProviderSettingsResponse {
        model_id: request.model_id,
        provider_id: request.provider_id,
        status: "accepted",
    })
}

pub fn get_execution_settings_with_store(
    store: &DesktopExecutionSettingsStore,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let record = store.load_record()?;
    let permission_mode = effective_execution_settings_permission_mode(record.permission_mode);
    Ok(GetExecutionSettingsResponse {
        permission_mode,
        context_compression_trigger_ratio: record.context_compression_trigger_ratio,
        auto_mode_available: auto_mode_available(),
        agent_capabilities: agent_capabilities_payload(&record),
    })
}

pub fn get_execution_settings_for_request(
    request: GetExecutionSettingsRequest,
    active_store: &DesktopExecutionSettingsStore,
    project_registry: &ProjectRegistry,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let Some(workspace_path) = request.workspace_path else {
        return get_execution_settings_with_store(active_store);
    };
    let workspace_root =
        canonical_workspace_root(PathBuf::from(workspace_path), "workspace path".to_owned())?;
    let workspace_root_text = workspace_root.to_string_lossy();
    if !project_registry
        .list_projects()
        .iter()
        .any(|project| project.path == workspace_root_text.as_ref())
    {
        return Err(invalid_payload("project is not registered".to_owned()));
    }
    let store = DesktopExecutionSettingsStore::new(workspace_root);
    get_execution_settings_with_store(&store)
}

pub fn set_execution_settings_with_store(
    request: SetExecutionSettingsRequest,
    store: &DesktopExecutionSettingsStore,
) -> Result<SetExecutionSettingsResponse, CommandErrorPayload> {
    ensure_execution_settings_record(&ExecutionSettingsRecord {
        permission_mode: request.permission_mode,
        context_compression_trigger_ratio: request.context_compression_trigger_ratio,
        subagents_enabled: request.subagents_enabled,
        agent_teams_enabled: request.agent_teams_enabled,
        background_agents_enabled: request.background_agents_enabled,
    })?;
    if request.permission_mode == PermissionMode::Auto && !auto_mode_available() {
        return Err(invalid_payload(
            "auto permission mode is unavailable in this desktop build".to_owned(),
        ));
    }
    let record = ExecutionSettingsRecord {
        permission_mode: request.permission_mode,
        context_compression_trigger_ratio: request.context_compression_trigger_ratio,
        subagents_enabled: request.subagents_enabled,
        agent_teams_enabled: request.agent_teams_enabled,
        background_agents_enabled: request.background_agents_enabled,
    };
    store.save_record(&record)?;
    Ok(SetExecutionSettingsResponse {
        permission_mode: record.permission_mode,
        context_compression_trigger_ratio: record.context_compression_trigger_ratio,
        auto_mode_available: auto_mode_available(),
        agent_capabilities: agent_capabilities_payload(&record),
    })
}

pub async fn list_provider_settings_with_store(
    store: &dyn ProviderSettingsStore,
) -> Result<ListProviderSettingsResponse, CommandErrorPayload> {
    let record = store.load_record()?.unwrap_or_default();

    Ok(ListProviderSettingsResponse {
        default_config_id: record.default_config_id.clone(),
        configs: provider_config_payloads(&record)?,
    })
}

fn provider_config_with_api_key<'a>(
    record: &'a ProviderSettingsRecord,
    config_id: &str,
) -> Result<&'a ProviderConfigRecord, CommandErrorPayload> {
    let Some(config) = record.configs.iter().find(|config| config.id == config_id) else {
        return Err(not_found(format!("provider config not found: {config_id}")));
    };
    ensure_provider_config_has_api_key(config)?;
    Ok(config)
}

fn provider_api_key_fingerprint(api_key: &str) -> [u8; 32] {
    *blake3::hash(api_key.as_bytes()).as_bytes()
}

fn prune_expired_provider_api_key_reveal_tokens(
    tokens: &mut HashMap<String, ProviderConfigRevealTokenRecord>,
    now: Instant,
) {
    tokens.retain(|_, token| token.expires_at > now);
}

async fn clear_provider_api_key_reveal_tokens_for_config(
    runtime_state: &DesktopRuntimeState,
    config_id: &str,
) {
    let mut tokens = runtime_state.provider_api_key_reveal_tokens.lock().await;
    tokens.retain(|_, token| token.config_id != config_id);
}

pub async fn request_provider_config_api_key_reveal_with_store(
    request: RequestProviderConfigApiKeyRevealRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    let record = store.load_record()?.unwrap_or_default();
    provider_config_with_api_key(&record, &request.config_id)?;
    Err(invalid_payload(
        "provider API key reveal requires runtime state".to_owned(),
    ))
}

async fn request_provider_config_api_key_reveal_with_runtime_state_unlocked(
    request: RequestProviderConfigApiKeyRevealRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    let record = runtime_state
        .provider_settings_store
        .load_record()?
        .unwrap_or_default();
    let config = provider_config_with_api_key(&record, &request.config_id)?;
    let api_key_fingerprint = provider_api_key_fingerprint(&config.api_key);

    let now = Instant::now();
    let reveal_token = RunId::new().to_string();
    let mut tokens = runtime_state.provider_api_key_reveal_tokens.lock().await;
    prune_expired_provider_api_key_reveal_tokens(&mut tokens, now);
    tokens.insert(
        reveal_token.clone(),
        ProviderConfigRevealTokenRecord {
            api_key_fingerprint,
            config_id: request.config_id.clone(),
            expires_at: now + PROVIDER_API_KEY_REVEAL_TTL,
        },
    );

    Ok(RequestProviderConfigApiKeyRevealResponse {
        config_id: request.config_id,
        expires_in_seconds: PROVIDER_API_KEY_REVEAL_TTL.as_secs(),
        reveal_token,
        status: "ready",
    })
}

pub async fn request_provider_config_api_key_reveal_with_runtime_state(
    request: RequestProviderConfigApiKeyRevealRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    request_provider_config_api_key_reveal_with_runtime_state_unlocked(request, runtime_state).await
}

pub async fn get_provider_config_api_key_with_store(
    request: GetProviderConfigApiKeyRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    ensure_non_empty("revealToken", &request.reveal_token)?;
    let record = store.load_record()?.unwrap_or_default();
    provider_config_with_api_key(&record, &request.config_id)?;

    Err(invalid_payload(
        "provider API key reveal requires runtime state".to_owned(),
    ))
}

async fn get_provider_config_api_key_with_runtime_state_unlocked(
    request: GetProviderConfigApiKeyRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    ensure_non_empty("revealToken", &request.reveal_token)?;

    let now = Instant::now();
    let token_record = {
        let mut tokens = runtime_state.provider_api_key_reveal_tokens.lock().await;
        let token_record = tokens.remove(&request.reveal_token);
        prune_expired_provider_api_key_reveal_tokens(&mut tokens, now);
        token_record
    }
    .ok_or_else(|| {
        invalid_payload("provider API key reveal token is invalid or expired".to_owned())
    })?;

    if token_record.expires_at <= now {
        return Err(invalid_payload(
            "provider API key reveal token expired".to_owned(),
        ));
    }
    if token_record.config_id != request.config_id {
        return Err(invalid_payload(
            "provider API key reveal token does not match configId".to_owned(),
        ));
    }

    let record = runtime_state
        .provider_settings_store
        .load_record()?
        .unwrap_or_default();
    let config = provider_config_with_api_key(&record, &request.config_id)?;
    if token_record.api_key_fingerprint != provider_api_key_fingerprint(&config.api_key) {
        return Err(invalid_payload(
            "provider API key reveal token no longer matches config".to_owned(),
        ));
    }
    Ok(GetProviderConfigApiKeyResponse {
        api_key: config.api_key.clone(),
        config_id: request.config_id,
    })
}

pub async fn get_provider_config_api_key_with_runtime_state(
    request: GetProviderConfigApiKeyRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    get_provider_config_api_key_with_runtime_state_unlocked(request, runtime_state).await
}

pub async fn save_provider_settings_with_store(
    request: ProviderSettingsRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    ensure_provider_settings(&request)?;
    let base_url = normalized_base_url(request.base_url.as_deref())?;
    let mut record = store.load_record()?.unwrap_or_default();
    let config_id = provider_config_id(&record, &request);
    let previous_config = record
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .cloned();
    let descriptor = provider_settings_descriptor(&request, previous_config.as_ref()).await?;
    let api_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let config_api_key = if let Some(api_key) = api_key {
        api_key.to_owned()
    } else if let Some(config) = previous_config.as_ref() {
        if config.provider_id != request.provider_id || config.base_url != base_url {
            return Err(invalid_payload(
                "apiKey is required when changing provider or baseUrl".to_owned(),
            ));
        }
        let api_key = match ensure_provider_config_has_api_key(config) {
            Ok(api_key) => api_key,
            Err(_) => {
                return Err(invalid_payload(
                    "apiKey is required for provider configs without a stored key".to_owned(),
                ));
            }
        };
        api_key.to_owned()
    } else {
        return Err(invalid_payload(
            "apiKey is required for new provider configs".to_owned(),
        ));
    };
    let config = ProviderConfigRecord {
        api_key: config_api_key,
        protocol: descriptor.protocol,
        base_url,
        display_name: request
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| provider_display_name(&request.provider_id)),
        id: config_id.clone(),
        model_id: request.model_id.clone(),
        provider_id: request.provider_id.clone(),
        model_descriptor: model_descriptor_record(&descriptor),
    };
    record.configs.retain(|existing| existing.id != config_id);
    record.configs.push(config);
    record.configs.sort_by(|left, right| left.id.cmp(&right.id));
    if request.set_default || record.default_config_id.is_none() {
        record.default_config_id = Some(config_id.clone());
    }
    store.save_record(&record)?;

    Ok(SaveProviderSettingsResponse {
        config: provider_config_payload(
            record
                .configs
                .iter()
                .find(|config| config.id == config_id)
                .expect("saved config should exist"),
            record.default_config_id.as_deref(),
        )?,
        status: "saved",
    })
}

async fn save_provider_settings_with_runtime_state_unlocked(
    request: ProviderSettingsRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let response =
        save_provider_settings_with_store(request, runtime_state.provider_settings_store.as_ref())
            .await?;
    clear_provider_api_key_reveal_tokens_for_config(runtime_state, &response.config.id).await;
    Ok(response)
}

pub async fn save_provider_settings_with_runtime_state(
    request: ProviderSettingsRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    save_provider_settings_with_runtime_state_unlocked(request, runtime_state).await
}

pub async fn list_mcp_servers_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListMcpServersResponse, CommandErrorPayload> {
    let mut servers = BTreeMap::new();
    let records = state.mcp_server_store.load_records()?;
    let records_by_id = records
        .iter()
        .map(|record| (record.id.clone(), record.clone()))
        .collect::<BTreeMap<_, _>>();
    let last_diagnostics =
        mcp_last_diagnostics_by_server(&state.mcp_diagnostic_store.load_records()?);

    for record in &records {
        let mut summary = mcp_server_summary_from_record(record);
        apply_mcp_last_diagnostic(&mut summary, last_diagnostics.get(&record.id));
        servers.insert(record.id.clone(), summary);
    }

    if let Some(harness) = state.harness() {
        if let Some(config) = harness.mcp_config() {
            for server_id in config.registry.server_ids().await {
                if let Some(summary) =
                    mcp_server_summary_from_registry(&config.registry, &server_id).await
                {
                    let mut summary = summary;
                    if let Some(record) = records_by_id.get(&server_id.0) {
                        summary.enabled = record.enabled;
                        summary.manageable = true;
                        if !record.enabled {
                            summary.status = "disabled";
                            summary.exposed_tool_count = 0;
                        }
                    }
                    apply_mcp_last_diagnostic(&mut summary, last_diagnostics.get(&server_id.0));
                    servers.insert(server_id.0.clone(), summary);
                }
            }
        }
    }

    Ok(ListMcpServersResponse {
        servers: servers.into_values().collect(),
    })
}

pub async fn save_mcp_server_with_store(
    request: SaveMcpServerRequest,
    store: &dyn McpServerStore,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_request(&request)?;
    let record = McpServerConfigRecord {
        enabled: request.enabled,
        display_name: request.display_name.trim().to_owned(),
        id: request.id.trim().to_owned(),
        scope: request.scope,
        transport: request.transport,
    };

    store.save_record(&record)?;

    Ok(SaveMcpServerResponse {
        server: mcp_server_summary_from_record(&record),
    })
}

pub async fn save_mcp_server_with_runtime_state(
    request: SaveMcpServerRequest,
    state: &DesktopRuntimeState,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_request(&request)?;
    let record = McpServerConfigRecord {
        enabled: request.enabled,
        display_name: request.display_name.trim().to_owned(),
        id: request.id.trim().to_owned(),
        scope: request.scope,
        transport: request.transport,
    };

    state.mcp_server_store.save_record(&record)?;

    let Some(harness) = state.harness() else {
        return Ok(SaveMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    };
    remove_mcp_server_from_harness(&harness, &record.id).await?;
    if !record.enabled {
        return Ok(SaveMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    }
    let server =
        register_mcp_record_with_harness(&record, &harness, state.default_conversation_id, state)
            .await?;

    Ok(SaveMcpServerResponse { server })
}

pub async fn get_mcp_server_config_with_store(
    request: GetMcpServerConfigRequest,
    store: &dyn McpServerStore,
) -> Result<GetMcpServerConfigResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    let record = store
        .load_records()?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| not_found(format!("mcp server not found: {id}")))?;
    ensure_mcp_server_record(&record)?;

    Ok(GetMcpServerConfigResponse { server: record })
}

pub async fn get_mcp_server_config_with_runtime_state(
    request: GetMcpServerConfigRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMcpServerConfigResponse, CommandErrorPayload> {
    get_mcp_server_config_with_store(request, state.mcp_server_store.as_ref()).await
}

pub async fn delete_mcp_server_with_store(
    request: DeleteMcpServerRequest,
    store: &dyn McpServerStore,
) -> Result<DeleteMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    store.delete_record(request.id.trim())?;

    Ok(DeleteMcpServerResponse {
        id: request.id.trim().to_owned(),
        status: "deleted",
    })
}

pub async fn delete_mcp_server_with_runtime_state(
    request: DeleteMcpServerRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    state.mcp_server_store.delete_record(id)?;
    if let Some(harness) = state.harness() {
        remove_mcp_server_from_harness(&harness, id).await?;
    }

    Ok(DeleteMcpServerResponse {
        id: id.to_owned(),
        status: "deleted",
    })
}

pub async fn set_mcp_server_enabled_with_runtime_state(
    request: SetMcpServerEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetMcpServerEnabledResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    let mut records = state.mcp_server_store.load_records()?;
    let Some(record) = records.iter_mut().find(|record| record.id == id) else {
        return Err(not_found(format!("mcp server not found: {id}")));
    };
    record.enabled = request.enabled;
    ensure_mcp_server_record(record)?;
    let record = record.clone();
    state.mcp_server_store.save_record(&record)?;

    let Some(harness) = state.harness() else {
        return Ok(SetMcpServerEnabledResponse {
            server: mcp_server_summary_from_record(&record),
        });
    };

    remove_mcp_server_from_harness(&harness, &record.id).await?;
    if !record.enabled {
        return Ok(SetMcpServerEnabledResponse {
            server: mcp_server_summary_from_record(&record),
        });
    }

    let server =
        register_mcp_record_with_harness(&record, &harness, state.default_conversation_id, state)
            .await?;
    Ok(SetMcpServerEnabledResponse { server })
}

pub async fn restart_mcp_server_with_runtime_state(
    request: RestartMcpServerRequest,
    state: &DesktopRuntimeState,
) -> Result<RestartMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    let record = state
        .mcp_server_store
        .load_records()?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| not_found(format!("mcp server not found: {id}")))?;
    ensure_mcp_server_record(&record)?;

    let Some(harness) = state.harness() else {
        return Ok(RestartMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    };

    remove_mcp_server_from_harness(&harness, &record.id).await?;
    if !record.enabled {
        return Ok(RestartMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    }

    let server =
        register_mcp_record_with_harness(&record, &harness, state.default_conversation_id, state)
            .await?;
    Ok(RestartMcpServerResponse { server })
}

pub async fn list_mcp_diagnostics_with_store(
    server_id: Option<String>,
    store: &dyn McpDiagnosticStore,
) -> Result<ListMcpDiagnosticsResponse, CommandErrorPayload> {
    if let Some(server_id) = server_id.as_deref() {
        ensure_mcp_server_id(server_id)?;
    }
    let events = store
        .load_records()?
        .into_iter()
        .filter(|record| {
            server_id
                .as_deref()
                .is_none_or(|server_id| record.server_id == server_id)
        })
        .collect();
    Ok(ListMcpDiagnosticsResponse { events })
}

pub async fn list_mcp_diagnostics_with_runtime_state(
    request: ListMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<ListMcpDiagnosticsResponse, CommandErrorPayload> {
    list_mcp_diagnostics_with_store(request.server_id, state.mcp_diagnostic_store.as_ref()).await
}

pub async fn clear_mcp_diagnostics_with_runtime_state(
    request: ClearMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<ClearMcpDiagnosticsResponse, CommandErrorPayload> {
    if let Some(server_id) = request.server_id.as_deref() {
        ensure_mcp_server_id(server_id)?;
    }
    state
        .mcp_diagnostic_store
        .clear_records(request.server_id.as_deref())?;
    Ok(ClearMcpDiagnosticsResponse { status: "cleared" })
}

pub async fn subscribe_mcp_diagnostics_with_runtime_state(
    request: SubscribeMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<SubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    subscribe_mcp_diagnostics_for_window_with_runtime_state(
        request,
        "default".to_owned(),
        Arc::new(|_batch| Ok(())),
        state,
    )
    .await
}

pub async fn subscribe_mcp_diagnostics_for_window_with_runtime_state(
    request: SubscribeMcpDiagnosticsRequest,
    window_label: String,
    emitter: McpDiagnosticBatchEmitter,
    state: &DesktopRuntimeState,
) -> Result<SubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    ensure_non_empty("windowLabel", &window_label)?;
    if let Some(server_id) = request.server_id.as_deref() {
        ensure_mcp_server_id(server_id)?;
    }
    let replay_events = list_mcp_diagnostics_with_store(
        request.server_id.clone(),
        state.mcp_diagnostic_store.as_ref(),
    )
    .await?
    .events;
    let subscription_id = format!("mcp-diagnostic-subscription-{}", EventId::new());
    let handle = spawn_mcp_diagnostic_subscription(
        subscription_id.clone(),
        request.server_id.clone(),
        replay_events.iter().map(|event| event.id.clone()).collect(),
        window_label.clone(),
        Arc::clone(&emitter),
        state.clone(),
    );
    state.mcp_diagnostic_subscriptions.lock().await.insert(
        subscription_id.clone(),
        McpDiagnosticSubscriptionHandle {
            task: handle,
            window_label,
        },
    );

    Ok(SubscribeMcpDiagnosticsResponse {
        subscription_id,
        server_id: request.server_id,
        replay_events,
    })
}

pub async fn unsubscribe_mcp_diagnostics_with_runtime_state(
    request: UnsubscribeMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    unsubscribe_mcp_diagnostics_for_window_with_runtime_state(request, "default".to_owned(), state)
        .await
}

pub async fn unsubscribe_mcp_diagnostics_for_window_with_runtime_state(
    request: UnsubscribeMcpDiagnosticsRequest,
    window_label: String,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    ensure_non_empty("subscriptionId", &request.subscription_id)?;
    ensure_non_empty("windowLabel", &window_label)?;
    let mut subscriptions = state.mcp_diagnostic_subscriptions.lock().await;
    let removed = match subscriptions.get(&request.subscription_id) {
        Some(subscription) if subscription.window_label != window_label => {
            return Err(invalid_payload(
                "subscription does not belong to this window".to_owned(),
            ));
        }
        Some(_) => subscriptions.remove(&request.subscription_id),
        None => None,
    };
    drop(subscriptions);

    if let Some(subscription) = removed {
        subscription.task.abort();
        return Ok(UnsubscribeMcpDiagnosticsResponse {
            subscription_id: request.subscription_id,
            status: "unsubscribed",
        });
    }

    Ok(UnsubscribeMcpDiagnosticsResponse {
        subscription_id: request.subscription_id,
        status: "alreadyClosed",
    })
}

fn spawn_mcp_diagnostic_subscription(
    subscription_id: String,
    server_id: Option<String>,
    mut seen_ids: HashSet<String>,
    window_label: String,
    emitter: McpDiagnosticBatchEmitter,
    state: DesktopRuntimeState,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(MCP_DIAGNOSTIC_SUBSCRIPTION_POLL_INTERVAL).await;
            let records = match state.mcp_diagnostic_store.load_records() {
                Ok(records) => records,
                Err(_) => break,
            };
            let events = records
                .into_iter()
                .filter(|record| {
                    server_id
                        .as_deref()
                        .is_none_or(|server_id| record.server_id == server_id)
                        && !seen_ids.contains(&record.id)
                })
                .collect::<Vec<_>>();
            if events.is_empty() {
                continue;
            }

            let mut emit_failed = false;
            for chunk in events.chunks(MCP_DIAGNOSTIC_SUBSCRIPTION_BATCH_LIMIT) {
                for event in chunk {
                    seen_ids.insert(event.id.clone());
                }
                let batch = McpDiagnosticBatchPayload {
                    subscription_id: subscription_id.clone(),
                    server_id: server_id.clone(),
                    events: chunk.to_vec(),
                    phase: "live",
                };
                if emitter(batch).is_err() {
                    emit_failed = true;
                    break;
                }
            }
            if emit_failed {
                break;
            }
        }

        state
            .mcp_diagnostic_subscriptions
            .lock()
            .await
            .remove(&subscription_id);
        let _ = window_label;
    })
}

pub async fn list_skills_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListSkillsResponse, CommandErrorPayload> {
    let records = state.skill_store.load_records()?;
    let runtime = state
        .harness()
        .map(|harness| harness.list_runtime_skills())
        .unwrap_or_default();
    Ok(ListSkillsResponse {
        skills: skill_summaries_from_records_and_runtime(&records, &runtime),
    })
}

pub async fn list_skill_catalog_sources_with_runtime_state(
) -> Result<ListSkillCatalogSourcesResponse, CommandErrorPayload> {
    Ok(list_catalog_sources_payload())
}

pub async fn list_skill_catalog_entries_with_runtime_state(
    request: ListSkillCatalogEntriesRequest,
    state: &DesktopRuntimeState,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    let installed_entry_ids = installed_catalog_entry_ids(state)?;
    list_catalog_entries_payload(request, &installed_entry_ids).await
}

pub async fn get_skill_catalog_entry_with_runtime_state(
    request: GetSkillCatalogEntryRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload> {
    let installed_entry_ids = installed_catalog_entry_ids(state)?;
    let mut response = get_catalog_entry_payload(request, &installed_entry_ids).await?;
    if active_skill_names(state)?.contains(response.entry.name.as_str()) {
        mark_catalog_entry_name_conflict(&mut response);
    }
    Ok(response)
}

pub async fn get_skill_catalog_file_with_runtime_state(
    request: GetSkillCatalogFileRequest,
    _state: &DesktopRuntimeState,
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload> {
    get_catalog_file_payload(request).await
}

pub async fn list_skill_catalog_install_tasks_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListSkillCatalogInstallTasksResponse, CommandErrorPayload> {
    let mut tasks = state
        .skill_catalog_install_tasks
        .read()
        .map_err(|_| {
            runtime_operation_failed("skill catalog install tasks unavailable".to_owned())
        })?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then(left.entry_id.cmp(&right.entry_id))
            .then(left.version.cmp(&right.version))
    });
    Ok(ListSkillCatalogInstallTasksResponse { tasks })
}

pub async fn install_skill_from_catalog_with_runtime_state(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
) -> Result<InstallSkillFromCatalogResponse, CommandErrorPayload> {
    start_skill_catalog_install_task_with_runtime_state(request, state.clone(), None).await
}

pub async fn start_skill_catalog_install_task_with_runtime_state(
    request: InstallSkillFromCatalogRequest,
    state: DesktopRuntimeState,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> Result<InstallSkillFromCatalogResponse, CommandErrorPayload> {
    let (task, request, created) =
        get_or_create_skill_catalog_install_task_record(&state, &request)?;
    if !created {
        return Ok(InstallSkillFromCatalogResponse { task });
    }

    let state_for_task = state.clone();
    let request_for_task = request.clone();
    let recording_emitter = skill_catalog_install_task_emitter(state, request, emitter);
    tauri::async_runtime::spawn(async move {
        let _skill_store_guard = state_for_task.skill_store_lock.lock().await;
        let _ = install_skill_from_catalog_with_progress(
            request_for_task,
            &state_for_task,
            Some(recording_emitter),
        )
        .await;
    });

    Ok(InstallSkillFromCatalogResponse { task })
}

pub async fn install_skill_from_catalog_package_with_runtime_state(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    install_skill_from_catalog_with_progress(
        request,
        state,
        None::<SkillCatalogInstallProgressEmitter>,
    )
    .await
}

pub async fn install_skill_from_catalog_with_progress(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    validate_catalog_install_operation_id(&request)?;
    let result: Result<ImportSkillResponse, CommandErrorPayload> = async {
        emit_skill_catalog_install_progress(&emitter, &request, "preparing", 5, None);
        let catalog_progress = |stage: &str, percent: u8| {
            emit_skill_catalog_install_progress(&emitter, &request, stage, percent, None);
        };
        let materialized =
            materialize_skill_from_catalog_with_progress(request.clone(), Some(&catalog_progress))
                .await?;
        let response = install_skill_package_with_progress(
            materialized.package_path.clone(),
            Some(materialized.origin.clone()),
            state,
            Some((&emitter, &request)),
        )
        .await?;
        drop(materialized);
        emit_skill_catalog_install_progress(&emitter, &request, "completed", 100, None);
        Ok(response)
    }
    .await;

    if let Err(error) = &result {
        emit_skill_catalog_install_progress(
            &emitter,
            &request,
            "failed",
            100,
            Some(error.message.clone()),
        );
    }

    result
}

#[cfg(test)]
fn get_or_create_skill_catalog_install_task(
    state: &DesktopRuntimeState,
    request: &InstallSkillFromCatalogRequest,
) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
    let (task, _, _) = get_or_create_skill_catalog_install_task_record(state, request)?;
    Ok(task)
}

fn get_or_create_skill_catalog_install_task_record(
    state: &DesktopRuntimeState,
    request: &InstallSkillFromCatalogRequest,
) -> Result<
    (
        SkillCatalogInstallTaskPayload,
        InstallSkillFromCatalogRequest,
        bool,
    ),
    CommandErrorPayload,
> {
    let key = skill_catalog_install_task_key(request)?;
    let mut tasks = state.skill_catalog_install_tasks.write().map_err(|_| {
        runtime_operation_failed("skill catalog install tasks unavailable".to_owned())
    })?;
    if let Some(existing) = tasks.get(&key) {
        if existing.status == "running" {
            let request = InstallSkillFromCatalogRequest {
                operation_id: Some(existing.operation_id.clone()),
                ..request.clone()
            };
            return Ok((existing.clone(), request, false));
        }
    }

    let operation_id = match request.operation_id.as_deref() {
        Some(operation_id) => {
            ensure_non_empty("operationId", operation_id)?;
            operation_id.to_owned()
        }
        None => catalog_install_operation_id(),
    };
    let now = now().to_rfc3339();
    let task = SkillCatalogInstallTaskPayload {
        operation_id: operation_id.clone(),
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
        stage: "preparing".to_owned(),
        percent: 5,
        status: "running".to_owned(),
        message: None,
        started_at: now.clone(),
        updated_at: now,
    };
    tasks.insert(key, task.clone());
    let request = InstallSkillFromCatalogRequest {
        operation_id: Some(operation_id),
        ..request.clone()
    };
    Ok((task, request, true))
}

#[cfg(test)]
async fn record_skill_catalog_install_task_progress(
    state: &DesktopRuntimeState,
    request: &InstallSkillFromCatalogRequest,
    stage: &str,
    percent: u8,
    message: Option<String>,
) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
    let operation_id = request
        .operation_id
        .as_deref()
        .ok_or_else(|| invalid_payload("operationId is required".to_owned()))?;
    let payload = SkillCatalogInstallProgressPayload {
        operation_id: operation_id.to_owned(),
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
        stage: skill_catalog_install_stage(stage),
        percent,
        message,
    };
    record_skill_catalog_install_task_payload(state, payload)
}

fn record_skill_catalog_install_task_payload(
    state: &DesktopRuntimeState,
    payload: SkillCatalogInstallProgressPayload,
) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
    let key = SkillCatalogInstallTaskKey {
        source_id: payload.source_id.clone(),
        entry_id: payload.entry_id.clone(),
        version: payload.version.clone(),
    };
    let mut tasks = state.skill_catalog_install_tasks.write().map_err(|_| {
        runtime_operation_failed("skill catalog install tasks unavailable".to_owned())
    })?;
    let now = now().to_rfc3339();
    let task = tasks
        .entry(key)
        .or_insert_with(|| SkillCatalogInstallTaskPayload {
            operation_id: payload.operation_id.clone(),
            source_id: payload.source_id.clone(),
            entry_id: payload.entry_id.clone(),
            version: payload.version.clone(),
            stage: "preparing".to_owned(),
            percent: 5,
            status: "running".to_owned(),
            message: None,
            started_at: now.clone(),
            updated_at: now.clone(),
        });
    task.operation_id = payload.operation_id;
    task.stage = payload.stage.to_owned();
    task.percent = payload.percent.min(100);
    task.status = match payload.stage {
        "completed" => "completed",
        "failed" => "failed",
        _ => "running",
    }
    .to_owned();
    task.message = payload.message;
    task.updated_at = now;
    Ok(task.clone())
}

fn skill_catalog_install_task_emitter(
    state: DesktopRuntimeState,
    request: InstallSkillFromCatalogRequest,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> SkillCatalogInstallProgressEmitter {
    Arc::new(move |payload| {
        let _ = record_skill_catalog_install_task_payload(&state, payload.clone());
        if payload.operation_id == request.operation_id.clone().unwrap_or_default() {
            if let Some(emitter) = &emitter {
                emitter(payload);
            }
        }
    })
}

fn skill_catalog_install_task_key(
    request: &InstallSkillFromCatalogRequest,
) -> Result<SkillCatalogInstallTaskKey, CommandErrorPayload> {
    ensure_non_empty("sourceId", &request.source_id)?;
    ensure_non_empty("entryId", &request.entry_id)?;
    if let Some(version) = request.version.as_deref() {
        ensure_non_empty("version", version)?;
    }
    Ok(SkillCatalogInstallTaskKey {
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
    })
}

fn catalog_install_operation_id() -> String {
    format!("catalog-install-{}", skill_import_id())
}

fn validate_catalog_install_operation_id(
    request: &InstallSkillFromCatalogRequest,
) -> Result<(), CommandErrorPayload> {
    if let Some(operation_id) = request.operation_id.as_deref() {
        ensure_non_empty("operationId", operation_id)?;
    }
    Ok(())
}

fn emit_skill_catalog_install_progress(
    emitter: &Option<SkillCatalogInstallProgressEmitter>,
    request: &InstallSkillFromCatalogRequest,
    stage: &str,
    percent: u8,
    message: Option<String>,
) {
    let Some(operation_id) = request.operation_id.clone() else {
        return;
    };
    let Some(emitter) = emitter else {
        return;
    };
    let stage = skill_catalog_install_stage(stage);
    let payload = SkillCatalogInstallProgressPayload {
        operation_id,
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
        stage,
        percent: percent.min(100),
        message,
    };
    // Progress events are UI telemetry. Failure to emit must not change install policy.
    emitter(payload);
}

fn skill_catalog_install_stage(stage: &str) -> &'static str {
    match stage {
        "preparing" => "preparing",
        "resolving" => "resolving",
        "checking" => "checking",
        "downloading" => "downloading",
        "validating" => "validating",
        "copying" => "copying",
        "reloading" => "reloading",
        "completed" => "completed",
        "failed" => "failed",
        _ => "preparing",
    }
}

fn installed_catalog_entry_ids(
    state: &DesktopRuntimeState,
) -> Result<HashSet<String>, CommandErrorPayload> {
    Ok(state
        .skill_store
        .load_records()?
        .into_iter()
        .filter_map(|record| record.origin.map(|origin| origin.entry_id))
        .collect())
}

fn active_skill_names(state: &DesktopRuntimeState) -> Result<HashSet<String>, CommandErrorPayload> {
    let mut names = state
        .skill_store
        .load_records()?
        .into_iter()
        .filter(|record| record.enabled)
        .map(|record| record.name)
        .collect::<HashSet<_>>();
    if let Some(harness) = state.harness() {
        names.extend(
            harness
                .list_runtime_skills()
                .into_iter()
                .map(|skill| skill.name),
        );
    }
    Ok(names)
}

pub async fn import_skill_with_runtime_state(
    request: ImportSkillRequest,
    state: &DesktopRuntimeState,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    let source_path = ensure_import_skill_source_path(&request.source_path)?;
    install_skill_package_with_runtime_state(source_path, None, state).await
}

async fn install_skill_package_with_runtime_state(
    source_path: PathBuf,
    origin: Option<SkillInstallOriginRecord>,
    state: &DesktopRuntimeState,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    install_skill_package_with_progress(source_path, origin, state, None).await
}

async fn install_skill_package_with_progress(
    source_path: PathBuf,
    origin: Option<SkillInstallOriginRecord>,
    state: &DesktopRuntimeState,
    progress_context: Option<(
        &Option<SkillCatalogInstallProgressEmitter>,
        &InstallSkillFromCatalogRequest,
    )>,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    let harness = state.harness().ok_or_else(|| {
        runtime_unavailable("Importing skills requires the runtime skill facade.")
    })?;
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "validating", 65, None);
    }
    let entry_path = source_path.join(SKILL_PACKAGE_ENTRY_FILE);
    let entry_metadata = std::fs::metadata(&entry_path).map_err(|error| {
        runtime_operation_failed(format!("skill entry metadata failed: {error}"))
    })?;
    if entry_metadata.len() > MAX_SKILL_MARKDOWN_BYTES {
        return Err(invalid_payload("skill entry file is too large".to_owned()));
    }
    let bytes = std::fs::read(&entry_path)
        .map_err(|error| runtime_operation_failed(format!("skill entry read failed: {error}")))?;
    let markdown = String::from_utf8(bytes)
        .map_err(|_| invalid_payload("skill entry file must be valid UTF-8".to_owned()))?;
    let validated = harness
        .validate_workspace_skill_markdown(&markdown, Some(entry_path))
        .await
        .map_err(|error| invalid_payload(error.to_string()))?;
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "validating", 72, None);
    }
    let content_hash = hash_skill_package(&source_path)?;

    let mut records = state.skill_store.load_records()?;
    let previous_records = records.clone();
    if records
        .iter()
        .any(|record| record.enabled && record.name == validated.summary.name)
        || harness
            .list_runtime_skills()
            .iter()
            .any(|skill| skill.name == validated.summary.name)
    {
        return Err(invalid_payload(format!(
            "active skill name already exists: {}",
            validated.summary.name
        )));
    }

    let id = skill_import_id();
    let now = now().to_rfc3339();
    let record = SkillStoreRecord {
        id: id.clone(),
        name: validated.summary.name,
        description: validated.summary.description,
        enabled: true,
        content_hash,
        package_dir: id.clone(),
        file_name: String::new(),
        imported_at: now.clone(),
        updated_at: now,
        tags: validated.summary.tags,
        category: validated.summary.category,
        last_validation_error: None,
        origin,
    };
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "copying", 82, None);
    }
    state
        .skill_store
        .write_skill_package(&record.id, true, &source_path)?;
    records.retain(|existing| existing.id != record.id);
    records.push(record.clone());
    records.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    if let Err(error) = state.skill_store.save_records(&records) {
        let _ = state.skill_store.delete_skill_package(&record.id);
        return Err(error);
    }
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "reloading", 95, None);
    }
    if let Err(error) = reload_managed_skills(state, &harness).await {
        let _ = state.skill_store.delete_skill_package(&record.id);
        let _ = state.skill_store.save_records(&previous_records);
        let _ = reload_managed_skills(state, &harness).await;
        return Err(error);
    }

    Ok(ImportSkillResponse {
        skill: managed_skill_summary(&record, runtime_status_for_name(&harness, &record.name)),
    })
}

pub async fn get_skill_detail_with_runtime_state(
    request: GetSkillDetailRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillDetailResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let records = state.skill_store.load_records()?;
    let record = records.iter().find(|record| record.id == request.id);
    let harness = state.harness();

    let Some(record) = record else {
        let harness = harness
            .as_ref()
            .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
        let view = harness
            .view_runtime_skill(&request.id, false)
            .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
        return Ok(GetSkillDetailResponse {
            skill: skill_detail_from_runtime_view(
                runtime_skill_summary_payload(&view.summary),
                view,
                Vec::new(),
                None,
            ),
        });
    };

    let runtime_view = harness.as_ref().and_then(|harness| {
        record
            .enabled
            .then(|| harness.view_runtime_skill(&record.name, false))
            .flatten()
    });
    let files = state.skill_store.list_skill_package_files(record)?;
    let detail = if let Some(view) = runtime_view {
        let status = skill_status_string(&view.summary.status);
        skill_detail_from_runtime_view(
            managed_skill_summary(record, Some(status)),
            view,
            files,
            record.last_validation_error.clone(),
        )
    } else {
        SkillDetailPayload {
            summary: managed_skill_summary(record, None),
            parameters: Vec::new(),
            config_keys: Vec::new(),
            files,
            body_preview: String::new(),
            validation_error: record.last_validation_error.clone(),
        }
    };
    Ok(GetSkillDetailResponse { skill: detail })
}

pub async fn get_skill_file_with_runtime_state(
    request: GetSkillFileRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillFileResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let records = state.skill_store.load_records()?;
    let record = records
        .iter()
        .find(|record| record.id == request.id)
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    let files = state.skill_store.list_skill_package_files(record)?;
    if !files
        .iter()
        .any(|file| file.kind == "file" && file.path == request.path)
    {
        return Err(invalid_payload("skill file not found".to_owned()));
    }
    Ok(GetSkillFileResponse {
        file: state
            .skill_store
            .read_skill_package_file(record, &request.path)?,
    })
}

pub async fn set_skill_enabled_with_runtime_state(
    request: SetSkillEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetSkillEnabledResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let harness = state.harness().ok_or_else(|| {
        runtime_unavailable("Changing skill state requires the runtime skill facade.")
    })?;
    let mut records = state.skill_store.load_records()?;
    let record_index = records
        .iter()
        .position(|record| record.id == request.id)
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    let record_name = records[record_index].name.clone();
    if records[record_index].enabled != request.enabled {
        if request.enabled
            && (records.iter().any(|candidate| {
                candidate.enabled && candidate.name == record_name && candidate.id != request.id
            }) || harness
                .list_runtime_skills()
                .iter()
                .any(|skill| skill.name == record_name))
        {
            return Err(invalid_payload(format!(
                "active skill name already exists: {}",
                record_name
            )));
        }
        state
            .skill_store
            .move_skill_package(&request.id, request.enabled)?;
        records[record_index].enabled = request.enabled;
        records[record_index].updated_at = now().to_rfc3339();
        records[record_index].last_validation_error = None;
        state.skill_store.save_records(&records)?;
        reload_managed_skills(state, &harness).await?;
    }
    let record = records[record_index].clone();
    Ok(SetSkillEnabledResponse {
        skill: managed_skill_summary(&record, runtime_status_for_name(&harness, &record.name)),
    })
}

pub async fn delete_skill_with_runtime_state(
    request: DeleteSkillRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteSkillResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let harness = state
        .harness()
        .ok_or_else(|| runtime_unavailable("Deleting skills requires the runtime skill facade."))?;
    let mut records = state.skill_store.load_records()?;
    let original_len = records.len();
    records.retain(|record| record.id != request.id);
    if records.len() == original_len {
        return Err(invalid_payload("skill not found".to_owned()));
    }
    state.skill_store.delete_skill_package(&request.id)?;
    state.skill_store.save_records(&records)?;
    reload_managed_skills(state, &harness).await?;
    Ok(DeleteSkillResponse {
        id: request.id,
        status: "deleted",
    })
}

async fn reload_managed_skills(
    state: &DesktopRuntimeState,
    harness: &Harness,
) -> Result<(), CommandErrorPayload> {
    harness
        .reload_workspace_managed_skills(state.skill_store.enabled_dir())
        .await
        .map_err(|error| runtime_operation_failed(format!("skill reload failed: {error}")))
}

pub async fn list_plugins_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListPluginsResponse, CommandErrorPayload> {
    let settings = state.plugin_store.load_record()?;
    if let Some(harness) = state.harness() {
        if let Some(registry) = harness.plugin_registry() {
            registry.discover().await.map_err(|error| {
                runtime_operation_failed(format!("plugin discovery failed: {error}"))
            })?;
            return Ok(ListPluginsResponse {
                allow_project_plugins: settings.allow_project_plugins,
                plugins: registry.product_snapshot(),
            });
        }
    }

    let registry = build_plugin_registry(&state.workspace_root, state.plugin_store.as_ref())?;
    registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    Ok(ListPluginsResponse {
        allow_project_plugins: settings.allow_project_plugins,
        plugins: registry.product_snapshot(),
    })
}

pub async fn get_plugin_detail_with_runtime_state(
    request: GetPluginDetailRequest,
    state: &DesktopRuntimeState,
) -> Result<GetPluginDetailResponse, CommandErrorPayload> {
    if let Some(harness) = state.harness() {
        if let Some(registry) = harness.plugin_registry() {
            registry.discover().await.map_err(|error| {
                runtime_operation_failed(format!("plugin discovery failed: {error}"))
            })?;
            let plugin = registry
                .product_detail(&request.plugin_id)
                .map(redact_plugin_detail_config)
                .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
            return Ok(GetPluginDetailResponse { plugin });
        }
    }

    let registry = build_plugin_registry(&state.workspace_root, state.plugin_store.as_ref())?;
    registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    let plugin = registry
        .product_detail(&request.plugin_id)
        .map(redact_plugin_detail_config)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    Ok(GetPluginDetailResponse { plugin })
}

pub async fn validate_plugin_from_path_with_runtime_state(
    request: ValidatePluginFromPathRequest,
    _state: &DesktopRuntimeState,
) -> Result<PluginInstallReport, CommandErrorPayload> {
    let source_path = ensure_plugin_source_path(&request.source_path)?;
    validate_plugin_source_path(&source_path).await
}

pub async fn install_plugin_from_path_with_runtime_state(
    request: InstallPluginFromPathRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let source_path = ensure_plugin_source_path(&request.source_path)?;
    let report = validate_plugin_source_path(&source_path).await?;
    let Some(summary) = report.summary.clone() else {
        return Ok(PluginOperationResult {
            plugin_id: None,
            status: PluginOperationStatus::Rejected,
            summary: None,
            report: Some(report),
        });
    };
    if !report.valid {
        return Ok(PluginOperationResult {
            plugin_id: Some(summary.id.clone()),
            status: PluginOperationStatus::Rejected,
            summary: Some(summary),
            report: Some(report),
        });
    }

    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    if settings
        .records
        .iter()
        .any(|record| record.name == summary.name || record.plugin_id == summary.id)
    {
        return Ok(PluginOperationResult {
            plugin_id: Some(summary.id.clone()),
            status: PluginOperationStatus::Rejected,
            summary: Some(summary.clone()),
            report: Some(PluginInstallReport {
                source_path: plugin_report_source_path(&source_path),
                valid: false,
                summary: Some(summary),
                warnings: Vec::new(),
                reason: Some("plugin is already installed".to_owned()),
            }),
        });
    }

    let package_dir = plugin_package_dir_name(&summary.id);
    let content_hash = hash_plugin_package(&source_path)?;
    state
        .plugin_store
        .write_plugin_package(&package_dir, &source_path)?;
    let installed_package = state.plugin_store.package_root().join(&package_dir);
    let installed_hash = hash_plugin_package(&installed_package)?;
    if installed_hash != content_hash {
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Err(runtime_operation_failed(
            "plugin package changed while it was being installed".to_owned(),
        ));
    }
    let installed_report = validate_plugin_source_path(&installed_package).await?;
    let installed_summary = installed_report.summary.as_ref();
    let installed_matches_source = installed_report.valid
        && installed_summary.is_some_and(|installed_summary| {
            installed_summary.id == summary.id
                && installed_summary.name == summary.name
                && installed_summary.version == summary.version
        });
    if !installed_matches_source {
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Ok(PluginOperationResult {
            plugin_id: Some(summary.id.clone()),
            status: PluginOperationStatus::Rejected,
            summary: Some(summary.clone()),
            report: Some(PluginInstallReport {
                source_path: plugin_report_source_path(&source_path),
                valid: false,
                summary: Some(summary),
                warnings: Vec::new(),
                reason: Some(installed_report.reason.unwrap_or_else(|| {
                    "installed plugin package did not match validation".to_owned()
                })),
            }),
        });
    }
    let now = now().to_rfc3339();
    settings.records.push(PluginStoreRecord {
        plugin_id: summary.id.clone(),
        name: summary.name.clone(),
        version: summary.version.clone(),
        enabled: false,
        package_dir: package_dir.clone(),
        source_path: source_path.display().to_string(),
        content_hash,
        imported_at: now.clone(),
        updated_at: now,
        config: Value::Null,
        last_validation_error: None,
    });
    settings.records.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
            .then(left.plugin_id.cmp(&right.plugin_id))
    });
    if let Err(error) = state.plugin_store.save_record(&settings) {
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Err(error);
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Err(error);
    }

    Ok(PluginOperationResult {
        plugin_id: Some(summary.id.clone()),
        status: PluginOperationStatus::Installed,
        summary: Some(summary),
        report: Some(report),
    })
}

pub async fn set_plugin_enabled_with_runtime_state(
    request: SetPluginEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    let record = settings
        .records
        .iter_mut()
        .find(|record| record.plugin_id == request.plugin_id)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    record.enabled = request.enabled;
    record.updated_at = now().to_rfc3339();
    state.plugin_store.save_record(&settings)?;
    if request.enabled {
        if let Err(error) = preflight_plugin_activation(state, &request.plugin_id).await {
            let _ = state.plugin_store.save_record(&previous_settings);
            return Err(error);
        }
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        return Err(error);
    }
    let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: if request.enabled {
            PluginOperationStatus::Enabled
        } else {
            PluginOperationStatus::Disabled
        },
        summary,
        report: None,
    })
}

pub async fn set_project_plugins_enabled_with_runtime_state(
    request: SetProjectPluginsEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetProjectPluginsEnabledResponse, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    settings.allow_project_plugins = request.enabled;
    state.plugin_store.save_record(&settings)?;
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        return Err(error);
    }
    Ok(SetProjectPluginsEnabledResponse {
        allow_project_plugins: settings.allow_project_plugins,
    })
}

pub async fn update_plugin_config_with_runtime_state(
    request: UpdatePluginConfigRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    let record_index = settings
        .records
        .iter()
        .position(|record| record.plugin_id == request.plugin_id)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    let registry = build_plugin_registry(&state.workspace_root, state.plugin_store.as_ref())?;
    let discovered = registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    ensure_no_secret_like_config_values(&request.values)?;
    let private_schema = discovered
        .iter()
        .find(|plugin| plugin.record.manifest.plugin_id() == request.plugin_id)
        .and_then(|plugin| {
            plugin
                .record
                .manifest
                .capabilities
                .configuration_schema
                .as_ref()
        });
    if registry.product_detail(&request.plugin_id).is_none() {
        return Err(invalid_payload("plugin not found".to_owned()));
    }
    let merged_config = merge_plugin_config_values(
        private_schema,
        &settings.records[record_index].config,
        request.values.clone(),
    );
    let validation_config = redact_secret_config_values(private_schema, merged_config.clone());
    registry
        .validate_config_update(&PluginConfigUpdate {
            plugin_id: request.plugin_id.clone(),
            values: validation_config,
        })
        .map_err(|error| invalid_payload(format!("plugin config rejected: {error}")))?;
    settings.records[record_index].config = merged_config;
    settings.records[record_index].updated_at = now().to_rfc3339();
    state.plugin_store.save_record(&settings)?;
    if settings.records[record_index].enabled {
        if let Err(error) = preflight_plugin_activation(state, &request.plugin_id).await {
            let _ = state.plugin_store.save_record(&previous_settings);
            return Err(error);
        }
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        return Err(error);
    }
    let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: PluginOperationStatus::Configured,
        summary,
        report: None,
    })
}

pub async fn uninstall_plugin_with_runtime_state(
    request: UninstallPluginRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    let original_len = settings.records.len();
    let mut package_dirs = Vec::new();
    settings.records.retain(|record| {
        if record.plugin_id == request.plugin_id {
            package_dirs.push(record.package_dir.clone());
            false
        } else {
            true
        }
    });
    if settings.records.len() == original_len {
        return Err(invalid_payload("plugin not found".to_owned()));
    }
    state.plugin_store.save_record(&settings)?;
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        return Err(error);
    }
    for package_dir in &package_dirs {
        if let Err(error) = state.plugin_store.delete_plugin_package(package_dir) {
            let _ = state.plugin_store.save_record(&previous_settings);
            let _ = reload_desktop_harness_after_plugin_change_locked(state).await;
            return Err(error);
        }
    }
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: PluginOperationStatus::Uninstalled,
        summary: None,
        report: None,
    })
}

pub async fn reload_plugin_with_runtime_state(
    request: ReloadPluginRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let settings = state.plugin_store.load_record()?;
    let enabled = settings
        .records
        .iter()
        .find(|record| record.plugin_id == request.plugin_id)
        .map(|record| record.enabled)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    if enabled {
        preflight_plugin_activation(state, &request.plugin_id).await?;
    }
    reload_desktop_harness_after_plugin_change_locked(state).await?;
    let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: PluginOperationStatus::Reloaded,
        summary,
        report: None,
    })
}

async fn preflight_plugin_activation(
    state: &DesktopRuntimeState,
    plugin_id: &PluginId,
) -> Result<(), CommandErrorPayload> {
    let registry = build_plugin_registry(&state.workspace_root, state.plugin_store.as_ref())?;
    let discovered = registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    if let Some(plugin) = discovered
        .iter()
        .find(|plugin| plugin.record.manifest.plugin_id() == *plugin_id)
    {
        if matches!(plugin.record.origin, ManifestOrigin::CargoExtension { .. }) {
            return Ok(());
        }
        return Err(invalid_payload(format!(
            "plugin cannot be enabled: {LOCAL_PLUGIN_SIDECAR_REQUIRED_REASON}"
        )));
    }
    let reason = registry
        .state_detail(plugin_id)
        .and_then(|detail| detail.rejection_reason)
        .map(|reason| plugin_rejection_report_reason(&reason))
        .unwrap_or_else(|| "plugin was not discovered".to_owned());
    Err(invalid_payload(format!(
        "plugin cannot be enabled: {reason}"
    )))
}

async fn plugin_summary_after_reload(
    state: &DesktopRuntimeState,
    plugin_id: &PluginId,
) -> Result<Option<PluginSummary>, CommandErrorPayload> {
    Ok(list_plugins_with_runtime_state(state)
        .await?
        .plugins
        .into_iter()
        .find(|plugin| &plugin.id == plugin_id))
}

async fn validate_plugin_source_path(
    source_path: &Path,
) -> Result<PluginInstallReport, CommandErrorPayload> {
    let loader = FileManifestLoader;
    let load_report = loader
        .load_package_report(source_path)
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("plugin manifest load failed: {error}"))
        })?;
    if let Some(failure) = load_report.failures.first() {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some(plugin_manifest_validation_failure_report_reason(
                &failure.failure,
            )),
        });
    }
    let Some(record) = load_report.records.first() else {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some("plugin manifest not found".to_owned()),
        });
    };
    if record.manifest.trust_level != TrustLevel::UserControlled {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some("local user plugin must declare user_controlled trust".to_owned()),
        });
    }
    if !matches!(record.origin, ManifestOrigin::CargoExtension { .. }) {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some(LOCAL_PLUGIN_SIDECAR_REQUIRED_REASON.to_owned()),
        });
    }

    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::Inline)
        .with_manifest_loader(Arc::new(InlineManifestLoader::new(vec![record.clone()])))
        .build()
        .map_err(|error| runtime_operation_failed(format!("plugin registry failed: {error}")))?;
    let discovered = registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    let plugin_id = record.manifest.plugin_id();
    let summary = registry
        .product_snapshot()
        .into_iter()
        .find(|summary| summary.id == plugin_id);
    let valid = discovered
        .iter()
        .any(|plugin| plugin.record.manifest.plugin_id() == plugin_id);
    let reason = if valid {
        None
    } else {
        registry
            .state_detail(&plugin_id)
            .and_then(|detail| detail.rejection_reason)
            .map(|reason| plugin_rejection_report_reason(&reason))
            .or_else(|| Some("plugin rejected".to_owned()))
    };
    let warnings = summary
        .as_ref()
        .map(|summary| summary.warnings.clone())
        .unwrap_or_default();
    Ok(PluginInstallReport {
        source_path: plugin_report_source_path(source_path),
        valid,
        summary,
        warnings,
        reason,
    })
}

fn plugin_report_source_path(_source_path: &Path) -> String {
    PLUGIN_REPORT_SOURCE_PATH_WITHHELD.to_owned()
}

fn plugin_manifest_validation_failure_report_reason(
    failure: &harness_contracts::ManifestValidationFailure,
) -> String {
    match failure {
        harness_contracts::ManifestValidationFailure::UnsupportedSchemaVersion { .. } => {
            "plugin manifest uses an unsupported schema version".to_owned()
        }
        harness_contracts::ManifestValidationFailure::RemoteIntegrityMismatch { .. } => {
            "plugin manifest integrity check failed".to_owned()
        }
        _ => "plugin manifest is invalid.".to_owned(),
    }
}

fn plugin_rejection_report_reason(reason: &RejectionReason) -> String {
    match reason {
        RejectionReason::SignatureInvalid { .. } => "plugin signature is invalid".to_owned(),
        RejectionReason::UnknownSigner { .. } => "plugin signer is unknown".to_owned(),
        RejectionReason::SignerRevoked { .. } => "plugin signer is revoked".to_owned(),
        RejectionReason::TrustMismatch { .. } => "plugin trust level is not allowed".to_owned(),
        RejectionReason::NamespaceConflict { .. } => "plugin namespace is not allowed".to_owned(),
        RejectionReason::DependencyUnsatisfied { .. } => {
            "plugin dependency is not satisfied".to_owned()
        }
        RejectionReason::DependencyCycle { .. } => "plugin dependency cycle detected".to_owned(),
        RejectionReason::HarnessVersionIncompatible { .. } => {
            "plugin requires an incompatible harness version".to_owned()
        }
        RejectionReason::SlotOccupied { .. } => "plugin capability slot is occupied".to_owned(),
        RejectionReason::AdmissionDenied { .. } => "plugin rejected by policy".to_owned(),
        _ => "plugin rejected by policy".to_owned(),
    }
}

fn ensure_plugin_source_path(value: &str) -> Result<PathBuf, CommandErrorPayload> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(invalid_payload(
            "plugin source path must be absolute".to_owned(),
        ));
    }
    ensure_no_symlink_components(&path, "plugin source directory")?;
    let path = path.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("plugin source path unavailable: {error}"))
    })?;
    ensure_no_symlink_components(&path, "plugin source directory")?;
    ensure_no_world_writable_ancestors(&path, "plugin source directory")?;
    ensure_not_world_writable_path(&path, "plugin source directory")?;
    if !path.is_dir() {
        return Err(invalid_payload(
            "plugin source path must point to a directory".to_owned(),
        ));
    }
    if !["plugin.json", "plugin.yaml", "plugin.yml"]
        .iter()
        .any(|name| path.join(name).is_file())
    {
        return Err(invalid_payload(
            "plugin package must contain plugin.json, plugin.yaml, or plugin.yml".to_owned(),
        ));
    }
    Ok(path)
}

#[cfg(unix)]
fn ensure_not_world_writable_path(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        runtime_operation_failed(format!("{label} metadata unavailable: {error}"))
    })?;
    if metadata.permissions().mode() & 0o002 != 0 {
        return Err(invalid_payload(format!(
            "{label} must not be world-writable"
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_not_world_writable_path(_path: &Path, _label: &str) -> Result<(), CommandErrorPayload> {
    Ok(())
}

#[cfg(unix)]
fn ensure_no_world_writable_ancestors(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    use std::os::unix::fs::PermissionsExt;

    for ancestor in path.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|error| {
            runtime_operation_failed(format!("{label} ancestor metadata unavailable: {error}"))
        })?;
        let mode = metadata.permissions().mode();
        let world_writable = mode & 0o002 != 0;
        let sticky = mode & 0o1000 != 0;
        if world_writable && !sticky {
            return Err(invalid_payload(format!(
                "{label} ancestors must not be world-writable"
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_no_world_writable_ancestors(
    _path: &Path,
    _label: &str,
) -> Result<(), CommandErrorPayload> {
    Ok(())
}

fn plugin_package_dir_name(plugin_id: &PluginId) -> String {
    plugin_id
        .0
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn redact_plugin_detail_config(mut detail: PluginDetail) -> PluginDetail {
    detail.config =
        redact_secret_config_values(detail.configuration_schema.as_ref(), detail.config);
    detail
}

fn redact_secret_config_values(schema: Option<&Value>, values: Value) -> Value {
    let Some(schema) = schema else {
        return values;
    };
    strip_secret_config_value(schema, &values).unwrap_or(Value::Null)
}

fn strip_secret_config_value(schema: &Value, value: &Value) -> Option<Value> {
    if schema
        .get("secret")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    match value {
        Value::Object(object) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            Some(Value::Object(
                object
                    .iter()
                    .filter_map(|(key, value)| {
                        let field_schema = properties.and_then(|properties| properties.get(key));
                        match field_schema {
                            Some(field_schema) => strip_secret_config_value(field_schema, value)
                                .map(|value| (key.clone(), value)),
                            None => Some((key.clone(), value.clone())),
                        }
                    })
                    .collect(),
            ))
        }
        Value::Array(values) => {
            let Some(item_schema) = schema.get("items") else {
                return Some(value.clone());
            };
            Some(Value::Array(
                values
                    .iter()
                    .filter_map(|value| strip_secret_config_value(item_schema, value))
                    .collect(),
            ))
        }
        value => Some(value.clone()),
    }
}

fn merge_plugin_config_values(schema: Option<&Value>, current: &Value, update: Value) -> Value {
    let update = redact_secret_config_values(schema, update);
    match update {
        Value::Object(update_object) => {
            let mut merged = current.as_object().cloned().unwrap_or_default();
            for (key, value) in update_object {
                merged.insert(key, value);
            }
            Value::Object(merged)
        }
        value => value,
    }
}

fn ensure_no_secret_like_config_values(value: &Value) -> Result<(), CommandErrorPayload> {
    fn visit(value: &Value) -> bool {
        match value {
            Value::Object(object) => object
                .iter()
                .any(|(key, value)| is_secret_like_key(key) || visit(value)),
            Value::Array(values) => values.iter().any(visit),
            Value::String(value) => is_secret_like_value(value),
            _ => false,
        }
    }
    if visit(value) {
        return Err(invalid_payload(
            "plugin config contains a secret-like field; secrets must be managed by the secure store"
                .to_owned(),
        ));
    }
    Ok(())
}

fn is_secret_like_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "secret",
        "token",
        "apikey",
        "credential",
        "password",
        "privatekey",
        "bearer",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn is_secret_like_value(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("sk-")
        || trimmed.starts_with("Bearer ")
        || trimmed.starts_with("ghp_")
        || trimmed.starts_with("gho_")
        || trimmed.starts_with("ghu_")
        || trimmed.starts_with("github_pat_")
}

fn ensure_import_skill_source_path(value: &str) -> Result<PathBuf, CommandErrorPayload> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(invalid_payload(
            "skill source path must be absolute".to_owned(),
        ));
    }
    ensure_no_symlink_components(&path, "skill source directory")?;
    let path = path.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("skill source path unavailable: {error}"))
    })?;
    ensure_no_symlink_components(&path, "skill source directory")?;
    if !path.is_dir() {
        return Err(invalid_payload(
            "skill source path must point to a directory".to_owned(),
        ));
    }
    let entry_path = path.join(SKILL_PACKAGE_ENTRY_FILE);
    ensure_no_symlink_components(&entry_path, "skill entry file")?;
    if !entry_path.is_file() {
        return Err(invalid_payload(
            "skill package must contain SKILL.md".to_owned(),
        ));
    }
    Ok(path)
}

fn skill_import_id() -> String {
    RunId::new().to_string().to_ascii_lowercase()
}

fn skill_summaries_from_records_and_runtime(
    records: &[SkillStoreRecord],
    runtime: &[RuntimeSkillSummary],
) -> Vec<SkillSummaryPayload> {
    let managed_names = records
        .iter()
        .map(|record| record.name.as_str())
        .collect::<HashSet<_>>();
    let mut skills = records
        .iter()
        .map(|record| {
            let status = runtime
                .iter()
                .find(|skill| skill.name == record.name)
                .map(|skill| skill_status_string(&skill.status));
            managed_skill_summary(record, status)
        })
        .collect::<Vec<_>>();
    skills.extend(
        runtime
            .iter()
            .filter(|skill| !managed_names.contains(skill.name.as_str()))
            .map(runtime_skill_summary_payload),
    );
    skills.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    skills
}

fn managed_skill_summary(
    record: &SkillStoreRecord,
    runtime_status: Option<&'static str>,
) -> SkillSummaryPayload {
    let status = if record.last_validation_error.is_some() {
        "rejected"
    } else if !record.enabled {
        "disabled"
    } else {
        runtime_status.unwrap_or("ready")
    };
    SkillSummaryPayload {
        id: record.id.clone(),
        name: record.name.clone(),
        description: record.description.clone(),
        source_kind: "workspace".to_owned(),
        enabled: record.enabled,
        manageable: true,
        status: status.to_owned(),
        tags: record.tags.clone(),
        category: record.category.clone(),
        imported_at: Some(record.imported_at.clone()),
        updated_at: Some(record.updated_at.clone()),
        origin: record.origin.clone(),
        source_plugin_id: None,
    }
}

fn runtime_skill_summary_payload(skill: &RuntimeSkillSummary) -> SkillSummaryPayload {
    SkillSummaryPayload {
        id: skill.name.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        source_kind: skill_source_string(&skill.source).to_owned(),
        enabled: true,
        manageable: false,
        status: skill_status_string(&skill.status).to_owned(),
        tags: skill.tags.clone(),
        category: skill.category.clone(),
        imported_at: None,
        updated_at: None,
        origin: None,
        source_plugin_id: skill_source_plugin_id(&skill.source),
    }
}

fn skill_detail_from_runtime_view(
    summary: SkillSummaryPayload,
    view: RuntimeSkillView,
    files: Vec<SkillFilePayload>,
    validation_error: Option<String>,
) -> SkillDetailPayload {
    SkillDetailPayload {
        summary,
        parameters: view
            .parameters
            .into_iter()
            .map(|parameter| SkillParameterPayload {
                name: parameter.name,
                param_type: parameter.param_type,
                required: parameter.required,
                default: parameter.default,
                description: parameter.description,
            })
            .collect(),
        config_keys: view.config_keys,
        files,
        body_preview: view.body_preview,
        validation_error,
    }
}

fn runtime_status_for_name(harness: &Harness, name: &str) -> Option<&'static str> {
    harness
        .list_runtime_skills()
        .iter()
        .find(|skill| skill.name == name)
        .map(|skill| skill_status_string(&skill.status))
}

fn skill_status_string(status: &jyowo_harness_sdk::ext::SkillStatus) -> &'static str {
    match status {
        jyowo_harness_sdk::ext::SkillStatus::Ready => "ready",
        jyowo_harness_sdk::ext::SkillStatus::PrerequisiteMissing { .. } => "prerequisite_missing",
    }
}

fn skill_source_string(source: &jyowo_harness_sdk::ext::SkillSourceKind) -> &'static str {
    match source {
        jyowo_harness_sdk::ext::SkillSourceKind::Bundled => "bundled",
        jyowo_harness_sdk::ext::SkillSourceKind::Workspace => "workspace",
        jyowo_harness_sdk::ext::SkillSourceKind::User => "user",
        jyowo_harness_sdk::ext::SkillSourceKind::Plugin(_) => "plugin",
        jyowo_harness_sdk::ext::SkillSourceKind::Mcp(_) => "mcp",
        _ => "workspace",
    }
}

fn skill_source_plugin_id(source: &jyowo_harness_sdk::ext::SkillSourceKind) -> Option<String> {
    match source {
        jyowo_harness_sdk::ext::SkillSourceKind::Plugin(plugin_id) => Some(plugin_id.0.clone()),
        _ => None,
    }
}

pub async fn list_memory_items_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListMemoryItemsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let mut items = harness
        .list_memory_items(options)
        .await
        .map_err(|_| memory_operation_failed("Memory items could not be loaded."))?
        .into_iter()
        .map(memory_item_summary_payload)
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then(left.id.cmp(&right.id))
    });

    Ok(ListMemoryItemsResponse { items })
}

pub async fn get_memory_item_with_runtime_state(
    request: GetMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Inspecting memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let item = harness
        .get_memory_item(options, id)
        .await
        .map_err(|_| memory_operation_failed("Memory detail could not be loaded."))?;

    Ok(GetMemoryItemResponse {
        item: memory_item_payload(item),
    })
}

pub async fn update_memory_item_with_runtime_state(
    request: UpdateMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    ensure_non_empty("content", &request.content)?;
    ensure_max_bytes("content", &request.content, MAX_MEMORY_CONTENT_BYTES)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Editing memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let item = harness
        .update_memory_item_content(options, id, request.content)
        .await
        .map_err(|_| memory_operation_failed("Memory item could not be saved."))?;

    Ok(UpdateMemoryItemResponse {
        item: memory_item_payload(item),
    })
}

pub async fn delete_memory_item_with_runtime_state(
    request: DeleteMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Deleting memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    harness
        .delete_memory_item(options, id)
        .await
        .map_err(|_| memory_operation_failed("Memory item could not be deleted."))?;

    Ok(DeleteMemoryItemResponse {
        id: request.id,
        status: "deleted",
    })
}

pub async fn export_memory_items_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Exporting memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let records = harness
        .export_memory_items(options)
        .await
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    let item_count = records.len().min(u32::MAX as usize) as u32;
    let items = records
        .into_iter()
        .map(memory_item_payload)
        .collect::<Vec<_>>();
    let content = serde_json::to_string_pretty(&items)
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    let exported_at = jyowo_harness_sdk::ext::now();
    let file_name = format!("memory-{}.json", exported_at.format("%Y%m%dT%H%M%S%.3fZ"));
    let relative_path = PathBuf::from(".jyowo")
        .join("runtime")
        .join("exports")
        .join(file_name);
    let export_path = state.workspace_root.join(&relative_path);
    write_memory_export_file(&export_path, &content)?;

    Ok(ExportMemoryItemsResponse {
        exported_at: exported_at.to_rfc3339(),
        format: "json",
        item_count,
        path: relative_path.to_string_lossy().into_owned(),
    })
}

fn parse_memory_id(value: &str) -> Result<MemoryId, CommandErrorPayload> {
    ensure_non_empty("id", value)?;
    let value = value.trim();
    let id = MemoryId::parse(value)
        .map_err(|_| invalid_payload("id must be a valid memory id".to_owned()))?;

    if id.to_string() != value {
        return Err(invalid_payload(
            "id must be a canonical memory id".to_owned(),
        ));
    }

    Ok(id)
}

fn memory_item_summary_payload(summary: MemorySummary) -> MemoryItemSummaryPayload {
    MemoryItemSummaryPayload {
        content_preview: summary.content_preview,
        id: summary.id.to_string(),
        kind: memory_kind_payload(&summary.kind).to_owned(),
        source: memory_source_payload(&summary.metadata.source).to_owned(),
        tags: summary.metadata.tags,
        updated_at: summary.updated_at.to_rfc3339(),
        visibility: memory_visibility_payload(&summary.visibility).to_owned(),
    }
}

fn memory_item_payload(record: MemoryRecord) -> MemoryItemPayload {
    MemoryItemPayload {
        access_count: record.metadata.access_count,
        confidence: record.metadata.confidence,
        content: record.content,
        created_at: record.created_at.to_rfc3339(),
        id: record.id.to_string(),
        kind: memory_kind_payload(&record.kind).to_owned(),
        source: memory_source_payload(&record.metadata.source).to_owned(),
        tags: record.metadata.tags,
        updated_at: record.updated_at.to_rfc3339(),
        visibility: memory_visibility_payload(&record.visibility).to_owned(),
    }
}

fn memory_kind_payload(kind: &MemoryKind) -> &'static str {
    match kind {
        MemoryKind::UserPreference => "user_preference",
        MemoryKind::Feedback => "feedback",
        MemoryKind::ProjectFact => "project_fact",
        MemoryKind::Reference => "reference",
        MemoryKind::AgentSelfNote => "agent_self_note",
        MemoryKind::Custom(_) => "custom",
        _ => "custom",
    }
}

fn memory_visibility_payload(visibility: &MemoryVisibility) -> &'static str {
    match visibility {
        MemoryVisibility::Private { .. } => "private",
        MemoryVisibility::User { .. } => "user",
        MemoryVisibility::Team { .. } => "team",
        MemoryVisibility::Tenant => "tenant",
        _ => "tenant",
    }
}

fn memory_source_payload(source: &MemorySource) -> &'static str {
    match source {
        MemorySource::UserInput => "user_input",
        MemorySource::AgentDerived => "agent_derived",
        MemorySource::SubagentDerived { .. } => "subagent_derived",
        MemorySource::ExternalRetrieval => "external_retrieval",
        MemorySource::Imported => "imported",
        MemorySource::Consolidated { .. } => "consolidated",
        _ => "imported",
    }
}

async fn mcp_config_from_records(
    records: Vec<McpServerConfigRecord>,
    default_session_id: SessionId,
    default_agent_id: AgentId,
    diagnostic_store: Arc<dyn McpDiagnosticStore>,
    workspace_root: &Path,
) -> Result<McpConfig, CommandErrorPayload> {
    let registry = McpRegistry::new();
    let mut server_ids_to_inject = Vec::new();

    for record in records {
        ensure_mcp_server_record(&record)?;
        if !record.enabled {
            continue;
        }
        let server_id = register_mcp_record_with_registry(
            &record,
            &registry,
            default_session_id,
            default_agent_id,
            Arc::clone(&diagnostic_store),
            workspace_root,
        )
        .await?;
        if matches!(
            registry.connection_state(&server_id).await,
            Some(McpConnectionState::Ready)
        ) {
            server_ids_to_inject.push(server_id);
        }
    }

    Ok(McpConfig {
        registry,
        server_ids_to_inject,
    })
}

async fn register_mcp_record_with_harness(
    record: &McpServerConfigRecord,
    harness: &Harness,
    default_session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<McpServerSummaryPayload, CommandErrorPayload> {
    let Some(config) = harness.mcp_config() else {
        return Ok(mcp_server_summary_from_record(record));
    };
    let server_id = register_mcp_record_with_registry(
        record,
        &config.registry,
        default_session_id,
        AgentId::new(),
        Arc::clone(&state.mcp_diagnostic_store),
        state.workspace_root(),
    )
    .await?;

    if matches!(
        config.registry.connection_state(&server_id).await,
        Some(McpConnectionState::Ready)
    ) {
        if let Err(error) = config
            .registry
            .inject_tools_into(harness.tool_registry(), &server_id)
            .await
        {
            config
                .registry
                .set_connection_state(
                    &server_id,
                    McpConnectionState::Failed {
                        last_error: error.to_string(),
                    },
                )
                .await
                .map_err(|error| runtime_operation_failed(error.to_string()))?;
        }
    }

    mcp_server_summary_from_registry(&config.registry, &server_id)
        .await
        .ok_or_else(|| {
            runtime_operation_failed("mcp server registry summary unavailable".to_owned())
        })
}

async fn register_mcp_record_with_registry(
    record: &McpServerConfigRecord,
    registry: &McpRegistry,
    default_session_id: SessionId,
    default_agent_id: AgentId,
    diagnostic_store: Arc<dyn McpDiagnosticStore>,
    workspace_root: &Path,
) -> Result<McpServerId, CommandErrorPayload> {
    let spec = mcp_server_spec_from_record(record, workspace_root)?;
    let server_id = spec.server_id.clone();
    let scope = mcp_server_scope_from_record(record, default_session_id, default_agent_id)?;
    let transport = mcp_transport_for_config(&record.transport);
    let event_sink = Arc::new(DesktopMcpEventSink { diagnostic_store });
    match registry
        .add_managed_server(spec.clone(), scope.clone(), transport, event_sink)
        .await
    {
        Ok(()) => {}
        Err(error) => {
            registry
                .add_failed_server(spec, scope, error.to_string())
                .await
                .map_err(|error| runtime_operation_failed(error.to_string()))?;
        }
    }

    Ok(server_id)
}

async fn remove_mcp_server_from_harness(
    harness: &Harness,
    id: &str,
) -> Result<(), CommandErrorPayload> {
    let Some(config) = harness.mcp_config() else {
        return Ok(());
    };
    let server_id = McpServerId(id.to_owned());
    if let Some(tool_names) = config.registry.injected_tool_names(&server_id).await {
        for tool_name in tool_names {
            if harness.tool_registry().get(&tool_name).is_some() {
                harness
                    .tool_registry()
                    .deregister(&tool_name)
                    .map_err(|error| runtime_operation_failed(error.to_string()))?;
            }
        }
    }
    match config.registry.remove_server(&server_id).await {
        Ok(()) | Err(jyowo_harness_sdk::ext::McpError::ServerNotFound(_)) => Ok(()),
        Err(error) => Err(runtime_operation_failed(error.to_string())),
    }
}

fn mcp_server_spec_from_record(
    record: &McpServerConfigRecord,
    workspace_root: &Path,
) -> Result<McpServerSpec, CommandErrorPayload> {
    match &record.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => {
            let mut policy = StdioPolicy::default();
            policy.working_dir = Some(mcp_stdio_working_dir(
                working_dir.as_deref(),
                workspace_root,
            )?);
            Ok(McpServerSpec::new(
                McpServerId(record.id.clone()),
                record.display_name.clone(),
                TransportChoice::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env: mcp_stdio_env(env, inherit_env),
                    policy,
                },
                McpServerSource::Workspace,
            ))
        }
        McpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => Ok(McpServerSpec::new(
            McpServerId(record.id.clone()),
            record.display_name.clone(),
            TransportChoice::Http {
                url: url.clone(),
                headers: mcp_http_headers(
                    headers,
                    headers_from_env,
                    bearer_token_env_var.as_deref(),
                )?,
            },
            McpServerSource::Workspace,
        )),
        McpServerTransportConfig::InProcess => Err(invalid_payload(
            "transport.kind must be stdio or http for workspace MCP servers".to_owned(),
        )),
    }
}

fn mcp_transport_for_config(
    transport: &McpServerTransportConfig,
) -> Arc<dyn jyowo_harness_sdk::ext::McpTransport> {
    match transport {
        McpServerTransportConfig::Http { .. } => Arc::new(HttpTransport::new()),
        McpServerTransportConfig::Stdio { .. } | McpServerTransportConfig::InProcess => {
            Arc::new(StdioTransport::new())
        }
    }
}

fn mcp_stdio_env(env: &[McpNameValueRecord], inherit_env: &[String]) -> StdioEnv {
    let extra = env
        .iter()
        .map(|record| (record.key.clone(), record.value.clone()))
        .collect::<BTreeMap<_, _>>();
    if inherit_env.is_empty() {
        StdioEnv::InheritWithDeny {
            deny: StdioEnv::default_deny_envs(),
            extra,
        }
    } else {
        StdioEnv::Allowlist {
            inherit: inherit_env.iter().cloned().collect::<BTreeSet<_>>(),
            extra,
        }
    }
}

fn mcp_stdio_working_dir(
    working_dir: Option<&str>,
    workspace_root: &Path,
) -> Result<PathBuf, CommandErrorPayload> {
    let Some(working_dir) = working_dir else {
        return Ok(workspace_root.to_path_buf());
    };
    ensure_non_empty("transport.workingDir", working_dir)?;
    let candidate = PathBuf::from(working_dir);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|error| invalid_payload(format!("transport.workingDir is invalid: {error}")))?;
    if !canonical.starts_with(workspace_root) {
        return Err(invalid_payload(
            "transport.workingDir must stay inside the workspace".to_owned(),
        ));
    }
    Ok(canonical)
}

fn mcp_http_headers(
    headers: &[McpNameValueRecord],
    headers_from_env: &[McpHeaderEnvRecord],
    bearer_token_env_var: Option<&str>,
) -> Result<BTreeMap<String, String>, CommandErrorPayload> {
    let mut resolved = BTreeMap::new();
    for header in headers {
        resolved.insert(header.key.trim().to_owned(), header.value.clone());
    }
    for header in headers_from_env {
        let value = std::env::var(&header.env_var).map_err(|_| {
            runtime_operation_failed(format!(
                "MCP header env var is unavailable: {}",
                header.env_var
            ))
        })?;
        resolved.insert(header.key.trim().to_owned(), value);
    }
    if let Some(env_var) = bearer_token_env_var {
        let token = std::env::var(env_var).map_err(|_| {
            runtime_operation_failed(format!(
                "MCP bearer token env var is unavailable: {env_var}"
            ))
        })?;
        resolved.insert("Authorization".to_owned(), format!("Bearer {token}"));
    }
    Ok(resolved)
}

fn mcp_server_scope_from_record(
    record: &McpServerConfigRecord,
    default_session_id: SessionId,
    default_agent_id: AgentId,
) -> Result<McpServerScope, CommandErrorPayload> {
    match record.scope.as_str() {
        "global" => Ok(McpServerScope::Global),
        "session" => Ok(McpServerScope::Session(default_session_id)),
        "agent" => Ok(McpServerScope::Agent(default_agent_id)),
        _ => Err(invalid_payload(
            "scope must be global, session, or agent".to_owned(),
        )),
    }
}

struct DesktopMcpEventSink {
    diagnostic_store: Arc<dyn McpDiagnosticStore>,
}

impl McpEventSink for DesktopMcpEventSink {
    fn emit(&self, event: Event) {
        if let Some(record) = mcp_diagnostic_record_from_event(event) {
            let _ = self.diagnostic_store.append_record(&record);
        }
    }
}

pub fn mcp_diagnostic_record_from_event(event: Event) -> Option<McpDiagnosticRecord> {
    let (server_id, event_type, severity, summary, timestamp) = match event {
        Event::McpToolInjected(event) => (
            event.server_id.0,
            "tool_injected",
            McpDiagnosticSeverity::Info,
            "MCP tool exposed.",
            event.at.to_rfc3339(),
        ),
        Event::McpConnectionLost(event) => (
            event.server_id.0,
            "connection_lost",
            if event.terminal {
                McpDiagnosticSeverity::Error
            } else {
                McpDiagnosticSeverity::Warning
            },
            if event.terminal {
                "MCP server connection lost."
            } else {
                "MCP server connection lost; reconnecting."
            },
            event.at.to_rfc3339(),
        ),
        Event::McpConnectionRecovered(event) => (
            event.server_id.0,
            "connection_recovered",
            McpDiagnosticSeverity::Info,
            "MCP server connection recovered.",
            event.at.to_rfc3339(),
        ),
        Event::McpOAuthRefresh(event) => (
            event.server_id.0,
            "oauth_refresh",
            match event.outcome {
                harness_contracts::McpOAuthRefreshOutcome::Error => McpDiagnosticSeverity::Error,
                _ => McpDiagnosticSeverity::Info,
            },
            match event.outcome {
                harness_contracts::McpOAuthRefreshOutcome::Started => "MCP OAuth refresh started.",
                harness_contracts::McpOAuthRefreshOutcome::Success => {
                    "MCP OAuth refresh completed."
                }
                harness_contracts::McpOAuthRefreshOutcome::Error => "MCP OAuth refresh failed.",
            },
            event.at.to_rfc3339(),
        ),
        Event::McpElicitationRequested(event) => (
            event.server_id.0,
            "elicitation_requested",
            McpDiagnosticSeverity::Info,
            "MCP elicitation requested.",
            event.at.to_rfc3339(),
        ),
        Event::McpElicitationResolved(event) => (
            event.server_id.0,
            "elicitation_resolved",
            match event.outcome {
                harness_contracts::ElicitationOutcome::Provided { .. } => {
                    McpDiagnosticSeverity::Info
                }
                _ => McpDiagnosticSeverity::Warning,
            },
            "MCP elicitation resolved.",
            event.at.to_rfc3339(),
        ),
        Event::McpToolsListChanged(event) => (
            event.server_id.0,
            "tools_changed",
            McpDiagnosticSeverity::Info,
            "MCP tools changed.",
            event.received_at.to_rfc3339(),
        ),
        Event::McpResourceUpdated(event) => (
            event.server_id.0,
            "resource_updated",
            McpDiagnosticSeverity::Info,
            match event.kind {
                harness_contracts::McpResourceUpdateKind::PromptsListChanged { .. } => {
                    "MCP prompts changed."
                }
                harness_contracts::McpResourceUpdateKind::ListChanged { .. } => {
                    "MCP resources changed."
                }
                harness_contracts::McpResourceUpdateKind::ResourceUpdated { .. } => {
                    "MCP resource updated."
                }
                _ => "MCP resource updated.",
            },
            event.at.to_rfc3339(),
        ),
        Event::McpSamplingRequested(event) => (
            event.server_id.0,
            "sampling",
            match event.outcome {
                harness_contracts::SamplingOutcome::Completed => McpDiagnosticSeverity::Info,
                harness_contracts::SamplingOutcome::UpstreamError { .. } => {
                    McpDiagnosticSeverity::Error
                }
                _ => McpDiagnosticSeverity::Warning,
            },
            "MCP sampling request handled.",
            event.at.to_rfc3339(),
        ),
        _ => return None,
    };

    Some(McpDiagnosticRecord {
        event_type: event_type.to_owned(),
        id: format!("mcp-diagnostic-{}", EventId::new()),
        server_id,
        severity,
        summary: summary.to_owned(),
        timestamp,
    })
}

async fn mcp_server_summary_from_registry(
    registry: &jyowo_harness_sdk::ext::McpRegistry,
    server_id: &McpServerId,
) -> Option<McpServerSummaryPayload> {
    let spec = registry.server_spec(server_id).await?;
    let scope = registry.server_scope(server_id).await?;
    let connection_state = registry.connection_state(server_id).await?;
    let exposed_tool_count = registry.injected_tool_count(server_id).await.unwrap_or(0);
    let (status, last_error) = mcp_connection_state_payload(&connection_state);

    Some(McpServerSummaryPayload {
        display_name: spec.display_name,
        enabled: true,
        exposed_tool_count: exposed_tool_count.try_into().unwrap_or(u32::MAX),
        id: server_id.0.clone(),
        last_diagnostic: None,
        last_diagnostic_at: None,
        last_diagnostic_severity: None,
        last_error,
        manageable: false,
        origin: mcp_server_origin_payload(&spec.source),
        scope: mcp_server_scope_payload(&scope),
        source_plugin_id: mcp_source_plugin_id(&spec.source),
        status,
        transport: mcp_transport_payload(&spec.transport),
    })
}

fn mcp_server_summary_from_record(record: &McpServerConfigRecord) -> McpServerSummaryPayload {
    McpServerSummaryPayload {
        display_name: record.display_name.clone(),
        enabled: record.enabled,
        exposed_tool_count: 0,
        id: record.id.clone(),
        last_diagnostic: None,
        last_diagnostic_at: None,
        last_diagnostic_severity: None,
        last_error: None,
        manageable: true,
        origin: "workspace",
        scope: record.scope.clone(),
        status: if record.enabled {
            "configured"
        } else {
            "disabled"
        },
        source_plugin_id: None,
        transport: mcp_transport_config_payload(&record.transport),
    }
}

fn mcp_last_diagnostics_by_server(
    records: &[McpDiagnosticRecord],
) -> BTreeMap<String, McpDiagnosticRecord> {
    let mut last = BTreeMap::new();
    for record in records {
        last.insert(record.server_id.clone(), record.clone());
    }
    last
}

fn apply_mcp_last_diagnostic(
    summary: &mut McpServerSummaryPayload,
    diagnostic: Option<&McpDiagnosticRecord>,
) {
    if let Some(diagnostic) = diagnostic {
        summary.last_diagnostic = Some(diagnostic.summary.clone());
        summary.last_diagnostic_at = Some(diagnostic.timestamp.clone());
        summary.last_diagnostic_severity = Some(diagnostic.severity);
    }
}

pub async fn list_conversations_with_runtime_state(
    state: &DesktopRuntimeState,
) -> ListConversationsResponse {
    let Some(harness) = state.harness() else {
        return ListConversationsResponse {
            conversations: Vec::new(),
        };
    };

    let summaries = list_runtime_conversation_summaries(&harness, state).await;
    let conversations = summaries
        .into_iter()
        .map(conversation_summary_payload_from_read_model)
        .collect();

    ListConversationsResponse { conversations }
}

pub async fn create_conversation_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<CreateConversationResponse, CommandErrorPayload> {
    let session_id = SessionId::new();
    let Some((harness, options)) = state.active_conversation_runtime(session_id) else {
        return Err(runtime_unavailable(
            "Creating conversations requires the runtime conversation facade.",
        ));
    };
    harness
        .open_or_create_conversation_session(options)
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("conversation create failed: {error}"))
        })?;

    let summary = harness
        .list_conversation_summaries(TenantId::SINGLE, 50)
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("conversation list failed after create: {error}"))
        })?
        .into_iter()
        .find(|summary| summary.id == session_id.to_string())
        .ok_or_else(|| not_found(format!("conversation not found: {session_id}")))?;

    Ok(CreateConversationResponse {
        conversation: conversation_summary_payload_from_read_model(summary),
    })
}

async fn list_runtime_conversation_summaries(
    harness: &Harness,
    state: &DesktopRuntimeState,
) -> Vec<harness_contracts::ConversationSummary> {
    let mut summaries = harness
        .list_conversation_summaries(TenantId::SINGLE, 50)
        .await
        .unwrap_or_default();

    let default_conversation_id = state.default_conversation_id();
    let default_conversation_deleted = state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&default_conversation_id);

    if summaries.is_empty()
        && !default_conversation_deleted
        && harness
            .open_or_create_conversation_session(
                state.conversation_session_options(default_conversation_id),
            )
            .await
            .is_ok()
    {
        summaries = harness
            .list_conversation_summaries(TenantId::SINGLE, 50)
            .await
            .unwrap_or_default();
    }

    let deleted = state.deleted_conversation_ids.lock().await;
    summaries.retain(|summary| {
        SessionId::parse(&summary.id).is_ok_and(|session_id| !deleted.contains(&session_id))
    });

    summaries
}

fn conversation_summary_payload_from_read_model(
    summary: harness_contracts::ConversationSummary,
) -> ConversationSummaryPayload {
    ConversationSummaryPayload {
        id: summary.id,
        is_empty: summary.is_empty,
        last_message_preview: summary
            .last_message_preview
            .map(|preview| preview.into_string()),
        title: summary.title.into_string(),
        updated_at: summary.updated_at.to_rfc3339(),
    }
}

pub async fn get_conversation_with_runtime_state(
    request: GetConversationRequest,
    state: &DesktopRuntimeState,
) -> Result<GetConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversations requires the runtime conversation facade.",
        ));
    };
    let snapshot = harness
        .get_conversation_snapshot(&request.conversation_id, 200)
        .await
        .map_err(conversation_read_error)?
        .ok_or_else(|| {
            not_found(format!(
                "conversation not found: {}",
                request.conversation_id
            ))
        })?;

    Ok(GetConversationResponse {
        conversation: ConversationPayload {
            id: request.conversation_id,
            messages: snapshot
                .messages
                .into_iter()
                .map(conversation_message_payload_from_read_model)
                .collect(),
            model_config_id: conversation_model_config_id(&session_id, state)?
                .or(snapshot.model_config_id),
            title: snapshot.title.into_string(),
            updated_at: snapshot.updated_at.to_rfc3339(),
        },
    })
}

fn conversation_message_payload_from_read_model(
    message: harness_contracts::ConversationMessage,
) -> ConversationMessagePayload {
    ConversationMessagePayload {
        author: match message.author {
            ConversationMessageAuthor::User => "user",
            ConversationMessageAuthor::Assistant => "assistant",
        },
        body: message.body.into_string(),
        client_message_id: message.client_message_id,
        id: message.id,
        timestamp: message.timestamp.to_rfc3339(),
    }
}

fn conversation_model_config_id(
    session_id: &SessionId,
    state: &DesktopRuntimeState,
) -> Result<Option<String>, CommandErrorPayload> {
    Ok(state
        .conversation_model_config_store
        .load_records()?
        .get(&session_id.to_string())
        .cloned())
}

async fn persist_conversation_model_config_id(
    session_id: SessionId,
    model_config_id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let _guard = state.conversation_model_config_lock.lock().await;
    let mut records = state.conversation_model_config_store.load_records()?;
    records.insert(session_id.to_string(), model_config_id.to_owned());
    state.conversation_model_config_store.save_records(&records)
}

pub async fn set_conversation_model_config_with_runtime_state(
    request: SetConversationModelConfigRequest,
    state: &DesktopRuntimeState,
) -> Result<SetConversationModelConfigResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("modelConfigId", &request.model_config_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    ensure_existing_conversation_session(session_id, state).await?;
    let provider_record = state
        .provider_settings_store
        .load_record()?
        .unwrap_or_default();
    let Some(config) = provider_record
        .configs
        .iter()
        .find(|config| config.id == request.model_config_id)
    else {
        return Err(not_found(format!(
            "provider config not found: {}",
            request.model_config_id
        )));
    };
    if ensure_provider_config_has_api_key(config).is_err() {
        return Err(invalid_payload(
            "apiKey is required before selecting a provider config".to_owned(),
        ));
    }
    persist_conversation_model_config_id(session_id, &request.model_config_id, state).await?;

    Ok(SetConversationModelConfigResponse {
        conversation_id: request.conversation_id,
        model_config_id: request.model_config_id,
        status: "saved",
    })
}

pub fn delete_conversation_payload(
    request: DeleteConversationRequest,
) -> Result<DeleteConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;

    Err(runtime_unavailable(
        "Deleting conversations requires the runtime conversation facade.",
    ))
}

pub async fn delete_conversation_with_runtime_state(
    request: DeleteConversationRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Deleting conversations requires the runtime conversation facade.",
        ));
    };

    let deleted = harness
        .delete_conversation_session(state.conversation_session_options(session_id))
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("conversation delete failed: {error}"))
        })?;
    if !deleted {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }

    state
        .deleted_conversation_ids
        .lock()
        .await
        .insert(session_id);

    Ok(DeleteConversationResponse {
        conversation_id: request.conversation_id,
        status: "deleted",
    })
}

pub fn start_run_payload(
    request: StartRunRequest,
) -> Result<StartRunResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;
    ensure_non_empty("prompt", &request.prompt)?;
    if let Some(client_message_id) = request.client_message_id.as_deref() {
        validate_client_message_id(client_message_id)?;
    }
    if let Some(permission_mode) = request.permission_mode {
        ensure_start_run_permission_mode(permission_mode)?;
    }
    validate_context_reference_payloads(request.context_references.as_deref())?;
    validate_attachment_reference_payloads(request.attachments.as_deref())?;

    Err(runtime_unavailable(
        "Starting runs requires the runtime conversation facade.",
    ))
}

pub async fn start_run_with_runtime_state(
    request: StartRunRequest,
    state: &DesktopRuntimeState,
) -> Result<StartRunResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    ensure_non_empty("prompt", &request.prompt)?;
    if let Some(client_message_id) = request.client_message_id.as_deref() {
        validate_client_message_id(client_message_id)?;
    }
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }

    let permission_mode = resolve_start_run_permission_mode(
        request.permission_mode,
        &state.execution_settings_store,
    )?;
    let input = build_conversation_turn_input(&request, state).await?;
    let _start_run_guard = state.start_run_lock.lock().await;
    let (harness, options) =
        if let Some(model_config_id) = conversation_model_config_id(&session_id, state)? {
            let stream_permission_runtime =
                state.stream_permission_runtime.as_ref().ok_or_else(|| {
                    runtime_unavailable("Starting runs requires the desktop runtime.")
                })?;
            let (harness, model_id, protocol) = build_desktop_harness(
                &state.workspace_root,
                Arc::clone(stream_permission_runtime),
                Some(&model_config_id),
            )
            .await?;
            (
                Arc::new(harness),
                state.conversation_session_options_for_model(session_id, model_id, protocol),
            )
        } else {
            let Some(runtime) = state.active_conversation_runtime(session_id) else {
                return Err(runtime_unavailable(
                    "Starting runs requires the runtime conversation facade.",
                ));
            };
            runtime
        };
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .map_err(|error| runtime_operation_failed(format!("conversation open failed: {error}")))?;
    let after_event_id = conversation_tail_event_id(&harness, options.clone()).await?;
    let run_harness = Arc::clone(&harness);
    let run_options = options.clone();
    let mut run_task = tokio::spawn(async move {
        run_harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: run_options,
                input,
                permission_mode_override: Some(permission_mode),
            })
            .await
    });
    let run_id =
        match wait_for_started_conversation_run(&harness, options, after_event_id, &mut run_task)
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => {
                run_task.abort();
                return Err(error);
            }
        };
    drop(run_task);

    Ok(StartRunResponse {
        run_id: run_id.to_string(),
        status: "started",
    })
}

pub fn create_attachment_from_path_payload(
    request: CreateAttachmentFromPathRequest,
) -> Result<CreateAttachmentFromPathResponse, CommandErrorPayload> {
    ensure_non_empty("path", &request.path)?;

    Err(runtime_unavailable(
        "Creating attachments requires the runtime workspace state.",
    ))
}

pub fn list_reference_candidates_payload(
) -> Result<ListReferenceCandidatesResponse, CommandErrorPayload> {
    Err(runtime_unavailable(
        "Listing reference candidates requires the runtime workspace state.",
    ))
}

pub async fn create_attachment_from_path_with_runtime_state(
    request: CreateAttachmentFromPathRequest,
    state: &DesktopRuntimeState,
) -> Result<CreateAttachmentFromPathResponse, CommandErrorPayload> {
    ensure_non_empty("path", &request.path)?;
    let requested_path = Path::new(&request.path);
    let candidate_path = if requested_path.is_absolute() {
        if requested_path.strip_prefix(state.workspace_root()).is_err() {
            let Some(parent) = requested_path.parent() else {
                return Err(invalid_payload(
                    "attachment path must stay inside the workspace".to_owned(),
                ));
            };
            let Ok(parent) = parent.canonicalize() else {
                return Err(invalid_payload(
                    "attachment path must stay inside the workspace".to_owned(),
                ));
            };
            if workspace_relative_path(&parent, state.workspace_root()).is_none() {
                return Err(invalid_payload(
                    "attachment path must stay inside the workspace".to_owned(),
                ));
            }
            let Some(file_name) = requested_path.file_name() else {
                return Err(invalid_payload("path must point to a file".to_owned()));
            };
            parent.join(file_name)
        } else {
            requested_path.to_path_buf()
        }
    } else {
        state.workspace_root().join(requested_path)
    };
    let source_path = canonicalize_existing_file(&candidate_path, "path")?;
    if workspace_relative_path(&source_path, state.workspace_root()).is_none() {
        return Err(invalid_payload(
            "attachment path must stay inside the workspace".to_owned(),
        ));
    }
    let metadata = source_path.metadata().map_err(|error| {
        runtime_operation_failed(format!("attachment metadata failed: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(invalid_payload("path must point to a file".to_owned()));
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(format!(
            "attachment must be at most {} MB",
            MAX_ATTACHMENT_BYTES / 1024 / 1024
        )));
    }

    let name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("attachment")
        .to_owned();
    let id = attachment_id(&source_path, metadata.len());
    let mime_type = infer_mime_type(&source_path);
    let bytes = std::fs::read(&source_path)
        .map_err(|error| runtime_operation_failed(format!("attachment read failed: {error}")))?;
    let hash = blake3::hash(&bytes);
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .map_err(|error| runtime_operation_failed(format!("attachment store unavailable: {error}")))?;
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            Bytes::from(bytes),
            BlobMeta {
                content_type: Some(mime_type.clone()),
                size: metadata.len(),
                content_hash: *hash.as_bytes(),
                created_at: Utc::now(),
                retention: BlobRetention::TenantScoped,
            },
        )
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("attachment blob write failed: {error}"))
        })?;
    let attachment = AttachmentReferencePayload {
        id: id.clone(),
        name,
        mime_type,
        size_bytes: metadata.len(),
        blob_ref: attachment_blob_ref_payload(&blob_ref),
    };

    write_attachment_record(
        state.workspace_root(),
        &AttachmentRecord {
            attachment: attachment.clone(),
            blob_ref,
        },
    )?;

    Ok(CreateAttachmentFromPathResponse { attachment })
}

pub async fn list_reference_candidates_with_runtime_state(
    request: ListReferenceCandidatesRequest,
    state: &DesktopRuntimeState,
) -> Result<ListReferenceCandidatesResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    ensure_reference_conversation_exists(session_id, state).await?;
    let files = context_files_from_workspace(state.workspace_root())
        .into_iter()
        .map(|file| ReferenceCandidatePayload {
            id: None,
            label: file.label.clone(),
            path: Some(file.label),
        })
        .collect();
    let artifacts = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: request.conversation_id.clone(),
        },
        state,
    )
    .await?
    .artifacts
    .into_iter()
    .map(|artifact| ReferenceCandidatePayload {
        id: Some(artifact.id),
        label: artifact.title,
        path: None,
    })
    .collect();
    let conversations = list_conversations_with_runtime_state(state)
        .await
        .conversations
        .into_iter()
        .map(|conversation| ReferenceCandidatePayload {
            id: Some(conversation.id),
            label: conversation.title,
            path: None,
        })
        .collect();
    let memories = match list_memory_items_with_runtime_state(state).await {
        Ok(payload) => payload
            .items
            .into_iter()
            .map(|item| ReferenceCandidatePayload {
                id: Some(item.id),
                label: item.content_preview,
                path: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let skills = match list_skills_with_runtime_state(state).await {
        Ok(payload) => payload
            .skills
            .into_iter()
            .map(|skill| ReferenceCandidatePayload {
                id: Some(skill.id),
                label: skill.name,
                path: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let tools = state.harness().map_or_else(Vec::new, |harness| {
        let mut tools = harness
            .tool_registry()
            .snapshot()
            .as_descriptors()
            .into_iter()
            .map(|descriptor| ReferenceCandidatePayload {
                id: Some(descriptor.name.clone()),
                label: descriptor.display_name.clone(),
                path: None,
            })
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
        tools
    });
    let mcp_servers = match list_mcp_servers_with_runtime_state(state).await {
        Ok(payload) => payload
            .servers
            .into_iter()
            .map(|server| ReferenceCandidatePayload {
                id: Some(server.id),
                label: server.display_name,
                path: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    Ok(ListReferenceCandidatesResponse {
        artifacts,
        conversations,
        files,
        memories,
        mcp_servers,
        skills,
        tools,
    })
}

async fn ensure_reference_conversation_exists(
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing reference candidates requires the runtime conversation facade.",
        ));
    };

    ensure_existing_conversation_session_with_harness(session_id, state, &harness).await
}

async fn ensure_existing_conversation_session(
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversations requires the runtime conversation facade.",
        ));
    };

    ensure_existing_conversation_session_with_harness(session_id, state, &harness).await
}

async fn ensure_existing_conversation_session_with_harness(
    session_id: SessionId,
    state: &DesktopRuntimeState,
    harness: &Harness,
) -> Result<(), CommandErrorPayload> {
    if session_id == state.default_conversation_id()
        && !state
            .deleted_conversation_ids
            .lock()
            .await
            .contains(&session_id)
    {
        harness
            .open_or_create_conversation_session(state.conversation_session_options(session_id))
            .await
            .map_err(|error| runtime_operation_failed(error.to_string()))?;
        return Ok(());
    }
    if harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Ok(());
    }

    Err(not_found(format!("conversation not found: {session_id}")))
}

pub fn cancel_run_payload(
    request: CancelRunRequest,
) -> Result<CancelRunResponse, CommandErrorPayload> {
    ensure_non_empty("runId", &request.run_id)?;

    Err(runtime_unavailable(
        "Cancelling runs requires the runtime conversation facade.",
    ))
}

pub async fn cancel_run_with_runtime_state(
    request: CancelRunRequest,
    state: &DesktopRuntimeState,
) -> Result<CancelRunResponse, CommandErrorPayload> {
    ensure_non_empty("runId", &request.run_id)?;
    let run_id = parse_run_id(&request.run_id)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Cancelling runs requires the runtime conversation facade.",
        ));
    };
    harness
        .cancel_conversation_run(run_id)
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?;

    Ok(CancelRunResponse {
        run_id: request.run_id,
        status: "cancelled",
    })
}

pub fn resolve_permission_payload(
    request: ResolvePermissionRequest,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;
    ensure_non_empty("requestId", &request.request_id)?;
    let _request_id = parse_request_id(&request.request_id)?;

    Err(runtime_unavailable(
        "Permission decisions require the runtime PermissionBroker.",
    ))
}

pub async fn resolve_permission_with_runtime_state(
    request: ResolvePermissionRequest,
    state: &DesktopRuntimeState,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("requestId", &request.request_id)?;

    let session_id = parse_session_id(&request.conversation_id)?;
    let request_id = parse_request_id(&request.request_id)?;
    let decision = to_harness_decision(request.decision);
    let Some(resolver) = state.permission_resolver.as_ref() else {
        return Err(runtime_unavailable(
            "Permission decisions require the runtime PermissionBroker.",
        ));
    };

    let Some(pending) = state
        .pending_permission_requests()
        .into_iter()
        .find(|pending| pending.request.request_id == request_id)
    else {
        return Err(not_found(format!(
            "permission request not found: {}",
            request.request_id
        )));
    };
    if pending.request.session_id != session_id {
        return Err(invalid_payload(
            "permission request does not belong to conversationId".to_owned(),
        ));
    }

    resolver.resolve_permission(request_id, decision).await?;

    Ok(ResolvePermissionResponse {
        decision: request.decision,
        request_id: request.request_id,
        status: "resolved",
    })
}

pub async fn resolve_permission_for_window_with_runtime_state(
    request: ResolvePermissionRequest,
    window_label: String,
    state: &DesktopRuntimeState,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    ensure_non_empty("windowLabel", &window_label)?;
    ensure_window_subscribed_to_conversation(state, &window_label, &request.conversation_id)
        .await?;
    resolve_permission_with_runtime_state(request, state).await
}

async fn ensure_window_subscribed_to_conversation(
    state: &DesktopRuntimeState,
    window_label: &str,
    conversation_id: &str,
) -> Result<(), CommandErrorPayload> {
    let subscriptions = state.conversation_event_subscriptions.lock().await;
    if subscriptions.values().any(|subscription| {
        subscription.window_label == window_label && subscription.conversation_id == conversation_id
    }) {
        return Ok(());
    }

    Err(invalid_payload(
        "permission request is not visible in this window".to_owned(),
    ))
}

pub fn list_activity_payload(
    request: ListActivityRequest,
) -> Result<ListActivityResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_activity(request.conversation_id.as_deref())?;

    Ok(ListActivityResponse { events: Vec::new() })
}

pub async fn list_activity_with_runtime_state(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<ListActivityResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_activity(request.conversation_id.as_deref())?;

    let mut events = read_activity_replay_events(&request, state).await?;
    events.retain(|event| event.event_type != "assistant.thinking.delta");

    Ok(ListActivityResponse { events })
}

pub async fn get_replay_timeline_with_runtime_state(
    request: ReplayTimelineRequest,
    state: &DesktopRuntimeState,
) -> Result<ReplayTimelineResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_replay(request.conversation_id.as_deref())?;

    let events = read_replay_run_events(
        ListActivityRequest {
            conversation_id: request.conversation_id,
            run_id: request.run_id,
        },
        state,
    )
    .await?;

    Ok(ReplayTimelineResponse {
        events,
        replayed: true,
    })
}

pub async fn page_conversation_timeline_with_runtime_state(
    request: PageConversationTimelineRequest,
    state: &DesktopRuntimeState,
) -> Result<PageConversationTimelineResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversation timeline requires the runtime conversation facade.",
        ));
    };
    let page = harness
        .page_conversation_timeline(
            &request.conversation_id,
            request.after_cursor,
            request
                .limit
                .unwrap_or(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT),
        )
        .await
        .map_err(conversation_read_error)?;
    let events = page
        .events
        .into_iter()
        .map(run_event_payload_from_read_model)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PageConversationTimelineResponse {
        events,
        cursor: page.cursor,
        gap: page.gap,
    })
}

pub async fn page_conversation_worktree_with_runtime_state(
    request: PageConversationWorktreeRequest,
    state: &DesktopRuntimeState,
) -> Result<ConversationWorktreePage, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversation worktree requires the runtime conversation facade.",
        ));
    };
    harness
        .page_conversation_worktree(
            &request.conversation_id,
            request.page_cursor,
            request.direction.into(),
            request.limit.unwrap_or(50),
        )
        .await
        .map_err(conversation_read_error)
}

pub async fn subscribe_conversation_events_with_runtime_state(
    request: SubscribeConversationEventsRequest,
    state: &DesktopRuntimeState,
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload> {
    subscribe_conversation_events_for_window_with_runtime_state(
        request,
        "default".to_owned(),
        Arc::new(|_batch| Ok(())),
        state,
    )
    .await
}

pub async fn subscribe_conversation_events_for_window_with_runtime_state(
    request: SubscribeConversationEventsRequest,
    window_label: String,
    emitter: ConversationEventBatchEmitter,
    state: &DesktopRuntimeState,
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("windowLabel", &window_label)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }

    let replay_page = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: request.conversation_id.clone(),
            after_cursor: request.after_cursor,
            limit: Some(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT),
        },
        state,
    )
    .await?;
    let cursor = replay_page.cursor;
    let replay_events = replay_page.events;
    let gap = replay_page.gap;
    let subscription_id = format!("subscription-{}", EventId::new());

    let handle = spawn_conversation_event_subscription(
        subscription_id.clone(),
        request.conversation_id.clone(),
        cursor.clone(),
        window_label.clone(),
        Arc::clone(&emitter),
        state.clone(),
    );
    state.conversation_event_subscriptions.lock().await.insert(
        subscription_id.clone(),
        ConversationSubscriptionHandle {
            conversation_id: request.conversation_id.clone(),
            task: handle,
            window_label,
        },
    );

    Ok(SubscribeConversationEventsResponse {
        subscription_id,
        conversation_id: request.conversation_id,
        replay_events,
        cursor,
        gap,
    })
}

pub async fn unsubscribe_conversation_events_with_runtime_state(
    request: UnsubscribeConversationEventsRequest,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    unsubscribe_conversation_events_for_window_with_runtime_state(
        request,
        "default".to_owned(),
        state,
    )
    .await
}

pub async fn unsubscribe_conversation_events_for_window_with_runtime_state(
    request: UnsubscribeConversationEventsRequest,
    window_label: String,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    ensure_non_empty("subscriptionId", &request.subscription_id)?;
    ensure_non_empty("windowLabel", &window_label)?;
    let mut subscriptions = state.conversation_event_subscriptions.lock().await;
    let removed = match subscriptions.get(&request.subscription_id) {
        Some(subscription) if subscription.window_label != window_label => {
            return Err(invalid_payload(
                "subscription does not belong to this window".to_owned(),
            ));
        }
        Some(_) => subscriptions.remove(&request.subscription_id),
        None => None,
    };
    drop(subscriptions);

    if let Some(subscription) = removed {
        let _ = &subscription.conversation_id;
        subscription.task.abort();
        return Ok(UnsubscribeConversationEventsResponse {
            subscription_id: request.subscription_id,
            status: "unsubscribed",
        });
    }

    Ok(UnsubscribeConversationEventsResponse {
        subscription_id: request.subscription_id,
        status: "alreadyClosed",
    })
}

pub fn unsubscribe_conversation_events_payload(
    request: UnsubscribeConversationEventsRequest,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    ensure_non_empty("subscriptionId", &request.subscription_id)?;

    Ok(UnsubscribeConversationEventsResponse {
        subscription_id: request.subscription_id,
        status: "alreadyClosed",
    })
}

fn spawn_conversation_event_subscription(
    subscription_id: String,
    conversation_id: String,
    initial_cursor: Option<ConversationCursor>,
    window_label: String,
    emitter: ConversationEventBatchEmitter,
    state: DesktopRuntimeState,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut cursor = initial_cursor;

        loop {
            tokio::time::sleep(CONVERSATION_SUBSCRIPTION_POLL_INTERVAL).await;
            let page = match page_conversation_timeline_with_runtime_state(
                PageConversationTimelineRequest {
                    conversation_id: conversation_id.clone(),
                    after_cursor: cursor.clone(),
                    limit: Some(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT),
                },
                &state,
            )
            .await
            {
                Ok(page) => page,
                Err(_) => {
                    let _ = emitter(ConversationEventBatchPayload {
                        subscription_id: subscription_id.clone(),
                        conversation_id: conversation_id.clone(),
                        events: Vec::new(),
                        cursor: None,
                        gap: true,
                        phase: "live",
                    });
                    break;
                }
            };

            if page.events.is_empty() {
                cursor = page.cursor.or(cursor);
                continue;
            }

            let mut emit_failed = false;
            for chunk in page.events.chunks(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT) {
                cursor = page.cursor.clone();
                let batch = ConversationEventBatchPayload {
                    subscription_id: subscription_id.clone(),
                    conversation_id: conversation_id.clone(),
                    events: chunk.to_vec(),
                    cursor: cursor.clone(),
                    gap: page.gap,
                    phase: "live",
                };
                if emitter(batch).is_err() {
                    emit_failed = true;
                    break;
                }
            }
            if emit_failed {
                break;
            }
        }

        state
            .conversation_event_subscriptions
            .lock()
            .await
            .remove(&subscription_id);
        let _ = window_label;
    })
}

pub async fn export_support_bundle_with_runtime_state(
    request: ExportSupportBundleRequest,
    state: &DesktopRuntimeState,
) -> Result<ExportSupportBundleResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_replay(request.conversation_id.as_deref())?;

    let events = read_replay_run_events(
        ListActivityRequest {
            conversation_id: request.conversation_id.clone(),
            run_id: request.run_id.clone(),
        },
        state,
    )
    .await
    .map_err(support_bundle_read_error)?;
    let event_count = events.len().min(u32::MAX as usize) as u32;
    let exported_at = now();
    let stamp = exported_at.format("%Y%m%dT%H%M%S%.3fZ");
    let export_id = RunId::new();
    let export_dir = PathBuf::from(".jyowo").join("runtime").join("exports");
    let jsonl_path = export_dir.join(format!("events-{stamp}-{export_id}.jsonl"));
    let markdown_path = export_dir.join(format!("support-report-{stamp}-{export_id}.md"));
    let bundle_path = export_dir.join(format!("support-bundle-{stamp}-{export_id}.json"));
    let jsonl = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| support_bundle_operation_failed())?
        .join("\n");
    let markdown = support_bundle_markdown(&request, exported_at.to_rfc3339(), event_count);
    let bundle = json!({
        "conversationId": request.conversation_id,
        "runId": request.run_id,
        "exportedAt": exported_at.to_rfc3339(),
        "eventCount": event_count,
        "redacted": true,
        "events": events,
    });
    let bundle = serde_json::to_string(&bundle).map_err(|_| support_bundle_operation_failed())?;

    write_support_bundle_file(&state.workspace_root.join(&jsonl_path), &jsonl)?;
    write_support_bundle_file(&state.workspace_root.join(&markdown_path), &markdown)?;
    write_support_bundle_file(&state.workspace_root.join(&bundle_path), &bundle)?;

    Ok(ExportSupportBundleResponse {
        bundle_path: bundle_path.to_string_lossy().into_owned(),
        event_count,
        exported_at: exported_at.to_rfc3339(),
        jsonl_path: jsonl_path.to_string_lossy().into_owned(),
        markdown_path: markdown_path.to_string_lossy().into_owned(),
        redacted: true,
    })
}

pub async fn get_context_snapshot_with_runtime_state(
    request: GetContextSnapshotRequest,
    state: &DesktopRuntimeState,
) -> Result<GetContextSnapshotResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    let session_id = match request.conversation_id.as_deref() {
        Some(conversation_id) => parse_session_id(conversation_id)?,
        None => state.default_conversation_id(),
    };
    let run_id = request.run_id.as_deref().map(parse_run_id).transpose()?;
    let Some(_harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading context snapshot requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut active_artifact = None;
    let mut next_actions = Vec::new();

    // Context snapshot is display-only metadata. If a selected conversation has no event stream
    // yet, keep the workspace metadata visible instead of failing the UI.
    if let Ok(payload) = collect_artifacts_from_runtime_state(state, session_id).await {
        active_artifact = payload
            .artifacts
            .into_iter()
            .find(|artifact| {
                run_id
                    .as_ref()
                    .is_none_or(|run_id| artifact.source_run_id == run_id.to_string())
            })
            .map(|artifact| artifact.title);
    }

    if let Some(title) = active_artifact.as_ref() {
        next_actions.push(format!("Review {title}"));
    }
    let decisions =
        context_decisions_from_pending_requests(state, session_id, run_id.as_ref(), &redactor);
    if !decisions.is_empty() {
        next_actions.push("Resolve pending runtime decisions".to_owned());
    }
    if next_actions.is_empty() {
        next_actions.push("Continue the conversation".to_owned());
    }

    Ok(GetContextSnapshotResponse {
        active_artifact,
        decisions,
        files: context_files_from_workspace(state.workspace_root()),
        next_actions,
        path: "workspace://local".to_owned(),
        project: redacted_display(workspace_project_name(state.workspace_root()), &redactor),
    })
}

async fn conversation_tail_event_id(
    harness: &Harness,
    options: SessionOptions,
) -> Result<Option<EventId>, CommandErrorPayload> {
    let mut after_event_id = None;
    let mut tail_event_id = None;

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: options.clone(),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation event page failed: {error}"))
            })?;
        let Some(next_event_id) = page.next_event_id else {
            return Ok(tail_event_id);
        };

        tail_event_id = Some(next_event_id);
        after_event_id = Some(next_event_id);
    }
}

async fn wait_for_started_conversation_run(
    harness: &Harness,
    options: SessionOptions,
    mut after_event_id: Option<EventId>,
    run_task: &mut tokio::task::JoinHandle<
        Result<jyowo_harness_sdk::ConversationTurnReceipt, jyowo_harness_sdk::HarnessError>,
    >,
) -> Result<RunId, CommandErrorPayload> {
    let deadline = tokio::time::Instant::now() + START_RUN_STARTED_TIMEOUT;

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: options.clone(),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation event page failed: {error}"))
            })?;

        for envelope in &page.events {
            if let Event::RunStarted(started) = &envelope.payload {
                if started.session_id == options.session_id
                    && started.tenant_id == options.tenant_id
                {
                    return Ok(started.run_id);
                }
            }
        }

        if let Some(next_event_id) = page.next_event_id {
            after_event_id = Some(next_event_id);
        }

        if run_task.is_finished() {
            let receipt = run_task.await.map_err(|error| {
                runtime_operation_failed(format!("conversation run task failed: {error}"))
            })?;
            return receipt.map(|receipt| receipt.run_id).map_err(|error| {
                runtime_operation_failed(format!("conversation run failed: {error}"))
            });
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(runtime_operation_failed(
                "conversation run did not emit RunStarted before timeout".to_owned(),
            ));
        }

        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn ensure_non_empty(field: &'static str, value: &str) -> Result<(), CommandErrorPayload> {
    if value.trim().is_empty() {
        return Err(invalid_payload(format!("{field} must not be empty")));
    }

    Ok(())
}

fn ensure_max_bytes(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), CommandErrorPayload> {
    if value.len() > max_bytes {
        return Err(invalid_payload(format!(
            "{field} must be at most {max_bytes} bytes"
        )));
    }

    Ok(())
}

fn ensure_optional(field: &'static str, value: Option<&str>) -> Result<(), CommandErrorPayload> {
    if let Some(value) = value {
        ensure_non_empty(field, value)?;
    }

    Ok(())
}

fn validate_context_reference_payloads(
    references: Option<&[ContextReferencePayload]>,
) -> Result<(), CommandErrorPayload> {
    if let Some(references) = references {
        for reference in references {
            match reference {
                ContextReferencePayload::WorkspaceFile { path, label } => {
                    ensure_non_empty("contextReferences.path", path)?;
                    ensure_non_empty("contextReferences.label", label)?;
                }
                ContextReferencePayload::Artifact { id, label }
                | ContextReferencePayload::Conversation { id, label }
                | ContextReferencePayload::Memory { id, label }
                | ContextReferencePayload::Skill { id, label }
                | ContextReferencePayload::Tool { id, label }
                | ContextReferencePayload::McpServer { id, label } => {
                    ensure_non_empty("contextReferences.id", id)?;
                    ensure_non_empty("contextReferences.label", label)?;
                }
            }
        }
    }

    Ok(())
}

fn validate_attachment_reference_payloads(
    attachments: Option<&[AttachmentReferencePayload]>,
) -> Result<(), CommandErrorPayload> {
    if let Some(attachments) = attachments {
        let mut total_size = 0_u64;
        for attachment in attachments {
            ensure_attachment_id(&attachment.id)?;
            ensure_non_empty("attachments.name", &attachment.name)?;
            ensure_non_empty("attachments.mimeType", &attachment.mime_type)?;
            if attachment.size_bytes > MAX_ATTACHMENT_BYTES {
                return Err(invalid_payload(format!(
                    "attachment must be at most {} MB",
                    MAX_ATTACHMENT_BYTES / 1024 / 1024
                )));
            }
            total_size = total_size.saturating_add(attachment.size_bytes);
        }
        if total_size > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(invalid_payload(format!(
                "attachments must total at most {} MB",
                MAX_TOTAL_ATTACHMENT_BYTES / 1024 / 1024
            )));
        }
    }

    Ok(())
}

fn ensure_attachment_id(value: &str) -> Result<(), CommandErrorPayload> {
    const PREFIX: &str = "attachment-";

    ensure_non_empty("attachments.id", value)?;
    let Some(hex) = value.strip_prefix(PREFIX) else {
        return Err(invalid_payload(
            "attachments.id must be a generated attachment id".to_owned(),
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_payload(
            "attachments.id must be a generated attachment id".to_owned(),
        ));
    }

    Ok(())
}

async fn build_conversation_turn_input(
    request: &StartRunRequest,
    state: &DesktopRuntimeState,
) -> Result<ConversationTurnInput, CommandErrorPayload> {
    validate_context_reference_payloads(request.context_references.as_deref())?;
    validate_attachment_reference_payloads(request.attachments.as_deref())?;
    let session_id = parse_session_id(&request.conversation_id)?;

    Ok(ConversationTurnInput {
        prompt: request.prompt.clone(),
        client_message_id: request.client_message_id.clone(),
        context_references: validate_context_references(
            request.context_references.as_deref().unwrap_or_default(),
            session_id,
            state,
        )
        .await?,
        attachments: validate_attachment_references(
            request.attachments.as_deref().unwrap_or_default(),
            state.workspace_root(),
        )?,
    })
}

fn resolve_start_run_permission_mode(
    requested: Option<PermissionMode>,
    store: &DesktopExecutionSettingsStore,
) -> Result<PermissionMode, CommandErrorPayload> {
    let permission_mode = match requested {
        Some(permission_mode) => permission_mode,
        None => effective_execution_settings_permission_mode(store.load_record()?.permission_mode),
    };
    ensure_start_run_permission_mode(permission_mode)?;
    Ok(permission_mode)
}

fn effective_execution_settings_permission_mode(permission_mode: PermissionMode) -> PermissionMode {
    if permission_mode == PermissionMode::Auto && !auto_mode_available() {
        PermissionMode::Default
    } else {
        permission_mode
    }
}

fn ensure_start_run_permission_mode(
    permission_mode: PermissionMode,
) -> Result<(), CommandErrorPayload> {
    match permission_mode {
        PermissionMode::Default | PermissionMode::Auto | PermissionMode::BypassPermissions => {}
        _ => {
            return Err(invalid_payload(
                "permissionMode must be default, auto, or bypass_permissions".to_owned(),
            ));
        }
    }
    if permission_mode == PermissionMode::Auto && !auto_mode_available() {
        return Err(invalid_payload(
            "permissionMode auto is not available in this desktop build".to_owned(),
        ));
    }
    Ok(())
}

async fn validate_context_references(
    references: &[ContextReferencePayload],
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<Vec<ConversationContextReference>, CommandErrorPayload> {
    let mut validated = Vec::with_capacity(references.len());

    for reference in references {
        validated.push(match reference {
            ContextReferencePayload::WorkspaceFile { path, label } => {
                let absolute_path = state.workspace_root().join(path);
                let canonical_path = absolute_path.canonicalize().map_err(|error| {
                    invalid_payload(format!("workspace file reference is invalid: {error}"))
                })?;
                let relative_path = workspace_relative_path(
                    &canonical_path,
                    state.workspace_root(),
                )
                .ok_or_else(|| {
                    invalid_payload(
                        "workspace file reference must stay inside the workspace".to_owned(),
                    )
                })?;
                ConversationContextReference::WorkspaceFile {
                    path: relative_path,
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Artifact { id, label } => {
                ensure_artifact_exists(id, session_id, state).await?;
                ConversationContextReference::Artifact {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Conversation { id, label } => {
                ensure_conversation_exists(id, state).await?;
                ConversationContextReference::Conversation {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Memory { id, label } => {
                ensure_memory_exists(id, state).await?;
                ConversationContextReference::Memory {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Skill { id, label } => {
                ensure_skill_exists(id, state).await?;
                ConversationContextReference::Skill {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Tool { id, label } => {
                ensure_tool_exists(id, state)?;
                ConversationContextReference::Tool {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::McpServer { id, label } => {
                ensure_mcp_server_exists(id, state).await?;
                ConversationContextReference::McpServer {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
        });
    }

    Ok(validated)
}

fn validate_attachment_references(
    attachments: &[AttachmentReferencePayload],
    workspace_root: &Path,
) -> Result<Vec<ConversationAttachmentReference>, CommandErrorPayload> {
    let mut validated = Vec::with_capacity(attachments.len());

    for attachment in attachments {
        let record = read_attachment_record(workspace_root, &attachment.id)?;
        if record.attachment != *attachment {
            return Err(invalid_payload(
                "attachment reference does not match stored metadata".to_owned(),
            ));
        }
        validated.push(ConversationAttachmentReference {
            id: attachment.id.clone(),
            name: attachment.name.clone(),
            mime_type: attachment.mime_type.clone(),
            size_bytes: attachment.size_bytes,
            blob_ref: record.blob_ref.clone(),
        });
    }

    Ok(validated)
}

fn attachment_blob_ref_payload(blob_ref: &BlobRef) -> AttachmentBlobRefPayload {
    AttachmentBlobRefPayload {
        id: blob_ref.id.to_string(),
        size: blob_ref.size,
        content_hash: blob_ref.content_hash,
        content_type: blob_ref.content_type.clone(),
    }
}

async fn ensure_artifact_exists(
    id: &str,
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let artifacts = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        state,
    )
    .await?;
    if artifacts.artifacts.iter().any(|artifact| artifact.id == id) {
        Ok(())
    } else {
        Err(invalid_payload(
            "artifact reference does not exist".to_owned(),
        ))
    }
}

async fn ensure_conversation_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let conversations = list_conversations_with_runtime_state(state).await;
    if conversations
        .conversations
        .iter()
        .any(|conversation| conversation.id == id)
    {
        Ok(())
    } else {
        Err(invalid_payload(
            "conversation reference does not exist".to_owned(),
        ))
    }
}

async fn ensure_memory_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let memories = list_memory_items_with_runtime_state(state).await?;
    if memories.items.iter().any(|memory| memory.id == id) {
        Ok(())
    } else {
        Err(invalid_payload(
            "memory reference does not exist".to_owned(),
        ))
    }
}

async fn ensure_skill_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let skills = list_skills_with_runtime_state(state).await?;
    if skills.skills.iter().any(|skill| skill.id == id) {
        Ok(())
    } else {
        Err(invalid_payload("skill reference does not exist".to_owned()))
    }
}

fn ensure_tool_exists(id: &str, state: &DesktopRuntimeState) -> Result<(), CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Validating tool references requires the runtime tool registry.",
        ));
    };

    if harness.tool_registry().snapshot().descriptor(id).is_some() {
        Ok(())
    } else {
        Err(invalid_payload("tool reference does not exist".to_owned()))
    }
}

async fn ensure_mcp_server_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let servers = list_mcp_servers_with_runtime_state(state).await?;
    if servers.servers.iter().any(|server| server.id == id) {
        Ok(())
    } else {
        Err(invalid_payload(
            "mcp server reference does not exist".to_owned(),
        ))
    }
}

fn ensure_eval_case_id(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("caseId", value)?;
    if value.len() > 64 {
        return Err(invalid_payload(
            "caseId must be at most 64 bytes".to_owned(),
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_payload(
            "caseId may only contain ASCII letters, digits, dots, underscores, and hyphens"
                .to_owned(),
        ));
    }

    Ok(())
}

fn require_conversation_id_for_replay(value: Option<&str>) -> Result<(), CommandErrorPayload> {
    if value.is_none() {
        return Err(invalid_payload(
            "conversationId is required for replay and support bundle export".to_owned(),
        ));
    }

    Ok(())
}

fn require_conversation_id_for_activity(value: Option<&str>) -> Result<(), CommandErrorPayload> {
    if value.is_none() {
        return Err(invalid_payload(
            "conversationId is required for activity listing".to_owned(),
        ));
    }

    Ok(())
}

fn ensure_provider_settings(request: &ProviderSettingsRequest) -> Result<(), CommandErrorPayload> {
    ensure_provider_metadata_shape(&request.provider_id, &request.model_id)?;
    if let Some(config_id) = &request.config_id {
        ensure_provider_config_id(config_id)?;
    }
    if let Some(display_name) = &request.display_name {
        ensure_optional("displayName", Some(display_name))?;
    }
    let _ = normalized_base_url(request.base_url.as_deref())?;

    Ok(())
}

fn ensure_provider_metadata_shape(
    provider_id: &str,
    model_id: &str,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("providerId", provider_id)?;
    ensure_non_empty("modelId", model_id)
}

fn ensure_provider_config_id(value: &str) -> Result<(), CommandErrorPayload> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_payload("configId must not be empty".to_owned()));
    }
    if trimmed.len() > 64
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(invalid_payload(
            "configId must contain only letters, numbers, dot, dash, or underscore".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_mcp_server_request(request: &SaveMcpServerRequest) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("displayName", &request.display_name)?;
    ensure_mcp_server_id(&request.id)?;
    ensure_mcp_server_scope(&request.scope)?;
    ensure_mcp_server_transport(&request.transport)
}

fn ensure_mcp_server_record(record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("displayName", &record.display_name)?;
    ensure_mcp_server_id(&record.id)?;
    ensure_mcp_server_scope(&record.scope)?;
    ensure_mcp_server_transport(&record.transport)
}

fn ensure_mcp_server_transport(
    transport: &McpServerTransportConfig,
) -> Result<(), CommandErrorPayload> {
    match transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => {
            ensure_non_empty("transport.command", command)?;
            if args.iter().any(|arg| arg.trim().is_empty()) {
                return Err(invalid_payload(
                    "transport.args must not contain empty values".to_owned(),
                ));
            }
            if args.len() > 64 {
                return Err(invalid_payload(
                    "transport.args must contain at most 64 values".to_owned(),
                ));
            }
            if args
                .iter()
                .any(|arg| mcp_stdio_arg_looks_secret_bearing(arg))
            {
                return Err(invalid_payload(
                    "transport.args must not contain secret-bearing values".to_owned(),
                ));
            }
            if env.len() > 64 {
                return Err(invalid_payload(
                    "transport.env must contain at most 64 values".to_owned(),
                ));
            }
            for item in env {
                ensure_env_var_name("transport.env.key", &item.key)?;
                ensure_max_bytes("transport.env.value", &item.value, 4096)?;
                if mcp_env_key_looks_secret_bearing(&item.key) || looks_like_raw_secret(&item.value)
                {
                    return Err(invalid_payload(
                        "transport.env must not contain secret-bearing values".to_owned(),
                    ));
                }
            }
            if inherit_env.len() > 128 {
                return Err(invalid_payload(
                    "transport.inheritEnv must contain at most 128 values".to_owned(),
                ));
            }
            for item in inherit_env {
                ensure_env_var_name("transport.inheritEnv", item)?;
            }
            if let Some(working_dir) = working_dir {
                ensure_non_empty("transport.workingDir", working_dir)?;
                ensure_max_bytes("transport.workingDir", working_dir, 4096)?;
            }
        }
        McpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => {
            ensure_mcp_http_url(url)?;
            if let Some(env_var) = bearer_token_env_var {
                ensure_env_var_name("transport.bearerTokenEnvVar", env_var)?;
            }
            if headers.len() > 64 || headers_from_env.len() > 64 {
                return Err(invalid_payload(
                    "transport.headers must contain at most 64 values".to_owned(),
                ));
            }
            for header in headers {
                ensure_http_header_name("transport.headers.key", &header.key)?;
                ensure_max_bytes("transport.headers.value", &header.value, 8192)?;
                if mcp_http_header_is_sensitive(&header.key)
                    || looks_like_raw_secret(&header.value)
                    || mcp_header_value_looks_secret_bearing(&header.value)
                {
                    return Err(invalid_payload(
                        "transport.headers must not contain secret-bearing values".to_owned(),
                    ));
                }
            }
            for header in headers_from_env {
                ensure_http_header_name("transport.headersFromEnv.key", &header.key)?;
                ensure_env_var_name("transport.headersFromEnv.envVar", &header.env_var)?;
                if mcp_http_header_is_sensitive(&header.key) {
                    return Err(invalid_payload(
                        "transport.headersFromEnv must not contain sensitive header names"
                            .to_owned(),
                    ));
                }
            }
        }
        McpServerTransportConfig::InProcess => {
            return Err(invalid_payload(
                "transport.kind must be stdio or http for workspace MCP servers".to_owned(),
            ));
        }
    }

    Ok(())
}

fn ensure_env_var_name(field: &'static str, value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty(field, value)?;
    let mut chars = value.chars();
    let valid = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid {
        return Err(invalid_payload(format!("{field} is invalid")));
    }
    Ok(())
}

fn ensure_http_header_name(field: &'static str, value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty(field, value)?;
    reqwest::header::HeaderName::from_bytes(value.trim().as_bytes())
        .map_err(|_| invalid_payload(format!("{field} is invalid")))?;
    Ok(())
}

fn ensure_mcp_http_url(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("transport.url", value)?;
    let url = reqwest::Url::parse(value)
        .map_err(|error| invalid_payload(format!("transport.url is invalid: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid_payload(
            "transport.url must be an http or https URL".to_owned(),
        ));
    }
    Ok(())
}

fn mcp_env_key_looks_secret_bearing(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('-', "_");
    [
        "auth",
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn mcp_http_header_is_sensitive(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie" | "proxy-authorization"
    )
}

fn mcp_header_value_looks_secret_bearing(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("bearer ")
        || normalized.contains(" token")
        || normalized.contains("secret")
        || normalized.contains("password")
}

fn mcp_stdio_arg_looks_secret_bearing(arg: &str) -> bool {
    let normalized = arg.to_ascii_lowercase().replace('-', "_");
    [
        "auth",
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
        || looks_like_raw_secret(arg)
}

fn looks_like_raw_secret(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    let known_prefix = [
        "ghp_",
        "github_pat_",
        "glpat-",
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    known_prefix || (trimmed.len() >= 32 && trimmed.chars().all(is_secretish_character))
}

fn is_secretish_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '=' | '/' | '+')
}

fn ensure_mcp_server_id(id: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("id", id)?;
    let valid = id.len() <= 64
        && id
            .chars()
            .enumerate()
            .all(|(index, character)| match character {
                'A'..='Z' | 'a'..='z' | '0'..='9' => true,
                '.' | '-' | '_' if index > 0 => true,
                _ => false,
            });
    if !valid {
        return Err(invalid_payload(
            "id must use letters, numbers, dot, dash, or underscore".to_owned(),
        ));
    }

    Ok(())
}

fn ensure_mcp_server_scope(scope: &str) -> Result<(), CommandErrorPayload> {
    match scope {
        "agent" | "global" | "session" => Ok(()),
        _ => Err(invalid_payload("unsupported MCP server scope".to_owned())),
    }
}

fn mcp_server_origin_payload(source: &McpServerSource) -> &'static str {
    match source {
        McpServerSource::Workspace | McpServerSource::Project => "workspace",
        McpServerSource::User => "user",
        McpServerSource::Policy => "policy",
        McpServerSource::Plugin(_) => "plugin",
        McpServerSource::Dynamic { .. } | McpServerSource::Managed { .. } => "managed",
        _ => "managed",
    }
}

fn mcp_source_plugin_id(source: &McpServerSource) -> Option<String> {
    match source {
        McpServerSource::Plugin(plugin_id) => Some(plugin_id.0.clone()),
        _ => None,
    }
}

fn mcp_server_scope_payload(scope: &McpServerScope) -> String {
    match scope {
        McpServerScope::Global => "global".to_owned(),
        McpServerScope::Session(_) => "session".to_owned(),
        McpServerScope::Agent(_) => "agent".to_owned(),
        _ => "session".to_owned(),
    }
}

fn mcp_transport_payload(transport: &TransportChoice) -> &'static str {
    match transport {
        TransportChoice::Stdio { .. } => "stdio",
        TransportChoice::Http { .. } => "http",
        TransportChoice::WebSocket { .. } => "websocket",
        TransportChoice::Sse { .. } => "sse",
        TransportChoice::InProcess => "inProcess",
        _ => "inProcess",
    }
}

fn mcp_transport_config_payload(transport: &McpServerTransportConfig) -> &'static str {
    match transport {
        McpServerTransportConfig::Stdio { .. } => "stdio",
        McpServerTransportConfig::Http { .. } => "http",
        McpServerTransportConfig::InProcess => "inProcess",
    }
}

fn mcp_connection_state_payload(state: &McpConnectionState) -> (&'static str, Option<String>) {
    match state {
        McpConnectionState::Connecting => ("connecting", None),
        McpConnectionState::Ready => ("ready", None),
        McpConnectionState::Reconnecting { .. } => (
            "reconnecting",
            Some("MCP server is reconnecting.".to_owned()),
        ),
        McpConnectionState::Failed { .. } => {
            ("failed", Some("MCP server connection failed.".to_owned()))
        }
        McpConnectionState::Closed => ("closed", None),
    }
}

trait ProviderSettingsMetadata {
    fn provider_id(&self) -> &str;
    fn model_id(&self) -> &str;
}

impl ProviderSettingsMetadata for ProviderSettingsRequest {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl ProviderSettingsMetadata for ValidateProviderSettingsRequest {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

async fn ensure_provider_model_supported<T: ProviderSettingsMetadata>(
    request: &T,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    ensure_provider_metadata_shape(request.provider_id(), request.model_id())?;
    resolve_provider_model_descriptor(request.provider_id(), request.model_id()).await
}

async fn provider_settings_descriptor(
    request: &ProviderSettingsRequest,
    previous_config: Option<&ProviderConfigRecord>,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    ensure_provider_metadata_shape(&request.provider_id, &request.model_id)?;
    match resolve_model_descriptor(&request.provider_id, &request.model_id) {
        Ok(descriptor) => Ok(descriptor),
        Err(error) if request.provider_id == "openrouter" => {
            if let Some(previous_config) = previous_config {
                if previous_config.provider_id == request.provider_id
                    && previous_config.model_id == request.model_id
                {
                    return provider_config_descriptor(previous_config);
                }
            }
            resolve_provider_model_descriptor(&request.provider_id, &request.model_id)
                .await
                .map_err(|_| provider_registry_error(error))
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

async fn resolve_provider_model_descriptor(
    provider_id: &str,
    model_id: &str,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match resolve_model_descriptor(provider_id, model_id) {
        Ok(descriptor) => Ok(descriptor),
        Err(error) if provider_id == "openrouter" => {
            let Some(inventory) = fetch_openrouter_inventory().await else {
                return Err(provider_registry_error(error));
            };
            let Some(model) = inventory
                .into_iter()
                .find(|model| model.model_id == model_id)
            else {
                return Err(provider_registry_error(error));
            };
            descriptor_from_inventory_model(model)
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

fn provider_registry_error(error: ProviderRegistryError) -> CommandErrorPayload {
    invalid_payload(error.to_string())
}

fn provider_registry_init_error(error: ProviderRegistryError) -> CommandErrorPayload {
    runtime_init_failed(error.to_string())
}

fn provider_display_name(provider_id: &str) -> String {
    provider_catalog_entries()
        .into_iter()
        .find(|entry| entry.provider_id == provider_id)
        .map_or_else(|| provider_id.to_owned(), |entry| entry.display_name)
}

fn model_descriptor_catalog_entry(descriptor: ModelDescriptor) -> ModelCatalogEntry {
    let conversation_capability = descriptor.conversation_capability;
    ModelCatalogEntry {
        protocol: descriptor.protocol,
        conversation_capability: conversation_capability_record(&conversation_capability),
        context_window: descriptor.context_window,
        display_name: descriptor.display_name,
        lifecycle: model_lifecycle_payload(descriptor.lifecycle),
        max_output_tokens: descriptor.max_output_tokens,
        model_id: descriptor.model_id,
        runtime_status: ModelRuntimeStatusPayload {
            kind: "runnable",
            reason: None,
        },
    }
}

fn model_lifecycle_payload(lifecycle: ModelLifecycle) -> ModelLifecyclePayload {
    match lifecycle {
        ModelLifecycle::Stable => ModelLifecyclePayload {
            kind: "stable",
            retirement_date: None,
        },
        ModelLifecycle::Preview => ModelLifecyclePayload {
            kind: "preview",
            retirement_date: None,
        },
        ModelLifecycle::Deprecated { retirement_date } => ModelLifecyclePayload {
            kind: "deprecated",
            retirement_date: Some(retirement_date.to_string()),
        },
    }
}

fn model_descriptor_record(descriptor: &ModelDescriptor) -> ProviderModelDescriptorRecord {
    ProviderModelDescriptorRecord {
        protocol: descriptor.protocol,
        conversation_capability: conversation_capability_record(
            &descriptor.conversation_capability,
        ),
        context_window: descriptor.context_window,
        display_name: descriptor.display_name.clone(),
        lifecycle: model_lifecycle_record(&descriptor.lifecycle),
        max_output_tokens: descriptor.max_output_tokens,
        model_id: descriptor.model_id.clone(),
        provider_id: descriptor.provider_id.clone(),
    }
}

fn conversation_capability_record(
    capabilities: &ConversationModelCapability,
) -> ConversationModelCapabilityRecord {
    ConversationModelCapabilityRecord {
        input_modalities: capabilities
            .input_modalities
            .iter()
            .map(model_modality_record)
            .collect(),
        output_modalities: capabilities
            .output_modalities
            .iter()
            .map(model_modality_record)
            .collect(),
        context_window: capabilities.context_window,
        max_output_tokens: capabilities.max_output_tokens,
        streaming: capabilities.streaming,
        tool_calling: capabilities.tool_calling,
        reasoning: capabilities.reasoning,
        prompt_cache: capabilities.prompt_cache,
        structured_output: capabilities.structured_output,
    }
}

fn model_lifecycle_record(lifecycle: &ModelLifecycle) -> ProviderModelLifecycleRecord {
    match lifecycle {
        ModelLifecycle::Stable => ProviderModelLifecycleRecord::Stable,
        ModelLifecycle::Preview => ProviderModelLifecycleRecord::Preview,
        ModelLifecycle::Deprecated { retirement_date } => {
            ProviderModelLifecycleRecord::Deprecated {
                retirement_date: retirement_date.to_string(),
            }
        }
    }
}

fn model_modality_record(modality: &ModelModality) -> ProviderModelModalityRecord {
    match modality {
        ModelModality::Text => ProviderModelModalityRecord::Text,
        ModelModality::Image => ProviderModelModalityRecord::Image,
        ModelModality::Audio => ProviderModelModalityRecord::Audio,
        ModelModality::Video => ProviderModelModalityRecord::Video,
        ModelModality::File => ProviderModelModalityRecord::File,
        ModelModality::Embedding => ProviderModelModalityRecord::Embedding,
    }
}

fn model_descriptor_from_record(
    record: &ProviderModelDescriptorRecord,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    Ok(ModelDescriptor {
        provider_id: record.provider_id.clone(),
        model_id: record.model_id.clone(),
        display_name: record.display_name.clone(),
        protocol: record.protocol,
        context_window: record.context_window,
        max_output_tokens: record.max_output_tokens,
        conversation_capability: conversation_capability_from_record(
            &record.conversation_capability,
        ),
        lifecycle: model_lifecycle_from_record(&record.lifecycle)?,
        pricing: None,
    })
}

fn conversation_capability_from_record(
    record: &ConversationModelCapabilityRecord,
) -> ConversationModelCapability {
    ConversationModelCapability {
        input_modalities: record
            .input_modalities
            .iter()
            .map(model_modality_from_record)
            .collect(),
        output_modalities: record
            .output_modalities
            .iter()
            .map(model_modality_from_record)
            .collect(),
        context_window: record.context_window,
        max_output_tokens: record.max_output_tokens,
        streaming: record.streaming,
        tool_calling: record.tool_calling,
        reasoning: record.reasoning,
        prompt_cache: record.prompt_cache,
        structured_output: record.structured_output,
    }
}

fn model_lifecycle_from_record(
    record: &ProviderModelLifecycleRecord,
) -> Result<ModelLifecycle, CommandErrorPayload> {
    match record {
        ProviderModelLifecycleRecord::Stable => Ok(ModelLifecycle::Stable),
        ProviderModelLifecycleRecord::Preview => Ok(ModelLifecycle::Preview),
        ProviderModelLifecycleRecord::Deprecated { retirement_date } => {
            let retirement_date =
                NaiveDate::parse_from_str(retirement_date, "%Y-%m-%d").map_err(|_| {
                    runtime_init_failed("provider model descriptor is invalid".to_owned())
                })?;
            Ok(ModelLifecycle::Deprecated { retirement_date })
        }
    }
}

fn model_modality_from_record(record: &ProviderModelModalityRecord) -> ModelModality {
    match record {
        ProviderModelModalityRecord::Text => ModelModality::Text,
        ProviderModelModalityRecord::Image => ModelModality::Image,
        ProviderModelModalityRecord::Audio => ModelModality::Audio,
        ProviderModelModalityRecord::Video => ModelModality::Video,
        ProviderModelModalityRecord::File => ModelModality::File,
        ProviderModelModalityRecord::Embedding => ModelModality::Embedding,
    }
}

fn descriptor_from_inventory_model(
    model: ModelInventoryEntry,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match model.runtime_status {
        ModelRuntimeStatus::Runnable => Ok(ModelDescriptor {
            provider_id: model.provider_id,
            model_id: model.model_id,
            display_name: model.display_name,
            protocol: model.protocol,
            context_window: model.context_window,
            max_output_tokens: model.max_output_tokens,
            conversation_capability: model.conversation_capability,
            lifecycle: model.lifecycle,
            pricing: model.pricing,
        }),
        ModelRuntimeStatus::Unsupported { reason } => Err(invalid_payload(format!(
            "model is not supported by the current runtime: {reason}"
        ))),
    }
}

fn provider_config_id(
    record: &ProviderSettingsRecord,
    request: &ProviderSettingsRequest,
) -> String {
    if let Some(config_id) = request
        .config_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return config_id.to_owned();
    }
    let provider_id = request.provider_id.clone();
    if record.configs.iter().any(|config| config.id == provider_id) {
        return format!("{provider_id}-{}", RunId::new());
    }
    provider_id
}

fn provider_config_payloads(
    record: &ProviderSettingsRecord,
) -> Result<Vec<ProviderConfigPayload>, CommandErrorPayload> {
    record
        .configs
        .iter()
        .map(|config| provider_config_payload(config, record.default_config_id.as_deref()))
        .collect()
}

fn provider_config_payload(
    config: &ProviderConfigRecord,
    default_config_id: Option<&str>,
) -> Result<ProviderConfigPayload, CommandErrorPayload> {
    let descriptor = provider_config_descriptor(config)?;
    Ok(ProviderConfigPayload {
        protocol: descriptor.protocol,
        base_url: config.base_url.clone(),
        display_name: config.display_name.clone(),
        has_api_key: provider_config_has_api_key(config),
        id: config.id.clone(),
        is_default: default_config_id.is_some_and(|id| id == config.id),
        model_id: config.model_id.clone(),
        provider_id: config.provider_id.clone(),
        model_descriptor: model_descriptor_catalog_entry(descriptor),
    })
}

fn provider_config_has_api_key(config: &ProviderConfigRecord) -> bool {
    !config.api_key.trim().is_empty()
}

fn ensure_provider_config_has_api_key(
    config: &ProviderConfigRecord,
) -> Result<&str, CommandErrorPayload> {
    config
        .api_key
        .trim()
        .is_empty()
        .then(|| invalid_payload("apiKey is not configured for this provider config".to_owned()))
        .map_or_else(|| Ok(config.api_key.trim()), Err)
}

fn provider_config_descriptor(
    config: &ProviderConfigRecord,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match resolve_model_descriptor(&config.provider_id, &config.model_id) {
        Ok(descriptor) => Ok(descriptor),
        Err(_) if config.provider_id == "openrouter" => {
            let descriptor = model_descriptor_from_record(&config.model_descriptor)?;
            if descriptor.provider_id != config.provider_id
                || descriptor.model_id != config.model_id
            {
                return Err(runtime_init_failed(
                    "provider model descriptor does not match provider config".to_owned(),
                ));
            }
            if descriptor.protocol != ModelProtocol::ChatCompletions {
                return Err(runtime_init_failed(
                    "provider model descriptor protocol is not supported".to_owned(),
                ));
            }
            if !descriptor.conversation_capability.streaming {
                return Err(runtime_init_failed(
                    "provider model descriptor is not runnable".to_owned(),
                ));
            }
            if !descriptor_has_runtime_supported_modalities(&descriptor) {
                return Err(runtime_init_failed(
                    "provider model descriptor is not supported by the current runtime".to_owned(),
                ));
            }
            Ok(descriptor)
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

fn descriptor_has_runtime_supported_modalities(descriptor: &ModelDescriptor) -> bool {
    descriptor
        .conversation_capability
        .input_modalities
        .iter()
        .all(|modality| matches!(modality, ModelModality::Text))
        && descriptor
            .conversation_capability
            .output_modalities
            .iter()
            .all(|modality| matches!(modality, ModelModality::Text))
        && !descriptor
            .conversation_capability
            .input_modalities
            .is_empty()
        && !descriptor
            .conversation_capability
            .output_modalities
            .is_empty()
}

fn normalized_base_url(value: Option<&str>) -> Result<Option<String>, CommandErrorPayload> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(invalid_payload(
            "baseUrl must start with http:// or https://".to_owned(),
        ));
    }
    if value.contains('?') || value.contains('#') {
        return Err(invalid_payload(
            "baseUrl must not include query parameters or fragments".to_owned(),
        ));
    }
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| invalid_payload("baseUrl must be a valid http(s) URL".to_owned()))?;
    if parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid_payload(
            "baseUrl must not include credentials".to_owned(),
        ));
    }
    if parsed.scheme() == "http" && !url_targets_loopback(&parsed) {
        return Err(invalid_payload(
            "baseUrl must use https:// unless it targets localhost".to_owned(),
        ));
    }
    Ok(Some(value.trim_end_matches('/').to_owned()))
}

fn url_targets_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn model_from_provider_settings(
    store: &dyn ProviderSettingsStore,
    selected_config_id: Option<&str>,
) -> Result<Option<(Arc<dyn ModelProvider>, String, ModelProtocol)>, CommandErrorPayload> {
    let Some(record) = store.load_record()? else {
        return Ok(None);
    };
    let Some(config_id) = selected_config_id.or(record.default_config_id.as_deref()) else {
        return Ok(None);
    };
    let Some(config) = record.configs.iter().find(|config| config.id == config_id) else {
        return Err(runtime_init_failed("provider config is missing".to_owned()));
    };
    let descriptor = provider_config_descriptor(config)?;
    let api_key = config.api_key.trim();
    if api_key.is_empty() {
        return Err(runtime_init_failed(
            "provider config has no api key".to_owned(),
        ));
    }
    let base_url = normalized_base_url(config.base_url.as_deref())?;
    let provider = build_provider(ProviderBuildConfig {
        provider_id: config.provider_id.clone(),
        api_key: api_key.to_owned(),
        base_url,
        model_descriptor: Some(descriptor.clone()),
    })
    .map_err(provider_registry_init_error)?;
    let protocol = descriptor.protocol;

    Ok(Some((
        Arc::from(provider),
        config.model_id.clone(),
        protocol,
    )))
}

fn parse_request_id(value: &str) -> Result<RequestId, CommandErrorPayload> {
    let request_id = RequestId::parse(value).map_err(|error| {
        invalid_payload(format!(
            "requestId must be a valid permission request id: {error}"
        ))
    })?;

    if request_id.to_string() != value {
        return Err(invalid_payload(
            "requestId must be a canonical permission request id".to_owned(),
        ));
    }

    Ok(request_id)
}

fn parse_session_id(value: &str) -> Result<SessionId, CommandErrorPayload> {
    let session_id = SessionId::parse(value).map_err(|error| {
        invalid_payload(format!(
            "conversationId must be a valid conversation session id: {error}"
        ))
    })?;

    if session_id.to_string() != value {
        return Err(invalid_payload(
            "conversationId must be a canonical conversation session id".to_owned(),
        ));
    }

    Ok(session_id)
}

fn validate_client_message_id(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("clientMessageId", value)?;
    if is_uuid_v4_like(value) {
        return Ok(());
    }

    Err(invalid_payload(
        "clientMessageId must be a UUID generated by the desktop client".to_owned(),
    ))
}

fn is_uuid_v4_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }

    for index in [8, 13, 18, 23] {
        if bytes[index] != b'-' {
            return false;
        }
    }
    if bytes[14] != b'4' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
        return false;
    }

    bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 8 | 13 | 18 | 23))
        .all(|(_, byte)| byte.is_ascii_hexdigit())
}

fn to_harness_decision(decision: PermissionDecision) -> Decision {
    match decision {
        PermissionDecision::Approve => Decision::AllowOnce,
        PermissionDecision::Deny => Decision::DenyOnce,
    }
}

fn run_event_payload_from_read_model(
    event: harness_contracts::ConversationTimelineEvent,
) -> Result<RunEventPayload, CommandErrorPayload> {
    Ok(RunEventPayload {
        id: event.id,
        conversation_sequence: event.cursor.conversation_sequence,
        payload: event.payload,
        run_id: event.run_id,
        sequence: event.sequence,
        source: run_event_source_label(&event.source)?,
        timestamp: event.timestamp.to_rfc3339(),
        event_type: run_event_type_label(&event.event_type)?,
        visibility: run_event_visibility_label(&event.visibility)?,
    })
}

fn run_event_source_label(value: &str) -> Result<&'static str, CommandErrorPayload> {
    match value {
        "user" => Ok("user"),
        "assistant" => Ok("assistant"),
        "tool" => Ok("tool"),
        "engine" => Ok("engine"),
        "policy" => Ok("policy"),
        _ => Err(runtime_operation_failed(
            "conversation timeline source is invalid".to_owned(),
        )),
    }
}

fn run_event_visibility_label(value: &str) -> Result<&'static str, CommandErrorPayload> {
    match value {
        "public" => Ok("public"),
        "redacted" => Ok("redacted"),
        "withheld" => Ok("withheld"),
        _ => Err(runtime_operation_failed(
            "conversation timeline visibility is invalid".to_owned(),
        )),
    }
}

fn run_event_type_label(value: &str) -> Result<&'static str, CommandErrorPayload> {
    match value {
        "run.started" => Ok("run.started"),
        "run.ended" => Ok("run.ended"),
        "user.message.appended" => Ok("user.message.appended"),
        "assistant.delta" => Ok("assistant.delta"),
        "assistant.thinking.delta" => Ok("assistant.thinking.delta"),
        "assistant.completed" => Ok("assistant.completed"),
        "assistant.review.requested" => Ok("assistant.review.requested"),
        "assistant.clarification.requested" => Ok("assistant.clarification.requested"),
        "assistant.notice" => Ok("assistant.notice"),
        "tool.requested" => Ok("tool.requested"),
        "tool.approved" => Ok("tool.approved"),
        "tool.denied" => Ok("tool.denied"),
        "tool.completed" => Ok("tool.completed"),
        "tool.failed" => Ok("tool.failed"),
        "permission.requested" => Ok("permission.requested"),
        "permission.resolved" => Ok("permission.resolved"),
        "artifact.created" => Ok("artifact.created"),
        "artifact.updated" => Ok("artifact.updated"),
        "engine.failed" => Ok("engine.failed"),
        _ => Err(runtime_operation_failed(
            "conversation timeline event type is invalid".to_owned(),
        )),
    }
}

fn permission_requested_run_event(
    event_id: String,
    event: &Event,
    sequence: u64,
    redactor: &dyn Redactor,
) -> RunEventPayload {
    let Event::PermissionRequested(event) = event else {
        unreachable!("permission activity must be built from PermissionRequested events");
    };
    let subject = permission_subject_display(&event.subject, redactor);
    let reason = if event.auto_resolved {
        "已按本次授权模式自动允许。"
    } else {
        "需要批准后才能继续。"
    };

    RunEventPayload {
        id: event_id,
        conversation_sequence: sequence,
        payload: serde_json::to_value(PermissionRequestedRunEventPayload {
            auto_resolved: event.auto_resolved,
            decision_scope: decision_scope_display(&event.scope_hint, redactor),
            exposure: subject.exposure,
            operation: subject.operation,
            reason: reason.to_owned(),
            request_id: event.request_id.to_string(),
            severity: severity_display(event.severity),
            target: subject.target,
            tool_use_id: event.tool_use_id.to_string(),
            workspace_boundary: "current workspace".to_owned(),
        })
        .unwrap_or_else(|_| json!({})),
        run_id: event.run_id.to_string(),
        sequence,
        source: "policy",
        timestamp: event.at.to_rfc3339(),
        event_type: "permission.requested",
        visibility: "public",
    }
}

struct PermissionSubjectDisplay {
    exposure: String,
    operation: String,
    target: String,
}

fn permission_subject_display(
    subject: &PermissionSubject,
    redactor: &dyn Redactor,
) -> PermissionSubjectDisplay {
    match subject {
        PermissionSubject::CommandExec { command, .. } => PermissionSubjectDisplay {
            exposure: "Can execute a command inside the workspace boundary.".to_owned(),
            operation: "Execute command".to_owned(),
            target: safe_command_label(command, redactor),
        },
        PermissionSubject::ToolInvocation { tool, .. } => PermissionSubjectDisplay {
            exposure: "Can invoke a runtime tool.".to_owned(),
            operation: "Use tool".to_owned(),
            target: public_text_display(tool.clone(), redactor),
        },
        PermissionSubject::FileWrite { path, .. } => PermissionSubjectDisplay {
            exposure: "Can write a file in the workspace.".to_owned(),
            operation: "Write file".to_owned(),
            target: safe_path_label(path, redactor),
        },
        PermissionSubject::FileDelete { path } => PermissionSubjectDisplay {
            exposure: "Can delete a file in the workspace.".to_owned(),
            operation: "Delete file".to_owned(),
            target: safe_path_label(path, redactor),
        },
        PermissionSubject::NetworkAccess { host, port } => PermissionSubjectDisplay {
            exposure: "Can access a network endpoint.".to_owned(),
            operation: "Access network".to_owned(),
            target: public_text_display(
                port.map_or_else(|| host.clone(), |port| format!("{host}:{port}")),
                redactor,
            ),
        },
        PermissionSubject::DangerousCommand { command, .. } => PermissionSubjectDisplay {
            exposure: "Can execute a dangerous command.".to_owned(),
            operation: "Execute dangerous command".to_owned(),
            target: safe_command_label(command, redactor),
        },
        PermissionSubject::McpToolCall { server, tool, .. } => PermissionSubjectDisplay {
            exposure: "Can invoke an MCP tool.".to_owned(),
            operation: "Use MCP tool".to_owned(),
            target: public_text_display(format!("{server}/{tool}"), redactor),
        },
        PermissionSubject::Custom { kind, .. } => PermissionSubjectDisplay {
            exposure: "Can perform a custom permission-gated operation.".to_owned(),
            operation: "Review custom operation".to_owned(),
            target: public_text_display(kind.clone(), redactor),
        },
        _ => PermissionSubjectDisplay {
            exposure: "Can continue a permission-gated operation.".to_owned(),
            operation: "Review permission".to_owned(),
            target: "runtime operation".to_owned(),
        },
    }
}

fn decision_scope_display(scope: &DecisionScope, redactor: &dyn Redactor) -> String {
    match scope {
        DecisionScope::ExactCommand { .. } => "this exact command".to_owned(),
        DecisionScope::ExactArgs(_) => "these exact command arguments".to_owned(),
        DecisionScope::ToolName(_) => "this tool".to_owned(),
        DecisionScope::Category(_) => "this tool category".to_owned(),
        DecisionScope::PathPrefix(_) => "this workspace path scope".to_owned(),
        DecisionScope::GlobPattern(_) => "this workspace glob".to_owned(),
        DecisionScope::ExecuteCodeScript { .. } => "execute code script".to_owned(),
        DecisionScope::Any => "any matching operation".to_owned(),
        _ => public_text_display("current operation".to_owned(), redactor),
    }
}

fn safe_command_label(command: &str, redactor: &dyn Redactor) -> String {
    let executable_token = command.split_whitespace().next().unwrap_or(command);
    let executable = Path::new(executable_token)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(executable_token);
    public_text_display(executable.to_owned(), redactor)
}

fn safe_path_label(path: &Path, redactor: &dyn Redactor) -> String {
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(
            || "workspace file".to_owned(),
            |name| format!("workspace file: {name}"),
        );
    public_text_display(label, redactor)
}

async fn read_replay_run_events(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    read_replay_run_events_after(request, state, None).await
}

async fn read_replay_run_events_after(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
    after_cursor: Option<String>,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    let session_id = match request.conversation_id.as_deref() {
        Some(conversation_id) => parse_session_id(conversation_id)?,
        None => state.default_conversation_id(),
    };
    let run_id = request.run_id.as_deref().map(parse_run_id).transpose()?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading replay events requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut mapper = RunEventMapper::default();
    let mut after_event_id = after_cursor
        .as_deref()
        .map(EventId::parse)
        .transpose()
        .map_err(|error| invalid_payload(format!("conversation cursor is invalid: {error}")))?;
    let mut conversation_sequence = 0;
    let mut run_sequences: HashMap<String, u64> = HashMap::new();
    if let Some(cursor_event_id) = after_event_id {
        let seed = seed_run_event_mapper_until_cursor(
            &harness,
            state.conversation_session_options(session_id),
            session_id,
            cursor_event_id,
            &mut mapper,
            &redactor,
        )
        .await?;
        conversation_sequence = seed.conversation_sequence;
        run_sequences = seed.run_sequences;
    }
    let mut events = Vec::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| runtime_operation_failed(format!("replay read failed: {error}")))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            let Some(event) = mapper.map(
                envelope.event_id.to_string(),
                envelope.payload,
                session_id,
                &redactor,
            ) else {
                continue;
            };
            if run_id
                .as_ref()
                .is_some_and(|run_id| event.run_id != run_id.to_string())
            {
                continue;
            }
            let event_conversation_sequence = conversation_sequence + 1;
            conversation_sequence += 1;
            let run_sequence = run_sequences.entry(event.run_id.clone()).or_insert(0);
            events.push(RunEventPayload {
                conversation_sequence: event_conversation_sequence,
                sequence: *run_sequence + 1,
                ..event
            });
            *run_sequence += 1;
        }

        after_event_id = page.next_event_id;
    }

    Ok(events)
}

struct RunEventMapperSeed {
    conversation_sequence: u64,
    run_sequences: HashMap<String, u64>,
}

async fn seed_run_event_mapper_until_cursor(
    harness: &Harness,
    options: SessionOptions,
    session_id: SessionId,
    cursor_event_id: EventId,
    mapper: &mut RunEventMapper,
    redactor: &dyn Redactor,
) -> Result<RunEventMapperSeed, CommandErrorPayload> {
    let mut after_event_id = None;
    let mut conversation_sequence = 0;
    let mut run_sequences: HashMap<String, u64> = HashMap::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: options.clone(),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| runtime_operation_failed(format!("replay read failed: {error}")))?;
        if page.events.is_empty() {
            return Err(invalid_payload("conversation cursor is unknown".to_owned()));
        }

        for envelope in page.events {
            let event_id = envelope.event_id;
            if let Some(event) =
                mapper.map(event_id.to_string(), envelope.payload, session_id, redactor)
            {
                conversation_sequence += 1;
                *run_sequences.entry(event.run_id).or_insert(0) += 1;
            }
            if event_id == cursor_event_id {
                return Ok(RunEventMapperSeed {
                    conversation_sequence,
                    run_sequences,
                });
            }
            after_event_id = Some(event_id);
        }
    }
}

async fn read_activity_replay_events(
    request: &ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    read_replay_run_events(request.clone(), state).await
}

fn message_content_display(content: &MessageContent, redactor: &dyn Redactor) -> String {
    let value = match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Structured(value) => value.to_string(),
        MessageContent::Multimodal(parts) => parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };

    public_text_display(value, redactor)
}

fn redact_private_absolute_paths(value: String) -> String {
    redact_unsafe_display_text(&value)
}

fn public_text_display(value: String, redactor: &dyn Redactor) -> String {
    redact_unsafe_display_text(&redacted_display(value, redactor))
}

fn public_ui_safe_text_display(value: &UiSafeText, redactor: &dyn Redactor) -> String {
    redact_unsafe_display_text(
        &UiSafeText::from_redacted_display(value.as_str(), redactor).into_string(),
    )
}

fn redact_unsafe_display_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut index = 0;

    while index < value.len() {
        if unsafe_url_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_url_token_end(value, index);
            continue;
        }
        if local_unsafe_path_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_token_end(value, index);
            continue;
        }

        let ch = value[index..]
            .chars()
            .next()
            .expect("index is within string bounds");
        output.push(ch);
        index += ch.len_utf8();
    }

    output
}

fn token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_alphanumeric() && ch != '_'))
}

fn unsafe_url_starts_at(value: &str, index: usize) -> bool {
    if unsafe_opaque_url_starts_at(value, index) {
        return true;
    }

    let tail = &value[index..];
    let Some(separator) = tail.find("://") else {
        return false;
    };
    if separator == 0 {
        return false;
    }
    tail[..separator]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn unsafe_opaque_url_starts_at(value: &str, index: usize) -> bool {
    const SCHEMES: &[&str] = &["blob:", "data:", "file:", "javascript:", "mailto:"];
    let tail = &value[index..];
    ascii_token_starts_at(value, index)
        && SCHEMES.iter().any(|scheme| {
            tail.get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

fn ascii_token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_ascii_alphanumeric() && ch != '_'))
}

fn local_unsafe_path_starts_at(value: &str, index: usize) -> bool {
    let tail = &value[index..];
    if tail.starts_with("~/")
        || tail.starts_with("~\\")
        || starts_with_jyowo_path(tail)
        || starts_with_known_unix_absolute_root(tail)
    {
        return true;
    }
    token_starts_at(value, index) && (tail.starts_with('/') || is_windows_absolute_path_start(tail))
}

fn starts_with_jyowo_path(value: &str) -> bool {
    value
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(".jyowo"))
        && value
            .as_bytes()
            .get(6)
            .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
}

fn is_windows_absolute_path_start(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn starts_with_known_unix_absolute_root(value: &str) -> bool {
    const ROOTS: &[&str] = &[
        "/Applications",
        "/Library",
        "/System",
        "/Users",
        "/Volumes",
        "/dev",
        "/etc",
        "/home",
        "/media",
        "/mnt",
        "/opt",
        "/private",
        "/root",
        "/run",
        "/tmp",
        "/usr",
        "/var",
    ];

    ROOTS.iter().any(|root| {
        value
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
    })
}

fn unsafe_url_token_end(value: &str, start: usize) -> usize {
    if starts_with_unsafe_opaque_scheme(value, start, "data:")
        || starts_with_unsafe_opaque_scheme(value, start, "javascript:")
    {
        return unsafe_data_url_token_end(value, start);
    }

    unsafe_token_end(value, start)
}

fn starts_with_unsafe_opaque_scheme(value: &str, start: usize, scheme: &str) -> bool {
    ascii_token_starts_at(value, start)
        && value[start..]
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

fn unsafe_data_url_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '，'
                    | '。'
                    | '；'
                    | '、'
                    | '）'
                    | '】'
                    | '」'
                    | '》'
                    | '！'
                    | '？'
            ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

fn unsafe_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | '`'
                        | ')'
                        | ']'
                        | '}'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                        | '，'
                        | '。'
                        | '；'
                        | '、'
                        | '）'
                        | '】'
                        | '」'
                        | '》'
                        | '！'
                        | '？'
                ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

fn truncate_utf8(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn context_decisions_from_pending_requests(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    run_id: Option<&RunId>,
    redactor: &dyn Redactor,
) -> Vec<ContextDecisionPayload> {
    let mut pending_requests = state.pending_permission_requests();
    pending_requests.sort_by_key(|pending| {
        (
            pending.request.created_at,
            pending.request.request_id.to_string(),
        )
    });

    pending_requests
        .into_iter()
        .filter(|pending| {
            pending.request.session_id == session_id
                && run_id.is_none_or(|run_id| pending.context.run_id == Some(*run_id))
        })
        .map(|pending| ContextDecisionPayload {
            detail: format!(
                "{} permission is waiting for decision {}.",
                severity_display(pending.request.severity),
                pending.request.request_id
            ),
            request_id: Some(pending.request.request_id.to_string()),
            title: format!(
                "Approve {}",
                public_text_display(pending.request.tool_name, redactor)
            ),
        })
        .collect()
}

fn context_files_from_workspace(workspace_root: &Path) -> Vec<ContextFilePayload> {
    [
        "apps/desktop/src/main.tsx",
        "apps/desktop/src/routes/index.tsx",
        "apps/desktop/src/shared/tauri/commands.ts",
        "apps/desktop/src-tauri/src/commands.rs",
        "docs/plans/2026-06-17-conversation-workspace-implementation.md",
    ]
    .into_iter()
    .filter_map(|label| {
        workspace_root
            .join(label)
            .is_file()
            .then(|| ContextFilePayload {
                label: label.to_owned(),
                state: Some("ready"),
            })
    })
    .take(5)
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentRecord {
    attachment: AttachmentReferencePayload,
    blob_ref: BlobRef,
}

fn canonicalize_existing_file(
    path: &Path,
    field: &'static str,
) -> Result<PathBuf, CommandErrorPayload> {
    path.canonicalize()
        .map_err(|error| invalid_payload(format!("{field} is invalid: {error}")))
}

fn workspace_relative_path(path: &Path, workspace_root: &Path) -> Option<String> {
    let workspace_root = workspace_root.canonicalize().ok()?;
    path.strip_prefix(workspace_root)
        .ok()
        .map(path_to_workspace_string)
}

fn path_to_workspace_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn attachment_id(path: &Path, size_bytes: u64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(&size_bytes.to_le_bytes());
    if let Ok(metadata) = path.metadata() {
        if let Ok(modified_at) = metadata.modified() {
            if let Ok(duration) = modified_at.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(&duration.as_nanos().to_le_bytes());
            }
        }
    }
    format!("attachment-{}", hasher.finalize().to_hex())
}

fn attachment_record_path(workspace_root: &Path, attachment_id: &str) -> PathBuf {
    workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("attachments")
        .join("records")
        .join(format!("{attachment_id}.json"))
}

fn write_attachment_record(
    workspace_root: &Path,
    record: &AttachmentRecord,
) -> Result<(), CommandErrorPayload> {
    let path = attachment_record_path(workspace_root, &record.attachment.id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("attachment record store unavailable: {error}"))
        })?;
    }
    let content = serde_json::to_vec_pretty(record)
        .map_err(|error| runtime_operation_failed(format!("attachment record failed: {error}")))?;
    std::fs::write(path, content).map_err(|error| {
        runtime_operation_failed(format!("attachment record write failed: {error}"))
    })
}

fn read_attachment_record(
    workspace_root: &Path,
    attachment_id: &str,
) -> Result<AttachmentRecord, CommandErrorPayload> {
    ensure_attachment_id(attachment_id)?;
    let path = attachment_record_path(workspace_root, attachment_id);
    let content = std::fs::read_to_string(path)
        .map_err(|_| invalid_payload("attachment reference does not exist".to_owned()))?;
    serde_json::from_str(&content)
        .map_err(|_| invalid_payload("attachment record is invalid".to_owned()))
}

fn infer_mime_type(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "css" => "text/css",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "rs" | "tsx" | "ts" | "js" | "jsx" | "txt" | "toml" | "yaml" | "yml" => "text/plain",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn workspace_project_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Local workspace")
        .to_owned()
}

#[derive(Default)]
struct RunEventMapper {
    allowed_run_ids: HashSet<RunId>,
    permission_run_ids: HashMap<RequestId, RunId>,
    tool_run_ids: HashMap<ToolUseId, RunId>,
}

impl RunEventMapper {
    fn is_allowed_run(&self, run_id: &RunId) -> bool {
        self.allowed_run_ids.contains(run_id)
    }

    fn map(
        &mut self,
        event_id: String,
        event: Event,
        requested_session_id: SessionId,
        redactor: &dyn Redactor,
    ) -> Option<RunEventPayload> {
        match event {
            Event::RunStarted(event) => {
                if event.session_id != requested_session_id {
                    return None;
                }

                self.allowed_run_ids.insert(event.run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "permissionMode": event.permission_mode,
                        "sessionId": event.session_id.to_string(),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.started_at.to_rfc3339(),
                    event_type: "run.started",
                    visibility: "public",
                })
            }
            Event::RunEnded(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({ "reason": run_end_reason_display(&event.reason, redactor) });
                if let Some(usage) = event.usage {
                    payload["usage"] = json!({
                        "cacheReadTokens": usage.cache_read_tokens,
                        "cacheWriteTokens": usage.cache_write_tokens,
                        "costMicros": usage.cost_micros,
                        "inputTokens": usage.input_tokens,
                        "outputTokens": usage.output_tokens,
                        "toolCalls": usage.tool_calls,
                    });
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.ended_at.to_rfc3339(),
                    event_type: "run.ended",
                    visibility: "public",
                })
            }
            Event::UserMessageAppended(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({
                    "messageId": event.message_id.to_string(),
                    "body": message_content_display(&event.content, redactor),
                });
                if let Some(client_message_id) = event
                    .metadata
                    .labels
                    .get("clientMessageId")
                    .filter(|client_message_id| is_uuid_v4_like(client_message_id))
                {
                    payload["clientMessageId"] = json!(client_message_id);
                }
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "user",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "user.message.appended",
                    visibility: "public",
                })
            }
            Event::AssistantDeltaProduced(event) => match event.delta {
                DeltaChunk::Text(text) => {
                    if !self.is_allowed_run(&event.run_id) {
                        return None;
                    }

                    Some(RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "messageId": event.message_id.to_string(),
                            "text": public_text_display(text, redactor),
                        }),
                        run_id: event.run_id.to_string(),
                        sequence: 0,
                        source: "assistant",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "assistant.delta",
                        visibility: "public",
                    })
                }
                DeltaChunk::Thought(_) => {
                    if !self.is_allowed_run(&event.run_id) {
                        return None;
                    }

                    Some(RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "status": "running",
                        }),
                        run_id: event.run_id.to_string(),
                        sequence: 0,
                        source: "assistant",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "assistant.thinking.delta",
                        visibility: "public",
                    })
                }
                DeltaChunk::ReasoningSummary(summary) => {
                    if !self.is_allowed_run(&event.run_id) {
                        return None;
                    }

                    Some(RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "safeSummaryDelta": public_text_display(summary.text, redactor),
                            "status": "running",
                        }),
                        run_id: event.run_id.to_string(),
                        sequence: 0,
                        source: "assistant",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "assistant.thinking.delta",
                        visibility: "public",
                    })
                }
                DeltaChunk::ToolUseStart { .. }
                | DeltaChunk::ToolUseInputDelta { .. }
                | DeltaChunk::ToolUseEnd { .. } => None,
                _ => None,
            },
            Event::AssistantMessageCompleted(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "messageId": event.message_id.to_string(),
                        "body": message_content_display(&event.content, redactor),
                        "toolUses": event.tool_uses.iter().map(|tool_use| {
                            json!({
                                "toolUseId": tool_use.tool_use_id.to_string(),
                                "toolName": public_text_display(tool_use.tool_name.clone(), redactor),
                            })
                        }).collect::<Vec<_>>(),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.completed",
                    visibility: "public",
                })
            }
            Event::ArtifactCreated(event) => {
                if event.session_id != requested_session_id || !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let artifact_kind = event.kind;
                let mut payload = json!({
                    "artifactId": event.artifact_id,
                    "kind": public_text_display(artifact_kind.clone(), redactor),
                    "status": artifact_status_label(event.status),
                    "source": artifact_source_label(event.source),
                    "title": public_text_display(event.title, redactor),
                });
                if let Some(preview) = event.preview {
                    payload["summary"] = json!(public_text_display(preview, redactor));
                }
                if let Some(media) = artifact_media_payload(event.blob_ref.as_ref(), &artifact_kind) {
                    payload["media"] = media;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "artifact.created",
                    visibility: "public",
                })
            }
            Event::ArtifactUpdated(event) => {
                if event.session_id != requested_session_id || !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({ "artifactId": event.artifact_id });
                payload["source"] = json!(artifact_source_label(event.source));
                if let Some(title) = event.title.as_ref() {
                    payload["title"] = json!(public_text_display(title.clone(), redactor));
                }
                if let Some(kind) = event.kind.as_ref() {
                    payload["kind"] = json!(public_text_display(kind.clone(), redactor));
                    if let Some(media) = artifact_media_payload(event.blob_ref.as_ref(), kind) {
                        payload["media"] = media;
                    }
                }
                if let Some(status) = event.status {
                    payload["status"] = json!(artifact_status_label(status));
                }
                if let Some(preview) = event.preview.as_ref() {
                    payload["summary"] = json!(public_text_display(preview.clone(), redactor));
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "artifact.updated",
                    visibility: "public",
                })
            }
            Event::AssistantReviewRequested(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({
                    "requestId": event.request_id.to_string(),
                    "title": public_ui_safe_text_display(&event.title, redactor),
                });
                if let Some(body) = event.body.as_ref() {
                    payload["body"] = json!(public_ui_safe_text_display(body, redactor));
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.review.requested",
                    visibility: "public",
                })
            }
            Event::AssistantClarificationRequested(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "requestId": event.request_id.to_string(),
                        "prompt": public_ui_safe_text_display(&event.prompt, redactor),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.clarification.requested",
                    visibility: "public",
                })
            }
            Event::AssistantNotice(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "noticeId": event.notice_id.to_string(),
                        "body": public_ui_safe_text_display(&event.body, redactor),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.notice",
                    visibility: "public",
                })
            }
            Event::ToolUseRequested(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                self.tool_run_ids.insert(event.tool_use_id, event.run_id);
                let mut payload = json!({
                    "argumentsSummary": "Input withheld from conversation timeline.",
                    "toolName": public_text_display(event.tool_name.clone(), redactor),
                    "toolUseId": event.tool_use_id.to_string(),
                });
                if let Some(command) =
                    safe_tool_command_preview(&event.tool_name, &event.input, redactor)
                {
                    payload["command"] = json!(command);
                }
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.requested",
                    visibility: "redacted",
                })
            }
            Event::ToolUseApproved(event) => self.tool_run_ids.get(&event.tool_use_id).map(|run_id| {
                RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "toolUseId": event.tool_use_id.to_string() }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.approved",
                    visibility: "public",
                }
            }),
            Event::ToolUseDenied(event) => self.tool_run_ids.get(&event.tool_use_id).map(|run_id| {
                RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "toolUseId": event.tool_use_id.to_string() }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.denied",
                    visibility: "public",
                }
            }),
            Event::ToolUseCompleted(event) => {
                self.tool_run_ids.get(&event.tool_use_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "durationMs": event.duration_ms,
                        "outputSummary": tool_result_summary(event.result),
                        "toolUseId": event.tool_use_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.completed",
                    visibility: "redacted",
                })
            }
            Event::ToolUseFailed(event) => self.tool_run_ids.get(&event.tool_use_id).map(|run_id| {
                RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "code": "tool_error",
                        "message": "Tool error withheld from conversation timeline.",
                        "toolUseId": event.tool_use_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.failed",
                    visibility: "redacted",
                }
            }),
            Event::PermissionRequested(event) => {
                if event.session_id != requested_session_id || !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                self.permission_run_ids.insert(event.request_id, event.run_id);
                Some(permission_requested_run_event(
                    event_id,
                    &Event::PermissionRequested(event),
                    0,
                    redactor,
                ))
            }
            Event::PermissionResolved(event) => self
                .permission_run_ids
                .get(&event.request_id)
                .map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "decision": permission_decision_payload(event.decision),
                        "requestId": event.request_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "policy",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "permission.resolved",
                    visibility: "public",
                }),
            Event::PluginLoaded(event) => Some(RunEventPayload {
                id: event_id,
                conversation_sequence: 0,
                payload: json!({
                    "capabilityCount": plugin_capability_count(&event.capabilities),
                    "pluginId": public_text_display(event.plugin_id.0, redactor),
                    "pluginName": public_text_display(event.plugin_name, redactor),
                    "trustLevel": plugin_trust_level_payload(event.trust_level),
                }),
                run_id: PLUGIN_RUNTIME_RUN_ID.to_owned(),
                sequence: 0,
                source: "plugin",
                timestamp: event.at.to_rfc3339(),
                event_type: "plugin.loaded",
                visibility: "redacted",
            }),
            Event::PluginRejected(event) => Some(RunEventPayload {
                id: event_id,
                conversation_sequence: 0,
                payload: json!({
                    "pluginId": public_text_display(event.plugin_id.0, redactor),
                    "pluginName": public_text_display(event.plugin_name, redactor),
                    "reason": plugin_rejection_reason_payload(&event.reason),
                    "trustLevel": plugin_trust_level_payload(event.trust_level),
                }),
                run_id: PLUGIN_RUNTIME_RUN_ID.to_owned(),
                sequence: 0,
                source: "plugin",
                timestamp: event.at.to_rfc3339(),
                event_type: "plugin.rejected",
                visibility: "redacted",
            }),
            Event::PluginFailed(event) => Some(RunEventPayload {
                id: event_id,
                conversation_sequence: 0,
                payload: json!({
                    "message": PLUGIN_FAILURE_WITHHELD_MESSAGE,
                    "pluginId": public_text_display(event.plugin_id.0, redactor),
                    "pluginName": public_text_display(event.plugin_name, redactor),
                    "trustLevel": plugin_trust_level_payload(event.trust_level),
                }),
                run_id: PLUGIN_RUNTIME_RUN_ID.to_owned(),
                sequence: 0,
                source: "plugin",
                timestamp: event.at.to_rfc3339(),
                event_type: "plugin.failed",
                visibility: "redacted",
            }),
            Event::EngineFailed(event) => event.run_id.and_then(|run_id| {
                self.is_allowed_run(&run_id).then(|| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "message": "Engine error withheld from conversation timeline." }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "engine.failed",
                    visibility: "redacted",
                })
            }),
            _ => None,
        }
    }
}

fn tool_result_summary(_result: impl Serialize) -> String {
    "Output withheld from conversation timeline.".to_owned()
}

fn plugin_capability_count(capabilities: &PluginCapabilitiesSummary) -> u64 {
    u64::from(capabilities.tools)
        + u64::from(capabilities.hooks)
        + u64::from(capabilities.mcp_servers)
        + u64::from(capabilities.skills)
        + if capabilities.steering { 1 } else { 0 }
        + if capabilities.memory_provider { 1 } else { 0 }
        + if capabilities.coordinator { 1 } else { 0 }
}

fn plugin_trust_level_payload(trust_level: TrustLevel) -> &'static str {
    match trust_level {
        TrustLevel::AdminTrusted => "admin_trusted",
        TrustLevel::UserControlled => "user_controlled",
        _ => "user_controlled",
    }
}

fn plugin_rejection_reason_payload(reason: &RejectionReason) -> &'static str {
    match reason {
        RejectionReason::SignatureInvalid { .. } => "SignatureInvalid",
        RejectionReason::UnknownSigner { .. } => "UnknownSigner",
        RejectionReason::SignerRevoked { .. } => "SignerRevoked",
        RejectionReason::TrustMismatch { .. } => "TrustMismatch",
        RejectionReason::NamespaceConflict { .. } => "NamespaceConflict",
        RejectionReason::DependencyUnsatisfied { .. } => "DependencyUnsatisfied",
        RejectionReason::DependencyCycle { .. } => "DependencyCycle",
        RejectionReason::HarnessVersionIncompatible { .. } => "HarnessVersionIncompatible",
        RejectionReason::SlotOccupied { .. } => "SlotOccupied",
        RejectionReason::AdmissionDenied { .. } => "AdmissionDenied",
        _ => "AdmissionDenied",
    }
}

fn safe_tool_command_preview(
    tool_name: &str,
    input: &Value,
    redactor: &dyn Redactor,
) -> Option<String> {
    if !is_command_tool_name(tool_name) {
        return None;
    }
    let command = input.get("command").and_then(Value::as_str)?.trim();
    if command.is_empty() || contains_obvious_secret(command) {
        return None;
    }
    Some(truncate_utf8(
        redact_private_absolute_paths(redacted_display(command.to_owned(), redactor)),
        1_200,
    ))
}

fn is_command_tool_name(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized == "bash" || normalized.contains("shell")
}

fn contains_obvious_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("xoxb-")
}

fn permission_decision_payload(decision: Decision) -> &'static str {
    match decision {
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => "approve",
        Decision::DenyOnce | Decision::DenyPermanent | Decision::Escalate => "deny",
        _ => "deny",
    }
}

fn run_end_reason_display(reason: &EndReason, redactor: &dyn Redactor) -> String {
    if matches!(reason, EndReason::Error(_)) {
        return "Run error withheld from conversation timeline.".to_owned();
    }

    let value = match reason {
        EndReason::Completed => "completed".to_owned(),
        EndReason::MaxIterationsReached => "max iterations reached".to_owned(),
        EndReason::TokenBudgetExhausted => "token budget exhausted".to_owned(),
        EndReason::BudgetExhausted(_) => "budget exhausted".to_owned(),
        EndReason::Interrupted => "interrupted".to_owned(),
        EndReason::Cancelled { .. } => "cancelled".to_owned(),
        EndReason::Error(_) => unreachable!("error reasons return before redaction"),
        EndReason::Compacted => "compacted".to_owned(),
        _ => "ended".to_owned(),
    };

    let value = redacted_display(value, redactor);
    if value.trim().is_empty() {
        "error".to_owned()
    } else {
        value
    }
}

fn parse_run_id(value: &str) -> Result<RunId, CommandErrorPayload> {
    ensure_non_empty("runId", value)?;
    let run_id = RunId::parse(value)
        .map_err(|_| invalid_payload("runId must be a valid run id".to_owned()))?;

    if run_id.to_string() != value {
        return Err(invalid_payload(
            "runId must be a canonical run id".to_owned(),
        ));
    }

    Ok(run_id)
}

fn severity_display(severity: Severity) -> &'static str {
    match severity {
        Severity::Info | Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
        _ => "medium",
    }
}

fn redacted_display(value: String, redactor: &dyn Redactor) -> String {
    redactor.redact(
        &value,
        &RedactRules {
            scope: RedactScope::EventBody,
            replacement: "[REDACTED]".to_owned(),
            pattern_set: RedactPatternSet::Default,
        },
    )
}

fn invalid_payload(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "INVALID_PAYLOAD",
        message,
    }
}

fn runtime_unavailable(message: &str) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_UNAVAILABLE",
        message: message.to_owned(),
    }
}

fn runtime_init_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_INIT_FAILED",
        message,
    }
}

fn runtime_operation_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message,
    }
}

fn not_found(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "NOT_FOUND",
        message,
    }
}

fn conversation_read_error(error: impl std::fmt::Display) -> CommandErrorPayload {
    let message = error.to_string();
    if message.contains("session not found") {
        return not_found(message);
    }
    runtime_operation_failed(format!("conversation read failed: {message}"))
}

fn memory_operation_failed(message: &'static str) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message: message.to_owned(),
    }
}

fn support_bundle_operation_failed() -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message: "Support bundle export could not be prepared.".to_owned(),
    }
}

fn support_bundle_read_error(error: CommandErrorPayload) -> CommandErrorPayload {
    if error.code == "INVALID_PAYLOAD" {
        return error;
    }

    support_bundle_operation_failed()
}

fn write_memory_export_file(path: &Path, content: &str) -> Result<(), CommandErrorPayload> {
    let Some(parent) = path.parent() else {
        return Err(memory_operation_failed(
            "Memory export could not be prepared.",
        ));
    };
    ensure_no_symlink_components(parent, "memory export directory")
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    std::fs::create_dir_all(parent)
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    std::fs::write(path, content)
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))
}

fn write_support_bundle_file(path: &Path, content: &str) -> Result<(), CommandErrorPayload> {
    let Some(parent) = path.parent() else {
        return Err(support_bundle_operation_failed());
    };
    ensure_no_symlink_components(parent, "support bundle export directory")
        .map_err(|_| support_bundle_operation_failed())?;
    std::fs::create_dir_all(parent).map_err(|_| support_bundle_operation_failed())?;
    ensure_no_symlink_components(parent, "support bundle export directory")
        .map_err(|_| support_bundle_operation_failed())?;
    ensure_no_symlink_components(path, "support bundle export file")
        .map_err(|_| support_bundle_operation_failed())?;

    let temp_path = path.with_file_name(format!(
        "{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("support-bundle"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "support bundle export temp file")
        .map_err(|_| support_bundle_operation_failed())?;

    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|_| support_bundle_operation_failed())?;
    if temp_file.write_all(content.as_bytes()).is_err() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(support_bundle_operation_failed());
    }
    if temp_file.sync_all().is_err() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(support_bundle_operation_failed());
    }
    drop(temp_file);
    ensure_no_symlink_components(path, "support bundle export file")
        .map_err(|_| support_bundle_operation_failed())?;
    std::fs::rename(&temp_path, path).map_err(|_| {
        let _ = std::fs::remove_file(&temp_path);
        support_bundle_operation_failed()
    })
}

fn support_bundle_markdown(
    request: &ExportSupportBundleRequest,
    exported_at: String,
    event_count: u32,
) -> String {
    format!(
        "# Jyowo Support Bundle\n\n- exportedAt: {exported_at}\n- conversationId: {}\n- runId: {}\n- eventCount: {event_count}\n- redacted: true\n",
        request.conversation_id.as_deref().unwrap_or(""),
        request.run_id.as_deref().unwrap_or("")
    )
}

#[tauri::command]
pub fn get_app_info() -> AppInfoPayload {
    get_app_info_payload()
}

#[tauri::command]
pub fn harness_healthcheck() -> HarnessHealthcheckPayload {
    harness_healthcheck_payload()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectRecord>,
    pub active_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchProjectResponse {
    pub project: ProjectRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectResponse {
    pub path: String,
    pub active_path: Option<String>,
    pub status: &'static str,
}

#[tauri::command]
pub fn list_projects(project_registry: tauri::State<'_, ProjectRegistry>) -> ListProjectsResponse {
    ListProjectsResponse {
        projects: project_registry.list_projects(),
        active_path: project_registry.active_path(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn switch_project(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(PathBuf::from(path), "project path".to_owned())?;
    let project = project_registry.set_active(&workspace_root)?;
    let new_runtime = runtime_state_for_workspace(workspace_root).await?;
    *runtime_handle.write().await = new_runtime;
    Ok(SwitchProjectResponse { project })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_project(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<DeleteProjectResponse, CommandErrorPayload> {
    if path.trim().is_empty() {
        return Err(CommandErrorPayload {
            code: "INVALID_PAYLOAD",
            message: "project path is required".to_owned(),
        });
    }

    let removed = project_registry.remove(&PathBuf::from(path))?;
    let active_path = project_registry.active_path();
    if active_path.is_none() {
        *runtime_handle.write().await = unconfigured_runtime_state();
    }

    Ok(DeleteProjectResponse {
        path: removed.path,
        active_path,
        status: "deleted",
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_project(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(PathBuf::from(path), "project path".to_owned())?;
    let project = project_registry.upsert_and_activate(&workspace_root)?;
    let new_runtime = runtime_state_for_workspace(workspace_root).await?;
    *runtime_handle.write().await = new_runtime;
    Ok(SwitchProjectResponse { project })
}

#[tauri::command]
pub async fn list_model_provider_catalog() -> ModelProviderCatalogResponse {
    list_model_provider_catalog_payload_with_remote().await
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_execution_settings(
    workspace_path: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.blocking_read();
    get_execution_settings_for_request(
        GetExecutionSettingsRequest { workspace_path },
        runtime_state.execution_settings_store.as_ref(),
        &project_registry,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_execution_settings(
    permission_mode: PermissionMode,
    context_compression_trigger_ratio: f32,
    subagents_enabled: bool,
    agent_teams_enabled: bool,
    background_agents_enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SetExecutionSettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _execution_settings_guard = runtime_state.execution_settings_lock.lock().await;
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode,
            context_compression_trigger_ratio,
            subagents_enabled,
            agent_teams_enabled,
            background_agents_enabled,
        },
        runtime_state.execution_settings_store.as_ref(),
    )
}

#[tauri::command]
pub async fn list_provider_settings(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListProviderSettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_provider_settings_with_store(runtime_state.provider_settings_store.as_ref()).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn request_provider_config_api_key_reveal(
    config_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest { config_id },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_provider_config_api_key(
    config_id: String,
    reveal_token: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id,
            reveal_token,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn validate_provider_settings(
    model_id: String,
    provider_id: String,
) -> Result<ValidateProviderSettingsResponse, CommandErrorPayload> {
    validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id,
        provider_id,
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_provider_settings(
    api_key: Option<String>,
    base_url: Option<String>,
    config_id: Option<String>,
    display_name: Option<String>,
    model_id: String,
    provider_id: String,
    set_default: Option<bool>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    let request = ProviderSettingsRequest {
        api_key,
        base_url,
        config_id,
        display_name,
        model_id,
        provider_id,
        set_default: set_default.unwrap_or(true),
    };
    let response =
        save_provider_settings_with_runtime_state_unlocked(request, &runtime_state).await?;
    if response.config.is_default {
        let stream_permission_runtime = runtime_state
            .stream_permission_runtime
            .as_ref()
            .ok_or_else(|| runtime_unavailable("Provider settings require the desktop runtime."))?;
        let (harness, model_id, protocol) = build_desktop_harness(
            &runtime_state.workspace_root,
            Arc::clone(stream_permission_runtime),
            Some(&response.config.id),
        )
        .await?;
        let _start_run_guard = runtime_state.start_run_lock.lock().await;
        runtime_state.replace_harness(Arc::new(harness), model_id, protocol);
    }
    Ok(response)
}

#[tauri::command]
pub async fn list_mcp_servers(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListMcpServersResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_mcp_servers_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_mcp_server(
    enabled: Option<bool>,
    display_name: String,
    id: String,
    scope: String,
    transport: McpServerTransportConfig,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: enabled.unwrap_or(true),
            display_name,
            id,
            scope,
            transport,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_mcp_server_config(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetMcpServerConfigResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_mcp_server_config_with_runtime_state(GetMcpServerConfigRequest { id }, &*runtime_state)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_mcp_server(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteMcpServerResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    delete_mcp_server_with_runtime_state(DeleteMcpServerRequest { id }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_mcp_server_enabled(
    id: String,
    enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SetMcpServerEnabledResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    set_mcp_server_enabled_with_runtime_state(
        SetMcpServerEnabledRequest { id, enabled },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn restart_mcp_server(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RestartMcpServerResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    restart_mcp_server_with_runtime_state(RestartMcpServerRequest { id }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_mcp_diagnostics(
    server_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListMcpDiagnosticsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_mcp_diagnostics_with_runtime_state(
        ListMcpDiagnosticsRequest { server_id },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn clear_mcp_diagnostics(
    server_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ClearMcpDiagnosticsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    clear_mcp_diagnostics_with_runtime_state(
        ClearMcpDiagnosticsRequest { server_id },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn subscribe_mcp_diagnostics(
    server_id: Option<String>,
    window: tauri::Window,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let window_label = window.label().to_owned();
    let emitter = Arc::new(move |batch: McpDiagnosticBatchPayload| {
        window
            .emit("mcp_diagnostic_batch", batch)
            .map_err(|error| error.to_string())
    });
    subscribe_mcp_diagnostics_for_window_with_runtime_state(
        SubscribeMcpDiagnosticsRequest { server_id },
        window_label,
        emitter,
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn unsubscribe_mcp_diagnostics(
    subscription_id: String,
    window: tauri::Window,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<UnsubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    unsubscribe_mcp_diagnostics_for_window_with_runtime_state(
        UnsubscribeMcpDiagnosticsRequest { subscription_id },
        window.label().to_owned(),
        &*runtime_state,
    )
    .await
}

#[tauri::command]
pub async fn list_skills(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListSkillsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_skills_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_skill_detail(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetSkillDetailResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_skill_detail_with_runtime_state(GetSkillDetailRequest { id }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_skill_file(
    id: String,
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetSkillFileResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_skill_file_with_runtime_state(GetSkillFileRequest { id, path }, &*runtime_state).await
}

#[tauri::command]
pub async fn list_skill_catalog_sources(
) -> Result<ListSkillCatalogSourcesResponse, CommandErrorPayload> {
    list_skill_catalog_sources_with_runtime_state().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_skill_catalog_entries(
    source_id: String,
    query: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
    sort: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_skill_catalog_entries_with_runtime_state(
        ListSkillCatalogEntriesRequest {
            source_id,
            query,
            cursor,
            limit,
            sort,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_skill_catalog_entry(
    source_id: String,
    entry_id: String,
    version: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_skill_catalog_entry_with_runtime_state(
        GetSkillCatalogEntryRequest {
            source_id,
            entry_id,
            version,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_skill_catalog_file(
    source_id: String,
    entry_id: String,
    version: Option<String>,
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_skill_catalog_file_with_runtime_state(
        GetSkillCatalogFileRequest {
            source_id,
            entry_id,
            version,
            path,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_skill_catalog_install_tasks(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListSkillCatalogInstallTasksResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_skill_catalog_install_tasks_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_skill_from_catalog(
    source_id: String,
    entry_id: String,
    version: Option<String>,
    operation_id: Option<String>,
    window: tauri::Window,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<InstallSkillFromCatalogResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await.clone();
    let emitter: Option<SkillCatalogInstallProgressEmitter> = Some({
        let window = window.clone();
        Arc::new(move |payload: SkillCatalogInstallProgressPayload| {
            let _ = window.emit("skill_catalog_install_progress", payload);
        }) as SkillCatalogInstallProgressEmitter
    });
    start_skill_catalog_install_task_with_runtime_state(
        InstallSkillFromCatalogRequest {
            source_id,
            entry_id,
            version,
            operation_id,
        },
        runtime_state,
        emitter,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn import_skill(
    source_path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _skill_store_guard = runtime_state.skill_store_lock.lock().await;
    import_skill_with_runtime_state(ImportSkillRequest { source_path }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_skill_enabled(
    id: String,
    enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SetSkillEnabledResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _skill_store_guard = runtime_state.skill_store_lock.lock().await;
    set_skill_enabled_with_runtime_state(SetSkillEnabledRequest { id, enabled }, &*runtime_state)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_skill(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteSkillResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _skill_store_guard = runtime_state.skill_store_lock.lock().await;
    delete_skill_with_runtime_state(DeleteSkillRequest { id }, &*runtime_state).await
}

#[tauri::command]
pub async fn list_plugins(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListPluginsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_plugins_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_plugin_detail(
    plugin_id: PluginId,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetPluginDetailResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_plugin_detail_with_runtime_state(GetPluginDetailRequest { plugin_id }, &*runtime_state)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn validate_plugin_from_path(
    source_path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PluginInstallReport, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    validate_plugin_from_path_with_runtime_state(
        ValidatePluginFromPathRequest { source_path },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_plugin_from_path(
    source_path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    install_plugin_from_path_with_runtime_state(
        InstallPluginFromPathRequest { source_path },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_plugin_enabled(
    plugin_id: PluginId,
    enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    set_plugin_enabled_with_runtime_state(
        SetPluginEnabledRequest { plugin_id, enabled },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_project_plugins_enabled(
    enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SetProjectPluginsEnabledResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    set_project_plugins_enabled_with_runtime_state(
        SetProjectPluginsEnabledRequest { enabled },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_plugin_config(
    plugin_id: PluginId,
    values: Value,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    update_plugin_config_with_runtime_state(
        UpdatePluginConfigRequest { plugin_id, values },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn uninstall_plugin(
    plugin_id: PluginId,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    uninstall_plugin_with_runtime_state(UninstallPluginRequest { plugin_id }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reload_plugin(
    plugin_id: PluginId,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    reload_plugin_with_runtime_state(ReloadPluginRequest { plugin_id }, &*runtime_state).await
}

#[tauri::command]
pub async fn list_memory_items(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListMemoryItemsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_memory_items_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_memory_item(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetMemoryItemResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_memory_item_with_runtime_state(GetMemoryItemRequest { id }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_memory_item(
    content: String,
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    update_memory_item_with_runtime_state(UpdateMemoryItemRequest { content, id }, &*runtime_state)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_memory_item(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteMemoryItemResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    delete_memory_item_with_runtime_state(DeleteMemoryItemRequest { id }, &*runtime_state).await
}

#[tauri::command]
pub async fn export_memory_items(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    export_memory_items_with_runtime_state(&*runtime_state).await
}

#[tauri::command]
pub async fn list_conversations(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListConversationsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    Ok(list_conversations_with_runtime_state(&*runtime_state).await)
}

#[tauri::command]
pub async fn create_conversation(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<CreateConversationResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    create_conversation_with_runtime_state(&*runtime_state).await
}

#[tauri::command]
pub async fn list_eval_cases(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListEvalCasesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_eval_cases_with_runtime_state(&*runtime_state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_artifacts(
    conversation_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_artifacts_with_runtime_state(ListArtifactsRequest { conversation_id }, &*runtime_state)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_artifact_media_preview(
    conversation_id: String,
    artifact_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetArtifactMediaPreviewResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id,
            artifact_id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_attachment_media_preview(
    conversation_id: String,
    attachment_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetAttachmentMediaPreviewResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id,
            attachment_id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn run_eval_case(
    case_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    run_eval_case_with_runtime_state(RunEvalCaseRequest { case_id }, &*runtime_state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_conversation(
    conversation_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetConversationResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_conversation_with_runtime_state(GetConversationRequest { conversation_id }, &*runtime_state)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_conversation_model_config(
    conversation_id: String,
    model_config_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SetConversationModelConfigResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    set_conversation_model_config_with_runtime_state(
        SetConversationModelConfigRequest {
            conversation_id,
            model_config_id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_conversation(
    conversation_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteConversationResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    delete_conversation_with_runtime_state(
        DeleteConversationRequest { conversation_id },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn start_run(
    attachments: Option<Vec<AttachmentReferencePayload>>,
    client_message_id: Option<String>,
    context_references: Option<Vec<ContextReferencePayload>>,
    conversation_id: String,
    permission_mode: Option<PermissionMode>,
    prompt: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<StartRunResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    start_run_with_runtime_state(
        StartRunRequest {
            attachments,
            client_message_id,
            context_references,
            conversation_id,
            permission_mode,
            prompt,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn create_attachment_from_path(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<CreateAttachmentFromPathResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest { path },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_reference_candidates(
    conversation_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListReferenceCandidatesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest { conversation_id },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn cancel_run(
    run_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<CancelRunResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    cancel_run_with_runtime_state(CancelRunRequest { run_id }, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn resolve_permission(
    conversation_id: String,
    decision: PermissionDecision,
    request_id: String,
    window: tauri::Window,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let window_label = window.label().to_owned();
    resolve_permission_for_window_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id,
            decision,
            request_id,
        },
        window_label,
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_activity(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListActivityResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id,
            run_id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_replay_timeline(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ReplayTimelineResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id,
            run_id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn page_conversation_timeline(
    conversation_id: String,
    after_cursor: Option<ConversationCursor>,
    limit: Option<usize>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<PageConversationTimelineResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id,
            after_cursor,
            limit,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn page_conversation_worktree(
    conversation_id: String,
    page_cursor: Option<ConversationTurnCursor>,
    direction: Option<PageConversationWorktreeDirection>,
    limit: Option<usize>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ConversationWorktreePage, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id,
            page_cursor,
            direction: direction.unwrap_or_else(default_worktree_direction),
            limit,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn subscribe_conversation_events(
    conversation_id: String,
    after_cursor: Option<ConversationCursor>,
    window: tauri::Window,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let window_label = window.label().to_owned();
    let emitter = Arc::new(move |batch: ConversationEventBatchPayload| {
        window
            .emit("conversation_event_batch", batch)
            .map_err(|error| error.to_string())
    });
    subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id,
            after_cursor,
        },
        window_label,
        emitter,
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn unsubscribe_conversation_events(
    subscription_id: String,
    window: tauri::Window,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest { subscription_id },
        window.label().to_owned(),
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn export_support_bundle(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ExportSupportBundleResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id,
            run_id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_context_snapshot(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetContextSnapshotResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id,
            run_id,
        },
        &*runtime_state,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_contracts::{
        ManifestOriginRef, PluginCapabilitiesSummary, PluginFailedEvent,
        PluginLifecycleStateDiscriminant, PluginLoadedEvent, PluginProductState,
        PluginRejectedEvent, PluginSourceKind, RejectionReason, TrustLevel,
    };

    struct EmptyRedactor;

    impl Redactor for EmptyRedactor {
        fn redact(&self, _input: &str, _rules: &RedactRules) -> String {
            String::new()
        }
    }

    #[test]
    fn run_end_reason_display_returns_non_empty_error_fallback() {
        assert_eq!(
            run_end_reason_display(
                &EndReason::Error(String::new()),
                &DefaultRedactor::default()
            ),
            "Run error withheld from conversation timeline."
        );
        assert_eq!(
            run_end_reason_display(
                &EndReason::Error("provider failed".to_owned()),
                &EmptyRedactor,
            ),
            "Run error withheld from conversation timeline."
        );
    }

    #[test]
    fn run_event_mapper_projects_plugin_lifecycle_events_without_raw_errors() {
        let requested_session_id = SessionId::new();
        let mut mapper = RunEventMapper::default();
        let manifest_origin = ManifestOriginRef::File {
            path: "/Users/goya/.config/jyowo/plugins/formatter/plugin.json".to_owned(),
        };
        let loaded = mapper
            .map(
                "evt-plugin-loaded".to_owned(),
                Event::PluginLoaded(PluginLoadedEvent {
                    tenant_id: TenantId::SINGLE,
                    plugin_id: PluginId("formatter@1.0.0".to_owned()),
                    plugin_name: "formatter".to_owned(),
                    plugin_version: "1.0.0".to_owned(),
                    trust_level: TrustLevel::UserControlled,
                    capabilities: plugin_capabilities_summary_for_test(),
                    manifest_origin: manifest_origin.clone(),
                    manifest_hash: [7; 32],
                    from_state: PluginLifecycleStateDiscriminant::Validated,
                    at: Utc::now(),
                }),
                requested_session_id,
                &DefaultRedactor::default(),
            )
            .expect("plugin loaded should be projected");

        assert_eq!(loaded.run_id, "plugin-runtime");
        assert_eq!(loaded.event_type, "plugin.loaded");
        assert_eq!(loaded.source, "plugin");
        assert_eq!(loaded.visibility, "redacted");
        assert_eq!(loaded.payload["pluginId"], "formatter@1.0.0");
        assert_eq!(loaded.payload["pluginName"], "formatter");
        assert_eq!(loaded.payload["trustLevel"], "user_controlled");
        assert_eq!(loaded.payload["capabilityCount"], 3);

        let rejected = mapper
            .map(
                "evt-plugin-rejected".to_owned(),
                Event::PluginRejected(PluginRejectedEvent {
                    tenant_id: TenantId::SINGLE,
                    plugin_id: PluginId("formatter@1.0.0".to_owned()),
                    plugin_name: "formatter".to_owned(),
                    plugin_version: "1.0.0".to_owned(),
                    trust_level: TrustLevel::UserControlled,
                    manifest_origin: manifest_origin.clone(),
                    manifest_hash: [7; 32],
                    reason: RejectionReason::AdmissionDenied {
                        policy: "Authorization=Bearer plugin-secret-token".to_owned(),
                    },
                    at: Utc::now(),
                }),
                requested_session_id,
                &DefaultRedactor::default(),
            )
            .expect("plugin rejected should be projected");

        assert_eq!(rejected.event_type, "plugin.rejected");
        assert_eq!(rejected.source, "plugin");
        assert_eq!(rejected.visibility, "redacted");
        assert_eq!(rejected.payload["reason"], "AdmissionDenied");
        let rejected_payload = serde_json::to_string(&rejected.payload).unwrap();
        assert!(!rejected_payload.contains("plugin-secret-token"));
        assert!(!rejected_payload.contains("Authorization"));
        assert!(!rejected_payload.contains("/Users/goya"));

        let failed = mapper
            .map(
                "evt-plugin-failed".to_owned(),
                Event::PluginFailed(PluginFailedEvent {
                    tenant_id: TenantId::SINGLE,
                    plugin_id: PluginId("formatter@1.0.0".to_owned()),
                    plugin_name: "formatter".to_owned(),
                    plugin_version: "1.0.0".to_owned(),
                    trust_level: TrustLevel::UserControlled,
                    manifest_origin,
                    manifest_hash: [7; 32],
                    failure: "sidecar crashed with token=plugin-secret-token".to_owned(),
                    at: Utc::now(),
                }),
                requested_session_id,
                &DefaultRedactor::default(),
            )
            .expect("plugin failed should be projected");

        assert_eq!(failed.event_type, "plugin.failed");
        assert_eq!(
            failed.payload["message"],
            "Plugin failure withheld from conversation timeline."
        );
        let failed_payload = serde_json::to_string(&failed.payload).unwrap();
        assert!(!failed_payload.contains("plugin-secret-token"));
        assert!(!failed_payload.contains("sidecar crashed"));
    }

    #[test]
    fn skill_catalog_progress_payload_serializes_camel_case() {
        let payload = SkillCatalogInstallProgressPayload {
            operation_id: "catalog-install-001".to_owned(),
            source_id: "anthropic".to_owned(),
            entry_id: "anthropic:frontend-design".to_owned(),
            version: Some("main".to_owned()),
            stage: "downloading",
            percent: 45,
            message: None,
        };

        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({
                "operationId": "catalog-install-001",
                "sourceId": "anthropic",
                "entryId": "anthropic:frontend-design",
                "version": "main",
                "stage": "downloading",
                "percent": 45
            })
        );
        assert_eq!(skill_catalog_install_stage("unknown"), "preparing");
    }

    #[test]
    fn skill_catalog_progress_emit_requires_operation_id_and_clamps_percent() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let emitter: Option<SkillCatalogInstallProgressEmitter> = Some(Arc::new(move |payload| {
            captured_events.lock().unwrap().push(payload);
        }));
        let request = InstallSkillFromCatalogRequest {
            source_id: "anthropic".to_owned(),
            entry_id: "anthropic:frontend-design".to_owned(),
            version: Some("main".to_owned()),
            operation_id: Some("catalog-install-001".to_owned()),
        };

        emit_skill_catalog_install_progress(&emitter, &request, "downloading", 250, None);

        let recorded_events = events.lock().unwrap();
        assert_eq!(recorded_events.len(), 1);
        assert_eq!(recorded_events[0].stage, "downloading");
        assert_eq!(recorded_events[0].percent, 100);
        drop(recorded_events);

        let request_without_operation = InstallSkillFromCatalogRequest {
            operation_id: None,
            ..request
        };
        emit_skill_catalog_install_progress(
            &emitter,
            &request_without_operation,
            "downloading",
            25,
            None,
        );

        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn skill_catalog_install_tasks_are_deduped_and_listable_by_entry() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let request = InstallSkillFromCatalogRequest {
            source_id: "anthropic".to_owned(),
            entry_id: "anthropic:frontend-design".to_owned(),
            version: Some("main".to_owned()),
            operation_id: Some("catalog-install-001".to_owned()),
        };
        let duplicate_request = InstallSkillFromCatalogRequest {
            operation_id: Some("catalog-install-002".to_owned()),
            ..request.clone()
        };

        let first = get_or_create_skill_catalog_install_task(&state, &request).unwrap();
        let duplicate =
            get_or_create_skill_catalog_install_task(&state, &duplicate_request).unwrap();
        record_skill_catalog_install_task_progress(&state, &request, "downloading", 45, None)
            .await
            .unwrap();
        let tasks = list_skill_catalog_install_tasks_with_runtime_state(&state)
            .await
            .unwrap();

        assert_eq!(duplicate.operation_id, first.operation_id);
        assert_eq!(tasks.tasks.len(), 1);
        assert_eq!(tasks.tasks[0].operation_id, "catalog-install-001");
        assert_eq!(tasks.tasks[0].stage, "downloading");
        assert_eq!(tasks.tasks[0].percent, 45);
        assert_eq!(tasks.tasks[0].status, "running");
    }

    #[tokio::test]
    async fn plugin_install_failure_does_not_write_store_record() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("plugin.json"),
            r#"{"manifest_schema_version":99,"name":"bad-plugin"}"#,
        )
        .unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();

        let result = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let plugins = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(result.status, PluginOperationStatus::Rejected);
        let report = result.report.as_ref().expect("rejection includes report");
        assert_eq!(report.source_path, "<local-plugin>");
        assert_eq!(
            report.reason.as_deref(),
            Some("plugin manifest uses an unsupported schema version")
        );
        assert!(!report
            .source_path
            .contains(source_path.to_string_lossy().as_ref()));
        assert!(!report
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("unsupported manifest_schema_version 99"));
        assert!(plugins.plugins.is_empty());
    }

    #[tokio::test]
    async fn installed_plugin_can_be_listed_and_disabled_without_activation() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "local-tools");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();

        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();
        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(install.status, PluginOperationStatus::Installed);
        assert_eq!(listed.plugins.len(), 1);
        assert_eq!(listed.plugins[0].id, installed_id);
        assert!(!listed.plugins[0].enabled);
        assert!(matches!(
            listed.plugins[0].state,
            PluginProductState::Disabled { .. }
        ));

        let disabled = set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: installed_id.clone(),
                enabled: false,
            },
            &state,
        )
        .await
        .unwrap();
        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(disabled.status, PluginOperationStatus::Disabled);
        assert_eq!(listed.plugins[0].id, installed_id);
        assert!(!listed.plugins[0].enabled);
        assert!(matches!(
            listed.plugins[0].state,
            PluginProductState::Disabled { .. }
        ));
    }

    #[tokio::test]
    async fn unregistered_user_plugin_package_is_rejected_by_desktop_registry() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "registered-tools");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        let rogue_package = state.plugin_store.package_root().join("rogue-tools_0.1.0");
        std::fs::create_dir_all(&rogue_package).unwrap();
        write_desktop_plugin_package(&rogue_package, "rogue-tools");

        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(listed.plugins.len(), 1);
        assert_eq!(listed.plugins[0].name, "registered-tools");
        assert!(listed
            .plugins
            .iter()
            .all(|plugin| plugin.name != "rogue-tools"));
    }

    #[tokio::test]
    async fn installing_file_plugin_without_sidecar_is_rejected() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest(source.path(), "local-preflight");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();
        let result = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let listed = list_plugins_with_runtime_state(&state).await.unwrap();

        assert_eq!(result.status, PluginOperationStatus::Rejected);
        assert_eq!(
            result
                .report
                .as_ref()
                .and_then(|report| report.reason.as_deref()),
            Some("local plugin package must include a jyowo-plugin-* sidecar executable")
        );
        assert!(listed.plugins.is_empty());
    }

    #[tokio::test]
    async fn plugin_config_update_preserves_existing_secret_config_fields() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_config_schema(source.path(), "secret-tools");
        write_desktop_plugin_sidecar(source.path(), "secret-tools");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();

        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();
        let mut settings = state.plugin_store.load_record().unwrap();
        settings.records[0].config =
            serde_json::json!({ "apiToken": "managed-secret-ref", "lineWidth": 80 });
        state.plugin_store.save_record(&settings).unwrap();

        update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id.clone(),
                values: serde_json::json!({ "lineWidth": 120 }),
            },
            &state,
        )
        .await
        .unwrap();

        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({ "apiToken": "managed-secret-ref", "lineWidth": 120 })
        );
        let detail = get_plugin_detail_with_runtime_state(
            GetPluginDetailRequest {
                plugin_id: installed_id,
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(
            detail.plugin.config,
            serde_json::json!({ "lineWidth": 120 })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_validates_merged_config_values() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_required_config_schema(source.path(), "merged-config");
        write_desktop_plugin_sidecar(source.path(), "merged-config");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let installed_id = PluginId("merged-config@0.1.0".to_owned());
        let package_dir = plugin_package_dir_name(&installed_id);
        state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .unwrap();
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: installed_id.clone(),
                    name: "merged-config".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: serde_json::json!({ "mode": "default", "limit": 10 }),
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .unwrap();

        update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "limit": 20 }),
            },
            &state,
        )
        .await
        .expect("merged config satisfies required schema fields");

        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({ "mode": "default", "limit": 20 })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_rejects_unknown_schema_fields_without_persisting() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_required_config_schema(source.path(), "strict-config");
        write_desktop_plugin_sidecar(source.path(), "strict-config");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let installed_id = PluginId("strict-config@0.1.0".to_owned());
        let package_dir = plugin_package_dir_name(&installed_id);
        state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .unwrap();
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: installed_id.clone(),
                    name: "strict-config".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: serde_json::json!({ "mode": "default", "limit": 10 }),
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .unwrap();

        let result = update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "unknown": "ignored" }),
            },
            &state,
        )
        .await;

        assert!(result.is_err());
        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({ "mode": "default", "limit": 10 })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_rejects_existing_unknown_schema_fields_without_persisting_patch()
    {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest_with_required_config_schema(source.path(), "strict-existing");
        write_desktop_plugin_sidecar(source.path(), "strict-existing");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let installed_id = PluginId("strict-existing@0.1.0".to_owned());
        let package_dir = plugin_package_dir_name(&installed_id);
        state
            .plugin_store
            .write_plugin_package(&package_dir, &source.path().canonicalize().unwrap())
            .unwrap();
        state
            .plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: installed_id.clone(),
                    name: "strict-existing".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir,
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: serde_json::json!({
                        "mode": "default",
                        "unknown": "already-present"
                    }),
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .unwrap();

        let result = update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "limit": 20 }),
            },
            &state,
        )
        .await;

        assert!(result.is_err());
        let settings = state.plugin_store.load_record().unwrap();
        assert_eq!(
            settings.records[0].config,
            serde_json::json!({
                "mode": "default",
                "unknown": "already-present"
            })
        );
    }

    #[tokio::test]
    async fn plugin_config_update_rejects_secret_like_fields_without_secret_schema() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest(source.path(), "plain-config");
        write_desktop_plugin_sidecar(source.path(), "plain-config");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let source_path = source.path().canonicalize().unwrap();
        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source_path.to_string_lossy().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();

        let result = update_plugin_config_with_runtime_state(
            UpdatePluginConfigRequest {
                plugin_id: installed_id,
                values: serde_json::json!({ "apiToken": "not-even-a-real-token" }),
            },
            &state,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_plugin_allow_gate_is_persisted_by_command() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();

        let response = set_project_plugins_enabled_with_runtime_state(
            SetProjectPluginsEnabledRequest { enabled: true },
            &state,
        )
        .await
        .unwrap();

        assert!(response.allow_project_plugins);
        assert!(
            state
                .plugin_store
                .load_record()
                .unwrap()
                .allow_project_plugins
        );
        assert!(
            list_plugins_with_runtime_state(&state)
                .await
                .unwrap()
                .allow_project_plugins
        );
    }

    #[tokio::test]
    async fn enabling_cargo_extension_plugin_does_not_run_activate_preflight() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        write_desktop_plugin_manifest(source.path(), "counting-sidecar");
        let binary = source.path().join("jyowo-plugin-counting-sidecar");
        let counter = workspace.path().join("activate-count");
        write_desktop_executable(
            &binary,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
cat "$0.metadata"
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf activate >> '{}'
    printf '{{"jsonrpc":"2.0","id":1,"result":{{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{{"jsonrpc":"2.0","id":1,"result":null}}'
    exit 0
    ;;
esac
fi
	exit 2
	"#,
                counter.display()
            ),
        );
        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: install.plugin_id.unwrap(),
                enabled: true,
            },
            &state,
        )
        .await
        .unwrap();

        assert!(
            !counter.exists(),
            "enable preflight must not execute sidecar activate"
        );
    }

    #[tokio::test]
    async fn enabling_plugin_rejects_installed_package_hash_mismatch() {
        let workspace = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_package(source.path(), "tampered-sidecar");
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        let install = install_plugin_from_path_with_runtime_state(
            InstallPluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        let installed_id = install.plugin_id.clone().unwrap();
        let settings = state.plugin_store.load_record().unwrap();
        let package_dir = settings.records[0].package_dir.clone();
        write_desktop_executable(
            &state
                .plugin_store
                .package_root()
                .join(&package_dir)
                .join("jyowo-plugin-tampered-sidecar"),
            r#"#!/bin/sh
printf tampered
exit 0
"#,
        );

        let result = set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: installed_id,
                enabled: true,
            },
            &state,
        )
        .await;

        assert!(result.is_err());
        let settings = state.plugin_store.load_record().unwrap();
        assert!(!settings.records[0].enabled);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn plugin_validation_rejects_world_writable_source_directory() {
        use std::os::unix::fs::PermissionsExt;

        let source = tempfile::tempdir().unwrap();
        write_desktop_plugin_manifest(source.path(), "world-writable-plugin");
        let mut permissions = std::fs::metadata(source.path()).unwrap().permissions();
        permissions.set_mode(0o777);
        std::fs::set_permissions(source.path(), permissions).unwrap();
        let state = DesktopRuntimeState::with_workspace_for_test(
            tempfile::tempdir().unwrap().path().to_path_buf(),
        )
        .unwrap();

        let error = validate_plugin_from_path_with_runtime_state(
            ValidatePluginFromPathRequest {
                source_path: source.path().canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .expect_err("world-writable plugin source must be rejected");

        assert!(error.message.contains("world-writable"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn plugin_validation_rejects_world_writable_source_ancestor() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("writable-parent");
        let source = parent.join("plugin");
        std::fs::create_dir_all(&source).unwrap();
        write_desktop_plugin_manifest(&source, "world-writable-parent-plugin");
        let mut permissions = std::fs::metadata(&parent).unwrap().permissions();
        permissions.set_mode(0o777);
        std::fs::set_permissions(&parent, permissions).unwrap();
        let state = DesktopRuntimeState::with_workspace_for_test(
            tempfile::tempdir().unwrap().path().to_path_buf(),
        )
        .unwrap();

        let error = validate_plugin_from_path_with_runtime_state(
            ValidatePluginFromPathRequest {
                source_path: source.canonicalize().unwrap().display().to_string(),
            },
            &state,
        )
        .await
        .expect_err("world-writable plugin source ancestor must be rejected");

        assert!(error.message.contains("world-writable"));
    }

    #[test]
    fn plugin_package_dir_validation_rejects_path_like_values() {
        for value in [".", "..", ".hidden", "nested/path", "nested\\path"] {
            assert!(
                ensure_plugin_package_dir_name(value).is_err(),
                "{value} must be rejected"
            );
        }

        ensure_plugin_package_dir_name("formatter_0.1.0").unwrap();
    }

    #[test]
    fn plugin_store_rejects_tampered_package_dir_in_index() {
        let workspace = tempfile::tempdir().unwrap();
        let workspace = workspace.path().canonicalize().unwrap();
        let store = DesktopPluginStore::new(workspace);
        let index_path = store.index_path();
        std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
        let record = serde_json::json!({
            "records": [{
                "pluginId": "formatter@0.1.0",
                "name": "formatter",
                "version": "0.1.0",
                "enabled": true,
                "packageDir": "..",
                "sourcePath": "<local-plugin>",
                "contentHash": "hash",
                "importedAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:00:00Z",
                "config": null
            }]
        });
        std::fs::write(index_path, serde_json::to_vec(&record).unwrap()).unwrap();

        let error = store
            .load_record()
            .expect_err("tampered index must fail closed");

        assert!(error.message.contains("plugin package directory"));
    }

    #[tokio::test]
    async fn desktop_cargo_extension_search_path_discovers_workspace_owned_sidecar() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        write_desktop_cargo_extension(
            &state.plugin_store.cargo_extension_root(),
            "standalone-tools",
        );

        let response = list_plugins_with_runtime_state(&state).await.unwrap();

        assert!(response.plugins.iter().any(|plugin| {
            plugin.id == PluginId("standalone-tools@0.1.0".to_owned())
                && plugin.source == PluginSourceKind::CargoExtension
        }));
    }

    #[tokio::test]
    async fn plugin_uninstall_does_not_delete_package_when_index_save_fails() {
        let workspace = tempfile::tempdir().unwrap();
        let plugin_id = PluginId("formatter@0.1.0".to_owned());
        let store = Arc::new(FailingSavePluginStore::new(PluginSettingsRecord {
            records: vec![PluginStoreRecord {
                plugin_id: plugin_id.clone(),
                name: "formatter".to_owned(),
                version: "0.1.0".to_owned(),
                enabled: true,
                package_dir: "formatter_0.1.0".to_owned(),
                source_path: "/tmp/formatter".to_owned(),
                content_hash: "hash".to_owned(),
                imported_at: "2026-01-01T00:00:00Z".to_owned(),
                updated_at: "2026-01-01T00:00:00Z".to_owned(),
                config: Value::Null,
                last_validation_error: None,
            }],
            ..PluginSettingsRecord::default()
        }));
        let mut state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();
        state.plugin_store = store.clone();

        let result =
            uninstall_plugin_with_runtime_state(UninstallPluginRequest { plugin_id }, &state).await;

        assert!(result.is_err());
        assert!(store.deleted_packages().is_empty());
    }

    #[test]
    fn desktop_cargo_extension_search_paths_use_workspace_owned_extension_dir() {
        let workspace = tempfile::tempdir().unwrap();
        let state =
            DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf()).unwrap();

        let paths = desktop_cargo_extension_search_paths(state.plugin_store.as_ref());

        assert_eq!(paths, vec![state.plugin_store.cargo_extension_root()]);
    }

    #[test]
    fn run_end_reason_display_withholds_error_reason() {
        let reason = run_end_reason_display(
            &EndReason::Error("provider failed with sk-abcdefghijklmnopqrstuvwxyz".to_owned()),
            &DefaultRedactor::default(),
        );

        assert_eq!(reason, "Run error withheld from conversation timeline.");
        assert!(!reason.contains("provider failed"));
        assert!(!reason.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn message_content_display_redacts_private_absolute_paths() {
        let body = message_content_display(
            &MessageContent::Text(
                "read /Users/goya/.ssh/config and C:\\Users\\goya\\.ssh\\config".to_owned(),
            ),
            &DefaultRedactor::default(),
        );

        assert_eq!(body, "read [REDACTED] and [REDACTED]");
    }

    #[tokio::test]
    async fn create_conversation_does_not_wait_for_start_run_lock() {
        let workspace = std::env::temp_dir().join(format!("jyowo-create-lock-{}", RunId::new()));
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        DesktopProviderSettingsStore::new(workspace.clone())
            .save_record(&ProviderSettingsRecord {
                default_config_id: Some("openai-work".to_owned()),
                configs: vec![ProviderConfigRecord {
                    api_key: "provider-test-token".to_owned(),
                    protocol: ModelProtocol::Responses,
                    base_url: None,
                    display_name: "OpenAI Work".to_owned(),
                    id: "openai-work".to_owned(),
                    model_id: "gpt-5.4-mini".to_owned(),
                    provider_id: "openai".to_owned(),
                    model_descriptor: ProviderModelDescriptorRecord {
                        protocol: ModelProtocol::Responses,
                        conversation_capability: ConversationModelCapabilityRecord {
                            input_modalities: vec![ProviderModelModalityRecord::Text],
                            output_modalities: vec![ProviderModelModalityRecord::Text],
                            context_window: 128_000,
                            max_output_tokens: 16_384,
                            streaming: true,
                            tool_calling: true,
                            reasoning: false,
                            prompt_cache: true,
                            structured_output: true,
                        },
                        context_window: 128_000,
                        display_name: "GPT-5.4 mini".to_owned(),
                        lifecycle: ProviderModelLifecycleRecord::Stable,
                        max_output_tokens: 16_384,
                        model_id: "gpt-5.4-mini".to_owned(),
                        provider_id: "openai".to_owned(),
                    },
                }],
            })
            .unwrap();
        let state = runtime_state_for_workspace(workspace).await.unwrap();
        let _start_guard = state.start_run_lock.lock().await;

        let created = tokio::time::timeout(
            Duration::from_millis(250),
            create_conversation_with_runtime_state(&state),
        )
        .await
        .expect("creating an empty conversation must not wait for the start-run lock")
        .expect("conversation should be created");

        assert!(created.conversation.is_empty);
    }

    fn plugin_capabilities_summary_for_test() -> PluginCapabilitiesSummary {
        PluginCapabilitiesSummary {
            tools: 1,
            hooks: 1,
            mcp_servers: 0,
            skills: 1,
            steering: false,
            memory_provider: false,
            coordinator: false,
        }
    }

    fn write_desktop_plugin_manifest(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "manifest_schema_version": 1,
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "tools": [{ "name": "local-tool", "destructive": false }]
            }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_desktop_plugin_manifest_with_config_schema(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "manifest_schema_version": 1,
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "configuration_schema": {
                    "type": "object",
                    "required": ["apiToken"],
                    "properties": {
                        "apiToken": { "type": "string", "secret": true },
                        "lineWidth": { "type": "number" }
                    },
                    "additionalProperties": false
                },
                "tools": [{ "name": "local-tool", "destructive": false }]
            }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_desktop_plugin_manifest_with_required_config_schema(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "manifest_schema_version": 1,
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "configuration_schema": {
                    "type": "object",
                    "required": ["mode"],
                    "properties": {
                        "mode": { "type": "string" },
                        "limit": { "type": "number" }
                    },
                    "additionalProperties": false
                }
            }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_desktop_plugin_package(root: &Path, name: &str) {
        write_desktop_plugin_manifest(root, name);
        write_desktop_plugin_sidecar(root, name);
    }

    fn write_desktop_cargo_extension(root: &Path, name: &str) {
        let manifest = serde_json::json!({
            "manifest_schema_version": 1,
            "name": name,
            "version": "0.1.0",
            "trust_level": "user_controlled",
            "min_harness_version": ">=0.0.0",
            "capabilities": {
                "tools": [{ "name": "local-tool", "destructive": false }]
            }
        });
        let metadata = serde_json::json!({
            "manifest": manifest,
            "package_metadata": { "package": name }
        });
        write_desktop_executable(
            &root.join(format!("jyowo-plugin-{name}")),
            format!(
                r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
printf '%s' '{}'
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
  printf '{{"jsonrpc":"2.0","id":1,"result":{{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}}}'
  exit 0
fi
exit 2
"#,
                metadata
            ),
        );
    }

    fn write_desktop_plugin_sidecar(root: &Path, name: &str) {
        write_desktop_executable(
            &root.join(format!("jyowo-plugin-{name}")),
            r#"#!/bin/sh
if [ "$1" = "--harness-runtime" ]; then
  printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
  exit 0
fi
exit 2
"#,
        );
    }

    fn write_desktop_executable(path: &Path, content: impl AsRef<str>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content.as_ref()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).unwrap();
        }
    }

    #[derive(Clone)]
    struct FailingSavePluginStore {
        deleted_packages: Arc<std::sync::Mutex<Vec<String>>>,
        record: PluginSettingsRecord,
        root: PathBuf,
    }

    impl FailingSavePluginStore {
        fn new(record: PluginSettingsRecord) -> Self {
            Self {
                deleted_packages: Arc::new(std::sync::Mutex::new(Vec::new())),
                record,
                root: std::env::temp_dir().join(format!("jyowo-plugin-store-{}", RunId::new())),
            }
        }

        fn deleted_packages(&self) -> Vec<String> {
            self.deleted_packages.lock().unwrap().clone()
        }
    }

    impl PluginStore for FailingSavePluginStore {
        fn package_root(&self) -> PathBuf {
            self.root.join("user")
        }

        fn cargo_extension_root(&self) -> PathBuf {
            self.root.join("extensions")
        }

        fn workspace_plugin_root(&self) -> PathBuf {
            self.root.join("workspace")
        }

        fn load_record(&self) -> Result<PluginSettingsRecord, CommandErrorPayload> {
            Ok(self.record.clone())
        }

        fn save_record(&self, _record: &PluginSettingsRecord) -> Result<(), CommandErrorPayload> {
            Err(runtime_operation_failed(
                "plugin index save failed".to_owned(),
            ))
        }

        fn write_plugin_package(
            &self,
            _package_dir: &str,
            _source_path: &Path,
        ) -> Result<(), CommandErrorPayload> {
            Ok(())
        }

        fn delete_plugin_package(&self, package_dir: &str) -> Result<(), CommandErrorPayload> {
            self.deleted_packages
                .lock()
                .unwrap()
                .push(package_dir.to_owned());
            Ok(())
        }
    }
}
