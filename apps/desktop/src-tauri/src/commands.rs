use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use jyowo_harness_sdk::builtin::{
    AnthropicProvider, CodexResponsesProvider, DeepSeekProvider, DefaultRedactor, DoubaoProvider,
    GeminiProvider, InMemoryMemoryProvider, JsonlEventStore, LocalLlamaProvider, LocalSandbox,
    MinimaxProvider, OpenAiProvider, OpenRouterProvider, QwenProvider, ZhipuProvider,
};
use jyowo_harness_sdk::ext::{
    now, AgentId, Decision, DecisionScope, Event, EventId, InteractivityLevel, McpConnectionState,
    McpEventSink, McpRegistry, McpServerId, McpServerScope, McpServerSource, McpServerSpec,
    MemoryId, MemoryKind, MemoryRecord, MemorySource, MemorySummary, MemoryVisibility,
    MessageContent, MessagePart, ModelProvider, PendingPermissionRequest, PermissionRequest,
    PermissionSubject, RedactPatternSet, RedactRules, RedactScope, Redactor, RequestId, RunId,
    SessionId, Severity, StdioEnv, StdioPolicy, StdioTransport, TenantId, ToolUseId,
    TransportChoice,
};
use jyowo_harness_sdk::{
    ConversationEventsPageRequest, ConversationTurnRequest, Harness, McpConfig, SessionOptions,
    StreamPermissionRuntime,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PLACEHOLDER_TIMESTAMP: &str = "2026-06-17T00:00:00.000Z";
const START_RUN_STARTED_TIMEOUT: Duration = Duration::from_secs(5);
const WORKSPACE_ROOT_ENV: &str = "JYOWO_WORKSPACE_ROOT";
const MAX_MEMORY_CONTENT_BYTES: usize = 64 * 1024;
const MAX_ARTIFACT_PREVIEW_BYTES: usize = 16 * 1024;

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
    pub api_key: String,
    pub model_id: String,
    pub provider_id: String,
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
    pub model_id: String,
    pub provider_id: String,
    pub secret_ref: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsRecord {
    pub model_id: String,
    pub provider_id: String,
    pub secret_ref: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_secret_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveMcpServerRequest {
    pub display_name: String,
    pub id: String,
    pub scope: String,
    pub transport: McpServerTransportConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum McpServerTransportConfig {
    Stdio { command: String, args: Vec<String> },
    InProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpServerConfigRecord {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSummaryPayload {
    pub display_name: String,
    pub exposed_tool_count: u32,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub origin: &'static str,
    pub scope: String,
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
pub struct DeleteMcpServerResponse {
    pub id: String,
    pub status: &'static str,
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
    fn secret_ref(&self, provider_id: &str) -> String;
    fn secret_ref_prefix(&self, provider_id: &str) -> String;
    fn save_secret(&self, secret_ref: &str, api_key: &str) -> Result<(), CommandErrorPayload>;
    fn delete_secret(&self, secret_ref: &str) -> Result<(), CommandErrorPayload>;
    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload>;
    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload>;
}

pub trait McpServerStore: Send + Sync {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload>;
    fn save_record(&self, record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload>;
    fn delete_record(&self, id: &str) -> Result<(), CommandErrorPayload>;
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
    fn secret_ref(&self, provider_id: &str) -> String {
        format!("{}{}", self.secret_ref_prefix(provider_id), RunId::new())
    }

    fn secret_ref_prefix(&self, provider_id: &str) -> String {
        provider_secret_ref_prefix(&self.workspace_root, provider_id)
    }

    fn save_secret(&self, secret_ref: &str, api_key: &str) -> Result<(), CommandErrorPayload> {
        let entry = keyring::Entry::new("jyowo.provider", secret_ref).map_err(|error| {
            runtime_operation_failed(format!("provider secret store unavailable: {error}"))
        })?;
        entry.set_password(api_key).map_err(|error| {
            runtime_operation_failed(format!("provider secret save failed: {error}"))
        })
    }

    fn delete_secret(&self, secret_ref: &str) -> Result<(), CommandErrorPayload> {
        let entry = keyring::Entry::new("jyowo.provider", secret_ref).map_err(|error| {
            runtime_operation_failed(format!("provider secret store unavailable: {error}"))
        })?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(runtime_operation_failed(format!(
                "provider secret cleanup failed: {error}"
            ))),
        }
    }

    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        ensure_no_symlink_components(&settings_path, "provider settings file")?;
        match std::fs::read(&settings_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
                runtime_operation_failed(format!("provider settings parse failed: {error}"))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(runtime_operation_failed(format!(
                "provider settings read failed: {error}"
            ))),
        }
    }

    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
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
        let mut temp_file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|error| {
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

#[derive(Clone)]
pub struct DesktopRuntimeState {
    default_conversation_id: SessionId,
    harness: Option<Arc<Harness>>,
    memory_lock: Arc<tokio::sync::Mutex<()>>,
    mcp_server_lock: Arc<tokio::sync::Mutex<()>>,
    mcp_server_store: Arc<dyn McpServerStore>,
    permission_resolver: Option<Arc<dyn PermissionResolver>>,
    provider_settings_lock: Arc<tokio::sync::Mutex<()>>,
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
    start_run_lock: Arc<tokio::sync::Mutex<()>>,
    stream_permission_runtime: Option<Arc<StreamPermissionRuntime>>,
    workspace_root: PathBuf,
}

impl DesktopRuntimeState {
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
            default_conversation_id: SessionId::new(),
            harness: Some(harness),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::new(workspace_root.clone())),
            permission_resolver: Some(permission_resolver),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::new(
                workspace_root.clone(),
            )),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: Some(stream_permission_runtime),
            workspace_root,
        })
    }

    #[must_use]
    pub fn harness(&self) -> Option<Arc<Harness>> {
        self.harness.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.stream_permission_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.pending_permission_requests())
    }

    #[must_use]
    pub fn conversation_session_options(&self, session_id: SessionId) -> SessionOptions {
        SessionOptions::new(&self.workspace_root)
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(session_id)
            .with_interactivity(InteractivityLevel::FullyInteractive)
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
    let harness =
        build_desktop_harness(&workspace_root, Arc::clone(&stream_permission_runtime)).await?;

    DesktopRuntimeState::with_harness_stream_permission_runtime_for_workspace(
        workspace_root,
        Arc::new(harness),
        stream_permission_runtime,
    )
}

async fn build_desktop_harness(
    workspace_root: &Path,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
) -> Result<Harness, CommandErrorPayload> {
    let event_store = JsonlEventStore::open(
        workspace_root.join(".jyowo").join("runtime").join("events"),
        Arc::new(DefaultRedactor::default()),
    )
    .await
    .map_err(|error| runtime_init_failed(format!("event store initialization failed: {error}")))?;
    let mcp_server_store = DesktopMcpServerStore::new(workspace_root.to_path_buf());
    let mcp_config = mcp_config_from_records(
        mcp_server_store.load_records()?,
        SessionId::new(),
        AgentId::new(),
    )
    .await?;

    Harness::builder()
        .with_workspace_root(workspace_root)
        .with_model(LocalLlamaProvider::default())
        .with_store(event_store)
        .with_sandbox(LocalSandbox::new(workspace_root))
        .with_mcp_config(mcp_config)
        .with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))
        .with_stream_permission_broker_arc(
            stream_permission_runtime.broker(),
            stream_permission_runtime.resolver_handle(),
        )
        .build()
        .await
        .map_err(|error| runtime_init_failed(format!("harness initialization failed: {error}")))
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
    pub last_message_preview: Option<String>,
    pub title: String,
    pub updated_at: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessagePayload {
    pub author: &'static str,
    pub body: String,
    pub id: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPayload {
    pub id: String,
    pub messages: Vec<ConversationMessagePayload>,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListConversationsResponse {
    pub conversations: Vec<ConversationSummaryPayload>,
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
pub struct StartRunRequest {
    pub context_references: Option<Vec<String>>,
    pub conversation_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunResponse {
    pub run_id: String,
    pub status: &'static str,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventPayload {
    pub id: String,
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
    pub command: Option<PermissionCommandRunEventPayload>,
    pub decision_scope: String,
    pub exposure: String,
    pub operation: String,
    pub reason: String,
    pub request_id: String,
    pub severity: &'static str,
    pub target: String,
    pub workspace_boundary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCommandRunEventPayload {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub executable: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
    pub source_run_id: String,
    pub status: &'static str,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArtifactsResponse {
    pub artifacts: Vec<ArtifactSummaryPayload>,
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

#[must_use]
pub fn list_eval_cases_payload() -> ListEvalCasesResponse {
    ListEvalCasesResponse {
        cases: vec![regression_smoke_eval_case(3)],
    }
}

#[must_use]
pub fn list_artifacts_payload() -> ListArtifactsResponse {
    ListArtifactsResponse {
        artifacts: vec![ArtifactSummaryPayload {
            action_label: "Open".to_owned(),
            description: "Generated implementation plan and app shell review output.".to_owned(),
            id: "artifact-foundation-plan".to_owned(),
            kind: "markdown".to_owned(),
            preview: Some(
                "# Foundation review\n\n- Conversation workspace restored.\n- Activity rail connected.\n- Support surfaces available from navigation."
                    .to_owned(),
            ),
            source_message_id: Some("message-002".to_owned()),
            source_run_id: "run-001".to_owned(),
            status: "ready",
            title: "Foundation implementation review".to_owned(),
        }],
    }
}

pub async fn list_artifacts_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing artifacts requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut after_event_id = None;
    let mut artifacts = Vec::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(state.default_conversation_id()),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("artifact read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            if let Event::AssistantMessageCompleted(event) = envelope.payload {
                let preview = message_content_display(&event.content, &redactor);
                if preview.trim().is_empty() {
                    continue;
                }
                let preview = truncate_utf8(preview, MAX_ARTIFACT_PREVIEW_BYTES);
                let title = artifact_title_from_preview(&preview);
                artifacts.push(ArtifactSummaryPayload {
                    action_label: "Open".to_owned(),
                    description: "Generated from the runtime conversation.".to_owned(),
                    id: format!("artifact-{}", event.message_id),
                    kind: "markdown".to_owned(),
                    preview: Some(preview),
                    source_message_id: Some(event.message_id.to_string()),
                    source_run_id: event.run_id.to_string(),
                    status: "ready",
                    title,
                });
            }
        }

        after_event_id = page.next_event_id;
    }

    artifacts.reverse();
    Ok(ListArtifactsResponse { artifacts })
}

pub fn run_eval_case_payload(
    request: RunEvalCaseRequest,
) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    ensure_eval_case_id(&request.case_id)?;
    if request.case_id != "regression-smoke" {
        return Err(invalid_payload(format!(
            "unsupported eval case: {}",
            request.case_id
        )));
    }

    Ok(RunEvalCaseResponse {
        case: regression_smoke_eval_case(4),
        status: "completed",
    })
}

pub async fn validate_provider_settings_payload(
    request: ValidateProviderSettingsRequest,
) -> Result<ValidateProviderSettingsResponse, CommandErrorPayload> {
    ensure_provider_metadata(&request.provider_id, &request.model_id)?;
    ensure_provider_model_supported(&request)?;

    Ok(ValidateProviderSettingsResponse {
        model_id: request.model_id,
        provider_id: request.provider_id,
        status: "accepted",
    })
}

pub async fn save_provider_settings_with_store(
    request: ProviderSettingsRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    ensure_provider_settings(&request)?;
    ensure_provider_model_supported(&request)?;
    let secret_ref = store.secret_ref(&request.provider_id);
    let previous_record = store.load_record()?;
    let allowed_stale_secret_ref_prefixes = supported_provider_ids()
        .iter()
        .map(|provider_id| store.secret_ref_prefix(provider_id))
        .collect::<Vec<_>>();
    let stale_secret_refs = stale_provider_secret_refs(
        previous_record.as_ref(),
        &secret_ref,
        &allowed_stale_secret_ref_prefixes,
    );
    let record = ProviderSettingsRecord {
        model_id: request.model_id.clone(),
        provider_id: request.provider_id.clone(),
        secret_ref: secret_ref.clone(),
        stale_secret_refs,
    };
    store.save_secret(&secret_ref, &request.api_key)?;
    if let Err(save_record_error) = store.save_record(&record) {
        if let Err(rollback_error) = store.delete_secret(&secret_ref) {
            return Err(runtime_operation_failed(format!(
                "{}; {}",
                save_record_error.message, rollback_error.message
            )));
        }

        return Err(save_record_error);
    }
    let failed_cleanup_refs = cleanup_stale_provider_secrets(store, &record.stale_secret_refs);
    if failed_cleanup_refs != record.stale_secret_refs {
        let cleaned_record = ProviderSettingsRecord {
            stale_secret_refs: failed_cleanup_refs,
            ..record.clone()
        };
        let _ = store.save_record(&cleaned_record);
    }

    Ok(SaveProviderSettingsResponse {
        model_id: request.model_id,
        provider_id: request.provider_id,
        secret_ref,
        status: "saved",
    })
}

pub async fn list_mcp_servers_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListMcpServersResponse, CommandErrorPayload> {
    let mut servers = BTreeMap::new();

    for record in state.mcp_server_store.load_records()? {
        servers.insert(record.id.clone(), mcp_server_summary_from_record(&record));
    }

    if let Some(harness) = state.harness() {
        if let Some(config) = harness.mcp_config() {
            for server_id in config.registry.server_ids().await {
                if let Some(summary) =
                    mcp_server_summary_from_registry(&config.registry, &server_id).await
                {
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
    let server =
        register_mcp_record_with_harness(&record, &harness, state.default_conversation_id).await?;

    Ok(SaveMcpServerResponse { server })
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
) -> Result<McpConfig, CommandErrorPayload> {
    let registry = McpRegistry::new();
    let mut server_ids_to_inject = Vec::new();

    for record in records {
        ensure_mcp_server_record(&record)?;
        let server_id = register_mcp_record_with_registry(
            &record,
            &registry,
            default_session_id,
            default_agent_id,
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
) -> Result<McpServerSummaryPayload, CommandErrorPayload> {
    let Some(config) = harness.mcp_config() else {
        return Ok(mcp_server_summary_from_record(record));
    };
    let server_id = register_mcp_record_with_registry(
        record,
        &config.registry,
        default_session_id,
        AgentId::new(),
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
) -> Result<McpServerId, CommandErrorPayload> {
    let spec = mcp_server_spec_from_record(record)?;
    let server_id = spec.server_id.clone();
    let scope = mcp_server_scope_from_record(record, default_session_id, default_agent_id)?;
    match registry
        .add_managed_server(
            spec.clone(),
            scope.clone(),
            Arc::new(StdioTransport::new()),
            Arc::new(DesktopMcpEventSink),
        )
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
) -> Result<McpServerSpec, CommandErrorPayload> {
    match &record.transport {
        McpServerTransportConfig::Stdio { command, args } => Ok(McpServerSpec::new(
            McpServerId(record.id.clone()),
            record.display_name.clone(),
            TransportChoice::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: StdioEnv::default(),
                policy: StdioPolicy::default(),
            },
            McpServerSource::Workspace,
        )),
        McpServerTransportConfig::InProcess => Err(invalid_payload(
            "transport.kind must be stdio for workspace MCP servers".to_owned(),
        )),
    }
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

struct DesktopMcpEventSink;

impl McpEventSink for DesktopMcpEventSink {
    fn emit(&self, _event: Event) {}
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
        exposed_tool_count: exposed_tool_count.try_into().unwrap_or(u32::MAX),
        id: server_id.0.clone(),
        last_error,
        origin: mcp_server_origin_payload(&spec.source),
        scope: mcp_server_scope_payload(&scope),
        status,
        transport: mcp_transport_payload(&spec.transport),
    })
}

fn mcp_server_summary_from_record(record: &McpServerConfigRecord) -> McpServerSummaryPayload {
    McpServerSummaryPayload {
        display_name: record.display_name.clone(),
        exposed_tool_count: 0,
        id: record.id.clone(),
        last_error: None,
        origin: "workspace",
        scope: record.scope.clone(),
        status: "configured",
        transport: mcp_transport_config_payload(&record.transport),
    }
}

fn stale_provider_secret_refs(
    previous_record: Option<&ProviderSettingsRecord>,
    next_secret_ref: &str,
    allowed_prefixes: &[String],
) -> Vec<String> {
    let Some(previous_record) = previous_record else {
        return Vec::new();
    };

    let mut stale_secret_refs = previous_record
        .stale_secret_refs
        .iter()
        .filter(|secret_ref| provider_secret_ref_has_allowed_prefix(secret_ref, allowed_prefixes))
        .cloned()
        .collect::<Vec<_>>();
    if previous_record.secret_ref != next_secret_ref {
        stale_secret_refs.push(previous_record.secret_ref.clone());
    }
    stale_secret_refs
        .retain(|secret_ref| provider_secret_ref_has_allowed_prefix(secret_ref, allowed_prefixes));
    stale_secret_refs.sort();
    stale_secret_refs.dedup();
    stale_secret_refs
}

fn cleanup_stale_provider_secrets(
    store: &dyn ProviderSettingsStore,
    stale_secret_refs: &[String],
) -> Vec<String> {
    stale_secret_refs
        .iter()
        .filter_map(|secret_ref| {
            store
                .delete_secret(secret_ref)
                .err()
                .map(|_| secret_ref.clone())
        })
        .collect()
}

#[must_use]
pub fn list_conversations_payload() -> ListConversationsResponse {
    ListConversationsResponse {
        conversations: vec![ConversationSummaryPayload {
            id: "conversation-placeholder".to_owned(),
            last_message_preview: Some(
                "Runtime conversation history is not connected yet.".to_owned(),
            ),
            title: "Build the desktop foundation".to_owned(),
            updated_at: PLACEHOLDER_TIMESTAMP,
        }],
    }
}

#[must_use]
pub fn list_conversations_with_runtime_state(
    state: &DesktopRuntimeState,
) -> ListConversationsResponse {
    ListConversationsResponse {
        conversations: vec![ConversationSummaryPayload {
            id: state.default_conversation_id().to_string(),
            last_message_preview: Some("Runtime conversation is ready.".to_owned()),
            title: "Build the desktop foundation".to_owned(),
            updated_at: PLACEHOLDER_TIMESTAMP,
        }],
    }
}

pub fn get_conversation_payload(
    request: GetConversationRequest,
) -> Result<GetConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;

    Ok(GetConversationResponse {
        conversation: ConversationPayload {
            id: request.conversation_id,
            messages: Vec::new(),
            title: "Build the desktop foundation".to_owned(),
            updated_at: PLACEHOLDER_TIMESTAMP.to_owned(),
        },
    })
}

pub async fn get_conversation_with_runtime_state(
    request: GetConversationRequest,
    state: &DesktopRuntimeState,
) -> Result<GetConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let messages = read_conversation_messages(session_id, state).await?;
    let updated_at = messages
        .last()
        .map(|message| message.timestamp.clone())
        .unwrap_or_else(|| PLACEHOLDER_TIMESTAMP.to_owned());

    Ok(GetConversationResponse {
        conversation: ConversationPayload {
            id: request.conversation_id,
            messages,
            title: "Build the desktop foundation".to_owned(),
            updated_at,
        },
    })
}

pub fn start_run_payload(
    request: StartRunRequest,
) -> Result<StartRunResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;
    ensure_non_empty("prompt", &request.prompt)?;
    ensure_optional_values("contextReferences", request.context_references.as_deref())?;

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
    ensure_optional_values("contextReferences", request.context_references.as_deref())?;

    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Starting runs requires the runtime conversation facade.",
        ));
    };
    let _start_run_guard = state.start_run_lock.lock().await;
    let options = state.conversation_session_options(session_id);
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
                prompt: request.prompt,
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
    ensure_non_empty("requestId", &request.request_id)?;

    let request_id = parse_request_id(&request.request_id)?;
    let decision = to_harness_decision(request.decision);
    let Some(resolver) = state.permission_resolver.as_ref() else {
        return Err(runtime_unavailable(
            "Permission decisions require the runtime PermissionBroker.",
        ));
    };

    resolver.resolve_permission(request_id, decision).await?;

    Ok(ResolvePermissionResponse {
        decision: request.decision,
        request_id: request.request_id,
        status: "resolved",
    })
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

    let mut pending_requests = state.pending_permission_requests();
    pending_requests.sort_by_key(|pending| {
        (
            pending.request.created_at,
            pending.request.request_id.to_string(),
        )
    });
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing runtime activity requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut events = read_activity_replay_events(&request, state).await?;
    for permission_request in pending_requests.iter().filter(|permission_request| {
        permission_request_matches_activity_request(permission_request, &request)
    }) {
        if events.iter().any(|event| {
            event_has_permission_request_id(event, &permission_request.request.request_id)
        }) {
            continue;
        }

        if let Some(permission_event) =
            durable_permission_requested_event(&harness, state, permission_request).await?
        {
            events.push(permission_requested_run_event(
                &permission_event,
                events.len() as u64,
                &redactor,
            ));
        }
    }

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

pub fn get_context_snapshot_payload(
    request: GetContextSnapshotRequest,
) -> Result<GetContextSnapshotResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;

    Ok(GetContextSnapshotResponse {
        active_artifact: None,
        decisions: Vec::new(),
        files: Vec::new(),
        next_actions: vec!["Connect the Rust runtime facade".to_owned()],
        path: "workspace://local".to_owned(),
        project: "Local workspace".to_owned(),
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
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading context snapshot requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut after_event_id = None;
    let mut active_artifact = None;
    let mut next_actions = Vec::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("context snapshot read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            if let Event::AssistantMessageCompleted(event) = envelope.payload {
                if run_id
                    .as_ref()
                    .is_some_and(|run_id| event.run_id != *run_id)
                {
                    continue;
                }
                let preview = message_content_display(&event.content, &redactor);
                if preview.trim().is_empty() {
                    continue;
                }
                let title = artifact_title_from_preview(&preview);
                active_artifact = Some(title);
            }
        }

        after_event_id = page.next_event_id;
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
        path: redacted_display(state.workspace_root().display().to_string(), &redactor),
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

async fn durable_permission_requested_event(
    harness: &Harness,
    state: &DesktopRuntimeState,
    pending: &PendingPermissionRequest,
) -> Result<Option<Event>, CommandErrorPayload> {
    let options = state.conversation_session_options(pending.request.session_id);
    if pending.request.tenant_id != options.tenant_id {
        return Ok(None);
    }
    let Some(pending_run_id) = pending.context.run_id else {
        return Ok(None);
    };

    let mut after_event_id = None;
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
            if let Event::PermissionRequested(requested) = &envelope.payload {
                if requested.request_id == pending.request.request_id
                    && requested.session_id == pending.request.session_id
                    && requested.tenant_id == pending.request.tenant_id
                    && requested.run_id == pending_run_id
                {
                    return Ok(Some(envelope.payload.clone()));
                }
            }
        }

        let Some(next_event_id) = page.next_event_id else {
            return Ok(None);
        };
        after_event_id = Some(next_event_id);
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

fn ensure_optional_values(
    field: &'static str,
    values: Option<&[String]>,
) -> Result<(), CommandErrorPayload> {
    if let Some(values) = values {
        for value in values {
            ensure_non_empty(field, value)?;
        }
    }

    Ok(())
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

fn regression_smoke_eval_case(passed: u32) -> EvalCasePayload {
    EvalCasePayload {
        id: "regression-smoke".to_owned(),
        last_run: Some(EvalLastRunPayload {
            completed_at: Some(PLACEHOLDER_TIMESTAMP),
            failed: 0,
            passed,
            status: "passed",
        }),
        title: "Regression smoke".to_owned(),
    }
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
    ensure_provider_metadata(&request.provider_id, &request.model_id)?;
    ensure_non_empty("apiKey", &request.api_key)?;

    Ok(())
}

fn ensure_provider_metadata(provider_id: &str, model_id: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("providerId", provider_id)?;
    ensure_non_empty("modelId", model_id)?;

    if !is_supported_provider(provider_id) {
        return Err(invalid_payload(
            "providerId must be a supported model provider".to_owned(),
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
        McpServerTransportConfig::Stdio { command, args } => {
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
        }
        McpServerTransportConfig::InProcess => {
            return Err(invalid_payload(
                "transport.kind must be stdio for workspace MCP servers".to_owned(),
            ));
        }
    }

    Ok(())
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

fn ensure_provider_model_supported<T: ProviderSettingsMetadata>(
    request: &T,
) -> Result<(), CommandErrorPayload> {
    let provider = provider_from_settings(request)?;
    let provider_id = provider.provider_id().to_owned();
    let supports_model = provider
        .supported_models()
        .into_iter()
        .any(|model| model.provider_id == provider_id && model.model_id == request.model_id());

    if supports_model {
        return Ok(());
    }

    Err(invalid_payload(
        "modelId must be supported by the selected provider".to_owned(),
    ))
}

fn is_supported_provider(provider_id: &str) -> bool {
    supported_provider_ids().contains(&provider_id)
}

fn supported_provider_ids() -> &'static [&'static str] {
    &[
        "anthropic",
        "codex",
        "deepseek",
        "doubao",
        "gemini",
        "local-llama",
        "minimax",
        "openai",
        "openrouter",
        "qwen",
        "zhipu",
    ]
}

#[must_use]
pub fn provider_secret_ref(workspace_root: &Path, provider_id: &str, secret_id: &str) -> String {
    format!(
        "{}{secret_id}",
        provider_secret_ref_prefix(workspace_root, provider_id)
    )
}

#[must_use]
pub fn provider_secret_ref_prefix(workspace_root: &Path, provider_id: &str) -> String {
    let workspace_scope = blake3::hash(workspace_root.to_string_lossy().as_bytes()).to_hex();
    format!("provider/workspace-{workspace_scope}/{provider_id}/")
}

fn provider_secret_ref_has_allowed_prefix(secret_ref: &str, allowed_prefixes: &[String]) -> bool {
    allowed_prefixes
        .iter()
        .any(|prefix| secret_ref.starts_with(prefix))
}

fn provider_from_settings(
    request: &impl ProviderSettingsMetadata,
) -> Result<Box<dyn ModelProvider>, CommandErrorPayload> {
    provider_from_parts(request.provider_id(), String::new())
}

fn provider_from_parts(
    provider_id: &str,
    api_key: String,
) -> Result<Box<dyn ModelProvider>, CommandErrorPayload> {
    let provider: Box<dyn ModelProvider> = match provider_id {
        "anthropic" => Box::new(AnthropicProvider::from_api_key(api_key)),
        "codex" => Box::new(CodexResponsesProvider::from_api_key(api_key)),
        "deepseek" => Box::new(DeepSeekProvider::from_api_key(api_key)),
        "doubao" => Box::new(DoubaoProvider::from_api_key(api_key)),
        "gemini" => Box::new(GeminiProvider::from_api_key(api_key)),
        "local-llama" => Box::new(LocalLlamaProvider::default().with_api_key(api_key)),
        "minimax" => Box::new(MinimaxProvider::from_api_key(api_key)),
        "openai" => Box::new(OpenAiProvider::from_api_key(api_key)),
        "openrouter" => Box::new(OpenRouterProvider::from_api_key(api_key)),
        "qwen" => Box::new(QwenProvider::from_api_key(api_key)),
        "zhipu" => Box::new(ZhipuProvider::from_api_key(api_key)),
        _ => {
            return Err(invalid_payload(
                "providerId must be a supported model provider".to_owned(),
            ));
        }
    };

    Ok(provider)
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

fn to_harness_decision(decision: PermissionDecision) -> Decision {
    match decision {
        PermissionDecision::Approve => Decision::AllowOnce,
        PermissionDecision::Deny => Decision::DenyOnce,
    }
}

fn permission_requested_run_event(
    event: &Event,
    sequence: u64,
    redactor: &dyn Redactor,
) -> RunEventPayload {
    let Event::PermissionRequested(event) = event else {
        unreachable!("permission activity must be built from PermissionRequested events");
    };
    let subject = permission_subject_display(&event.subject, redactor);

    RunEventPayload {
        id: format!("permission-requested-{}", event.request_id),
        payload: serde_json::to_value(PermissionRequestedRunEventPayload {
            command: subject.command,
            decision_scope: redacted_display(decision_scope_display(&event.scope_hint), redactor),
            exposure: subject.exposure,
            operation: subject.operation,
            reason: "The runtime requires approval before continuing.".to_owned(),
            request_id: event.request_id.to_string(),
            severity: severity_display(event.severity),
            target: subject.target,
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
    command: Option<PermissionCommandRunEventPayload>,
    exposure: String,
    operation: String,
    target: String,
}

fn permission_subject_display(
    subject: &PermissionSubject,
    redactor: &dyn Redactor,
) -> PermissionSubjectDisplay {
    match subject {
        PermissionSubject::CommandExec {
            argv, command, cwd, ..
        } => PermissionSubjectDisplay {
            command: Some(PermissionCommandRunEventPayload {
                argv: argv
                    .iter()
                    .map(|arg| redacted_display(arg.clone(), redactor))
                    .collect(),
                cwd: cwd
                    .as_ref()
                    .map(|path| redacted_display(path.display().to_string(), redactor)),
                executable: redacted_display(
                    argv.first().cloned().unwrap_or_else(|| command.clone()),
                    redactor,
                ),
            }),
            exposure: "Can execute a command inside the workspace boundary.".to_owned(),
            operation: "Execute command".to_owned(),
            target: redacted_display(command.clone(), redactor),
        },
        PermissionSubject::ToolInvocation { tool, .. } => PermissionSubjectDisplay {
            command: None,
            exposure: "Can invoke a runtime tool.".to_owned(),
            operation: "Use tool".to_owned(),
            target: redacted_display(tool.clone(), redactor),
        },
        PermissionSubject::FileWrite { path, .. } => PermissionSubjectDisplay {
            command: None,
            exposure: "Can write a file in the workspace.".to_owned(),
            operation: "Write file".to_owned(),
            target: redacted_display(path.display().to_string(), redactor),
        },
        PermissionSubject::FileDelete { path } => PermissionSubjectDisplay {
            command: None,
            exposure: "Can delete a file in the workspace.".to_owned(),
            operation: "Delete file".to_owned(),
            target: redacted_display(path.display().to_string(), redactor),
        },
        PermissionSubject::NetworkAccess { host, port } => PermissionSubjectDisplay {
            command: None,
            exposure: "Can access a network endpoint.".to_owned(),
            operation: "Access network".to_owned(),
            target: redacted_display(
                port.map_or_else(|| host.clone(), |port| format!("{host}:{port}")),
                redactor,
            ),
        },
        PermissionSubject::DangerousCommand { command, .. } => PermissionSubjectDisplay {
            command: Some(PermissionCommandRunEventPayload {
                argv: vec![redacted_display(command.clone(), redactor)],
                cwd: None,
                executable: redacted_display(command.clone(), redactor),
            }),
            exposure: "Can execute a dangerous command.".to_owned(),
            operation: "Execute dangerous command".to_owned(),
            target: redacted_display(command.clone(), redactor),
        },
        PermissionSubject::McpToolCall { server, tool, .. } => PermissionSubjectDisplay {
            command: None,
            exposure: "Can invoke an MCP tool.".to_owned(),
            operation: "Use MCP tool".to_owned(),
            target: redacted_display(format!("{server}/{tool}"), redactor),
        },
        PermissionSubject::Custom { kind, .. } => PermissionSubjectDisplay {
            command: None,
            exposure: "Can perform a custom permission-gated operation.".to_owned(),
            operation: "Review custom operation".to_owned(),
            target: redacted_display(kind.clone(), redactor),
        },
        _ => PermissionSubjectDisplay {
            command: None,
            exposure: "Can continue a permission-gated operation.".to_owned(),
            operation: "Review permission".to_owned(),
            target: "runtime operation".to_owned(),
        },
    }
}

fn decision_scope_display(scope: &DecisionScope) -> String {
    match scope {
        DecisionScope::ExactCommand { command, cwd } => cwd.as_ref().map_or_else(
            || format!("exact command: {command}"),
            |cwd| format!("exact command: {command} in {}", cwd.display()),
        ),
        DecisionScope::ExactArgs(value) => format!("exact args: {value}"),
        DecisionScope::ToolName(tool) => format!("tool: {tool}"),
        DecisionScope::Category(category) => format!("category: {category}"),
        DecisionScope::PathPrefix(path) => format!("path prefix: {}", path.display()),
        DecisionScope::GlobPattern(pattern) => format!("glob: {pattern}"),
        DecisionScope::ExecuteCodeScript { .. } => "execute code script".to_owned(),
        DecisionScope::Any => "any matching operation".to_owned(),
        _ => "current operation".to_owned(),
    }
}

fn pending_permission_run_id(pending: &PendingPermissionRequest) -> Option<String> {
    pending.context.run_id.map(|run_id| run_id.to_string())
}

fn permission_request_conversation_id(request: &PermissionRequest) -> String {
    request.session_id.to_string()
}

fn permission_request_matches_activity_request(
    pending_permission_request: &PendingPermissionRequest,
    activity_request: &ListActivityRequest,
) -> bool {
    let permission_request = &pending_permission_request.request;
    let matches_run_id = activity_request.run_id.as_deref().map(|run_id| {
        pending_permission_run_id(pending_permission_request)
            .map(|permission_run_id| run_id == permission_run_id)
            .unwrap_or(false)
    });
    let matches_conversation_id =
        activity_request
            .conversation_id
            .as_deref()
            .map(|conversation_id| {
                conversation_id == permission_request_conversation_id(permission_request)
            });

    match (matches_conversation_id, matches_run_id) {
        (Some(matches_conversation_id), Some(matches_run_id)) => {
            matches_conversation_id && matches_run_id
        }
        (Some(matches_conversation_id), None) => matches_conversation_id,
        (None, Some(matches_run_id)) => matches_run_id,
        (None, None) => false,
    }
}

async fn read_replay_run_events(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
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
    let mut after_event_id = None;
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
            let Some(event) =
                mapper.map(envelope.event_id.to_string(), envelope.payload, &redactor)
            else {
                continue;
            };
            if run_id
                .as_ref()
                .is_some_and(|run_id| event.run_id != run_id.to_string())
            {
                continue;
            }
            events.push(RunEventPayload {
                sequence: events.len() as u64,
                ..event
            });
        }

        after_event_id = page.next_event_id;
    }

    Ok(events)
}

async fn read_activity_replay_events(
    request: &ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    read_replay_run_events(request.clone(), state).await
}

async fn read_conversation_messages(
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<Vec<ConversationMessagePayload>, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversation messages requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut after_event_id = None;
    let mut messages = Vec::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation read failed: {error}"))
            })?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            match envelope.payload {
                Event::UserMessageAppended(event) => {
                    messages.push(ConversationMessagePayload {
                        author: "user",
                        body: message_content_display(&event.content, &redactor),
                        id: event.message_id.to_string(),
                        timestamp: event.at.to_rfc3339(),
                    });
                }
                Event::AssistantMessageCompleted(event) => {
                    messages.push(ConversationMessagePayload {
                        author: "assistant",
                        body: message_content_display(&event.content, &redactor),
                        id: event.message_id.to_string(),
                        timestamp: event.at.to_rfc3339(),
                    });
                }
                _ => {}
            }
        }

        after_event_id = page.next_event_id;
    }

    Ok(messages)
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

    redacted_display(value, redactor)
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

fn artifact_title_from_preview(preview: &str) -> String {
    let title = preview
        .lines()
        .find_map(|line| {
            let line = line.trim().trim_start_matches('#').trim();
            (!line.is_empty()).then(|| line.to_owned())
        })
        .unwrap_or_else(|| "Generated artifact".to_owned());

    truncate_utf8(title, 120)
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
            title: format!(
                "Approve {}",
                redacted_display(pending.request.tool_name, redactor)
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

fn workspace_project_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Local workspace")
        .to_owned()
}

fn event_has_permission_request_id(event: &RunEventPayload, request_id: &RequestId) -> bool {
    event
        .payload
        .get("requestId")
        .and_then(Value::as_str)
        .is_some_and(|value| value == request_id.to_string())
}

#[derive(Default)]
struct RunEventMapper {
    permission_run_ids: HashMap<RequestId, RunId>,
    tool_run_ids: HashMap<ToolUseId, RunId>,
}

impl RunEventMapper {
    fn map(
        &mut self,
        event_id: String,
        event: Event,
        redactor: &dyn Redactor,
    ) -> Option<RunEventPayload> {
        match event {
            Event::RunStarted(event) => Some(RunEventPayload {
                id: event_id,
                payload: json!({ "sessionId": event.session_id.to_string() }),
                run_id: event.run_id.to_string(),
                sequence: 0,
                source: "engine",
                timestamp: event.started_at.to_rfc3339(),
                event_type: "run.started",
                visibility: "public",
            }),
            Event::RunEnded(event) => {
                let mut payload = json!({ "reason": event.reason });
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
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.ended_at.to_rfc3339(),
                    event_type: "run.ended",
                    visibility: "public",
                })
            }
            Event::AssistantDeltaProduced(event) => Some(RunEventPayload {
                id: event_id,
                payload: json!({ "text": assistant_delta_text(event.delta, redactor) }),
                run_id: event.run_id.to_string(),
                sequence: 0,
                source: "assistant",
                timestamp: event.at.to_rfc3339(),
                event_type: "assistant.delta",
                visibility: "public",
            }),
            Event::AssistantMessageCompleted(event) => Some(RunEventPayload {
                id: event_id,
                payload: json!({ "messageId": event.message_id.to_string() }),
                run_id: event.run_id.to_string(),
                sequence: 0,
                source: "assistant",
                timestamp: event.at.to_rfc3339(),
                event_type: "assistant.completed",
                visibility: "public",
            }),
            Event::ToolUseRequested(event) => {
                self.tool_run_ids.insert(event.tool_use_id, event.run_id);
                Some(RunEventPayload {
                    id: event_id,
                    payload: json!({
                        "argumentsSummary": redacted_display(event.input.to_string(), redactor),
                        "toolName": redacted_display(event.tool_name, redactor),
                        "toolUseId": event.tool_use_id.to_string(),
                    }),
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
                    payload: json!({
                        "durationMs": event.duration_ms,
                        "outputSummary": tool_result_summary(event.result, redactor),
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
                    payload: json!({
                        "code": redacted_display(event.error.code, redactor),
                        "message": redacted_display(event.error.message, redactor),
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
                self.permission_run_ids.insert(event.request_id, event.run_id);
                Some(permission_requested_run_event(
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
            Event::EngineFailed(event) => event.run_id.map(|run_id| RunEventPayload {
                id: event_id,
                payload: json!({ "message": redacted_display(format!("{:?}", event.error), redactor) }),
                run_id: run_id.to_string(),
                sequence: 0,
                source: "engine",
                timestamp: event.at.to_rfc3339(),
                event_type: "engine.failed",
                visibility: "redacted",
            }),
            _ => None,
        }
    }
}

fn assistant_delta_text(delta: impl Serialize, redactor: &dyn Redactor) -> String {
    let summary = serde_json::to_string(&delta).unwrap_or_default();
    redacted_display(summary, redactor)
}

fn tool_result_summary(result: impl Serialize, redactor: &dyn Redactor) -> String {
    let summary = serde_json::to_string(&result).unwrap_or_default();
    redacted_display(summary, redactor)
}

fn permission_decision_payload(decision: Decision) -> &'static str {
    match decision {
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => "approve",
        Decision::DenyOnce | Decision::DenyPermanent | Decision::Escalate => "deny",
        _ => "deny",
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
    api_key: String,
    model_id: String,
    provider_id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    let request = ProviderSettingsRequest {
        api_key,
        model_id,
        provider_id,
    };
    save_provider_settings_with_store(request, runtime_state.provider_settings_store.as_ref()).await
}

#[tauri::command]
pub async fn list_mcp_servers(
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ListMcpServersResponse, CommandErrorPayload> {
    list_mcp_servers_with_runtime_state(runtime_state.inner()).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_mcp_server(
    display_name: String,
    id: String,
    scope: String,
    transport: McpServerTransportConfig,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            display_name,
            id,
            scope,
            transport,
        },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_mcp_server(
    id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<DeleteMcpServerResponse, CommandErrorPayload> {
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    delete_mcp_server_with_runtime_state(DeleteMcpServerRequest { id }, runtime_state.inner()).await
}

#[tauri::command]
pub async fn list_memory_items(
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ListMemoryItemsResponse, CommandErrorPayload> {
    list_memory_items_with_runtime_state(runtime_state.inner()).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_memory_item(
    id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<GetMemoryItemResponse, CommandErrorPayload> {
    get_memory_item_with_runtime_state(GetMemoryItemRequest { id }, runtime_state.inner()).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_memory_item(
    content: String,
    id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload> {
    let _memory_guard = runtime_state.memory_lock.lock().await;
    update_memory_item_with_runtime_state(
        UpdateMemoryItemRequest { content, id },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_memory_item(
    id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<DeleteMemoryItemResponse, CommandErrorPayload> {
    let _memory_guard = runtime_state.memory_lock.lock().await;
    delete_memory_item_with_runtime_state(DeleteMemoryItemRequest { id }, runtime_state.inner())
        .await
}

#[tauri::command]
pub async fn export_memory_items(
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
    let _memory_guard = runtime_state.memory_lock.lock().await;
    export_memory_items_with_runtime_state(runtime_state.inner()).await
}

#[tauri::command]
pub fn list_conversations(
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> ListConversationsResponse {
    list_conversations_with_runtime_state(runtime_state.inner())
}

#[tauri::command]
pub fn list_eval_cases() -> ListEvalCasesResponse {
    list_eval_cases_payload()
}

#[tauri::command]
pub async fn list_artifacts(
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    list_artifacts_with_runtime_state(runtime_state.inner()).await
}

#[tauri::command(rename_all = "camelCase")]
pub fn run_eval_case(case_id: String) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    run_eval_case_payload(RunEvalCaseRequest { case_id })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_conversation(
    conversation_id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<GetConversationResponse, CommandErrorPayload> {
    get_conversation_with_runtime_state(
        GetConversationRequest { conversation_id },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn start_run(
    context_references: Option<Vec<String>>,
    conversation_id: String,
    prompt: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<StartRunResponse, CommandErrorPayload> {
    start_run_with_runtime_state(
        StartRunRequest {
            context_references,
            conversation_id,
            prompt,
        },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn cancel_run(
    run_id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<CancelRunResponse, CommandErrorPayload> {
    cancel_run_with_runtime_state(CancelRunRequest { run_id }, runtime_state.inner()).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn resolve_permission(
    decision: PermissionDecision,
    request_id: String,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            decision,
            request_id,
        },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_activity(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ListActivityResponse, CommandErrorPayload> {
    list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id,
            run_id,
        },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_replay_timeline(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ReplayTimelineResponse, CommandErrorPayload> {
    get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id,
            run_id,
        },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn export_support_bundle(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<ExportSupportBundleResponse, CommandErrorPayload> {
    export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id,
            run_id,
        },
        runtime_state.inner(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_context_snapshot(
    conversation_id: Option<String>,
    run_id: Option<String>,
    runtime_state: tauri::State<'_, DesktopRuntimeState>,
) -> Result<GetContextSnapshotResponse, CommandErrorPayload> {
    get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id,
            run_id,
        },
        runtime_state.inner(),
    )
    .await
}
