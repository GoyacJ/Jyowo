use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Cursor;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use chrono::{DateTime, NaiveDate, Utc};
use futures::{future::BoxFuture, stream::BoxStream, StreamExt};
use harness_contracts::{
    validate_agent_profile, validate_provider_capability_route, AgentCapabilityUnavailableReason,
    AgentProfile, AgentProfileScope, ApproveMemoryCandidateRequest, ApproveMemoryCandidateResponse,
    AutomationRunRecord, AutomationSpec, AutomationWorkspaceScope, BackgroundAgentState,
    CapabilityRouteKind, ConversationInspectorSelection, DiagnosticsRawOutput,
    DiagnosticsRunRequest, DiagnosticsRunnerCap, DiagnosticsRunnerKind,
    GetMemoryRecallTraceRequest, GetMemoryRecallTraceResponse, GetMemorySettingsRequest,
    GetMemorySettingsResponse, GetModelRequestPreviewRequest, GetModelRequestPreviewResponse,
    GetThreadMemorySettingsRequest, GetThreadMemorySettingsResponse, ListMemoryCandidatesRequest,
    ListMemoryCandidatesResponse, ListMemoryRecallTracesRequest, ListMemoryRecallTracesResponse,
    ListProviderCapabilityRouteOptionsResponse, LocalIsolationTag, MergeMemoryCandidateRequest,
    MergeMemoryCandidateResponse, MissedRunPolicy, PluginConfigUpdate, PluginDetail, PluginId,
    PluginInstallReport, PluginOperationResult, PluginOperationStatus, PluginSummary,
    ProviderCapabilityRoute, ProviderCapabilityRouteOption, ProviderCapabilityRouteSettings,
    ProviderProbeSnapshot, ProviderServiceAdapterAvailability, RejectMemoryCandidateRequest,
    RejectMemoryCandidateResponse, RejectionReason, SandboxMode, TrustLevel,
    UpdateMemorySettingsRequest, UpdateMemorySettingsResponse, UpdateThreadMemorySettingsRequest,
    UpdateThreadMemorySettingsResponse, WorkspaceAccess,
};
use harness_model::ModelRuntimeSemantics;
use harness_plugin::{
    CargoExtensionManifestLoader, CargoExtensionRuntimeLoader, DiscoverySource, FileManifestLoader,
    InlineManifestLoader, ManifestOrigin, PluginConfig, PluginName, PluginRegistry,
};
use harness_sandbox::{
    execute_with_lifecycle, EventSink, ExecContext, ExecSpec, LocalIsolation, SandboxBackend,
    StdioSpec,
};
use harness_tool::{
    provider_service_adapter_availability_from_snapshot, BuiltinToolset, ToolRegistryBuilder,
};
use image::{ImageFormat, ImageReader, Limits};
use jyowo_harness_sdk::builtin::{
    DefaultRedactor, FileBlobStore, LocalLlamaProvider, LocalSandbox,
};
use jyowo_harness_sdk::ext::inventory_from_models_api_json;
use jyowo_harness_sdk::ext::{
    build_provider, now, provider_catalog_entries, resolve_model_descriptor,
    runnable_inventory_models, AgentId, BlobRef, BlobRetention, BlobStore,
    ConversationModelCapability, DirectorySourceKind, Event, EventId, EventStore, FallbackPolicy,
    HttpTransport, InteractivityLevel, McpAuthorizationContext, McpConnectContext,
    McpConnectionState, McpEventSink, McpRegistry, McpServerId, McpServerScope, McpServerSource,
    McpServerSpec, MemoryId, MemoryKind, MemoryRecord, MemorySource, MemorySummary,
    MemoryVisibility, ModelDescriptor, ModelInventoryEntry, ModelLifecycle, ModelModality,
    ModelProtocol, ModelProvider, ModelRuntimeStatus, PermissionMode, ProviderBaseUrlRegion,
    ProviderBuildConfig, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ProviderProbeInput, ProviderProbeRunner, ProviderRegistryError,
    ProviderRequestDefaults, ProviderRuntimeCapability, ProviderServiceCapability,
    ProviderServiceCategory, ProviderServiceCostRisk, ProviderServiceExecution, RunId, SessionId,
    SkillLoader, SkillSourceConfig, StdioEnv, StdioPolicy, StdioTransport, TenantId,
    ToolCapability, ToolError, ToolProfile, TransportChoice,
};
use jyowo_harness_sdk::{
    DesktopSettingsRuntime, McpConfig, RuntimeSkillSummary, RuntimeSkillView, SessionOptions,
};
use parking_lot::{Mutex as ParkingMutex, RwLock as ParkingRwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::project_registry::ProjectRegistry;
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

mod agents;
mod app;
#[allow(dead_code)]
mod artifacts;
mod automations;
#[allow(dead_code)]
mod constants;
mod contracts;
mod daemon;
mod error;
mod evals;
mod mcp;
mod memory;
mod memory_settings;
mod model_settings;
mod plugins;
mod projects;
mod providers;
mod runtime;
mod runtime_tools;
mod skills;
pub mod stores;
#[allow(dead_code)]
mod support;
#[cfg(test)]
mod tests;
mod validation;

pub use daemon::*;
use error::invalid_payload;
use harness_contracts::AgentCapabilityKind;
use providers::{
    provider_capability_route_runtime_context, save_provider_settings_with_runtime_state_unlocked,
    sync_runtime_provider_capability_routes,
};
pub(crate) use runtime::build_desktop_settings_runtime;
pub(crate) use support::*;
use validation::{ensure_non_empty, ensure_provider_settings};

pub use agents::{
    delete_agent_profile_with_runtime_state, list_agent_profiles_with_runtime_state,
    save_agent_profile_with_runtime_state,
};
pub use app::{
    get_app_info_payload, harness_healthcheck_payload, list_eval_cases_payload,
    list_eval_cases_with_runtime_state,
};
pub use automations::{
    delete_automation_with_runtime_state, list_automation_runs_with_runtime_state,
    list_automations_with_runtime_state, save_automation_with_runtime_state,
    set_automation_enabled_with_runtime_state,
};
pub use contracts::{
    AppInfoPayload, ArtifactRevisionPayload, ArtifactSummaryPayload, AttachmentBlobRefPayload,
    AttachmentReferencePayload, AutomationStore, BackgroundAgentActionResponse,
    BackgroundAgentDeleteResponse, BackgroundAgentIdRequest, BackgroundAgentPayload,
    BrowserMcpPresetId, BrowserMcpPresetSummaryPayload, CancelRunRequest, CancelRunResponse,
    ClearMcpDiagnosticsRequest, ClearMcpDiagnosticsResponse, ContextDecisionPayload,
    ContextFilePayload, ContextReferencePayload, ConversationMessagePayload,
    ConversationMetadataFile, ConversationMetadataRecord, ConversationMetadataState,
    ConversationMetadataStore, ConversationModelCapabilityRecord, ConversationPayload,
    ConversationSummaryPayload, CreateAttachmentFromPathRequest, CreateAttachmentFromPathResponse,
    CreateConversationResponse, DeleteAgentProfileRequest, DeleteAgentProfileResponse,
    DeleteAutomationRequest, DeleteAutomationResponse, DeleteConversationRequest,
    DeleteConversationResponse, DeleteMcpServerRequest, DeleteMcpServerResponse,
    DeleteMemoryItemRequest, DeleteMemoryItemResponse, DeleteProviderCapabilityRouteRequest,
    DeleteProviderCapabilityRouteResponse, DeleteSkillRequest, DeleteSkillResponse,
    EvalCasePayload, EvalLastRunPayload, ExportConversationEvidenceRequest,
    ExportConversationEvidenceResponse, ExportMemoryItemsFormat, ExportMemoryItemsRequest,
    ExportMemoryItemsResponse, ExportMemoryItemsScope, ExportSupportBundleRequest,
    ExportSupportBundleResponse, GetArtifactMediaPreviewRequest, GetArtifactMediaPreviewResponse,
    GetArtifactRevisionContentRequest, GetArtifactRevisionContentResponse,
    GetAttachmentMediaPreviewRequest, GetAttachmentMediaPreviewResponse, GetBackgroundAgentRequest,
    GetBackgroundAgentResponse, GetContextSnapshotRequest, GetContextSnapshotResponse,
    GetConversationCommandOutputRequest, GetConversationCommandOutputResponse,
    GetConversationDiffPatchRequest, GetConversationDiffPatchResponse,
    GetConversationInspectorItemRequest, GetConversationRequest, GetConversationResponse,
    GetExecutionSettingsRequest, GetExecutionSettingsResponse, GetMcpServerConfigRequest,
    GetMcpServerConfigResponse, GetMemoryItemRequest, GetMemoryItemResponse,
    GetModelUsageSummaryResponse, GetPluginDetailRequest, GetPluginDetailResponse,
    GetProviderConfigApiKeyRequest, GetProviderConfigApiKeyResponse, GetSkillDetailRequest,
    GetSkillDetailResponse, GetSkillFileRequest, GetSkillFileResponse, HarnessHealthcheckPayload,
    HarnessInfoPayload, ImportSkillRequest, ImportSkillResponse, InstallPluginFromPathRequest,
    InstallSkillFromCatalogResponse, ListActivityRequest, ListActivityResponse,
    ListAgentProfilesResponse, ListArtifactsRequest, ListArtifactsResponse,
    ListAutomationRunsRequest, ListAutomationRunsResponse, ListAutomationsResponse,
    ListBackgroundAgentsRequest, ListBackgroundAgentsResponse, ListBrowserMcpPresetsResponse,
    ListConversationsResponse, ListEvalCasesResponse, ListMcpDiagnosticsRequest,
    ListMcpDiagnosticsResponse, ListMcpServersResponse, ListMemoryItemsResponse,
    ListOfficialQuotaSnapshotsResponse, ListPluginsResponse, ListProjectConversationGroupsResponse,
    ListProviderCapabilityRoutesResponse, ListProviderProbeSnapshotsResponse,
    ListProviderSettingsResponse, ListReferenceCandidatesRequest, ListReferenceCandidatesResponse,
    ListRuntimeToolsResponse, ListSkillCatalogInstallTasksResponse, ListSkillsResponse,
    McpDiagnosticBatchEmitter, McpDiagnosticBatchPayload, McpDiagnosticRecord,
    McpDiagnosticSeverity, McpDiagnosticStore, McpHeaderEnvRecord, McpNameValueRecord,
    McpNameValueSaveRecord, McpServerConfigRecord, McpServerConfigTransportPayload, McpServerStore,
    McpServerSummaryPayload, McpServerTransportConfig, MemoryItemPayload, MemoryItemSummaryPayload,
    ModelCatalogEntry, ModelLifecyclePayload, ModelProviderCatalogEntry,
    ModelProviderCatalogResponse, ModelRuntimeStatusPayload, ModelSettingsPageResponse,
    OfficialQuotaScopePayload, OfficialQuotaSnapshotPayload, OfficialQuotaStatusPayload,
    PermissionDecision, PermissionRequestedRunEventPayload, PluginSettingsRecord, PluginStore,
    PluginStoreRecord, ProbeProviderConfigRequest, ProbeProviderConfigResponse,
    ProviderBaseUrlRegionPayload, ProviderCapabilityRouteStore,
    ProviderCapabilityRouteValidationToken, ProviderConfigPayload, ProviderConfigRecord,
    ProviderDefaultsRecord, ProviderDiagnosticsStore, ProviderModelDescriptorRecord,
    ProviderModelLifecycleRecord, ProviderModelModalityRecord, ProviderProbeErrorKindPayload,
    ProviderProbeSnapshotPayload, ProviderProbeStatusPayload, ProviderQuotaCacheRecord,
    ProviderQuotaCacheStore, ProviderRuntimeCapabilityPayload, ProviderServiceCapabilityPayload,
    ProviderSettingsRecord, ProviderSettingsRequest, ProviderSettingsStore,
    ReferenceCandidatePayload, RefreshModelProviderCatalogResponse, RefreshOfficialQuotaRequest,
    RefreshOfficialQuotaResponse, ReloadPluginRequest, ReplayTimelineRequest,
    ReplayTimelineResponse, RequestProviderConfigApiKeyRevealRequest,
    RequestProviderConfigApiKeyRevealResponse, ResolvePermissionRequest, ResolvePermissionResponse,
    RestartMcpServerRequest, RestartMcpServerResponse, RunAutomationNowRequest,
    RunAutomationNowResponse, RunEvalCaseRequest, RunEvalCaseResponse, RunEventBodyPayload,
    RunEventPayload, RuntimeToolServiceBindingSummary, RuntimeToolSummary,
    SaveAgentProfileResponse, SaveAutomationRequest, SaveAutomationResponse,
    SaveBrowserMcpPresetRequest, SaveBrowserMcpPresetResponse, SaveMcpServerRequest,
    SaveMcpServerResponse, SaveMcpServerTransportConfig, SaveProviderCapabilityRouteRequest,
    SaveProviderCapabilityRouteResponse, SaveProviderSettingsResponse,
    SendBackgroundAgentInputRequest, SetAutomationEnabledRequest, SetAutomationEnabledResponse,
    SetExecutionSettingsRequest, SetExecutionSettingsResponse, SetMcpServerEnabledRequest,
    SetMcpServerEnabledResponse, SetPluginEnabledRequest, SetProjectPluginsEnabledRequest,
    SetProjectPluginsEnabledResponse, SetSkillEnabledRequest, SetSkillEnabledResponse,
    SkillCatalogInstallProgressEmitter, SkillCatalogInstallProgressPayload,
    SkillCatalogInstallTaskPayload, SkillDetailPayload, SkillFileContentPayload, SkillFilePayload,
    SkillParameterPayload, SkillStore, SkillStoreRecord, SkillSummaryPayload, StartRunRequest,
    StartRunResponse, SubscribeMcpDiagnosticsRequest, SubscribeMcpDiagnosticsResponse,
    UninstallPluginRequest, UnsubscribeMcpDiagnosticsRequest, UnsubscribeMcpDiagnosticsResponse,
    UpdateMemoryItemRequest, UpdateMemoryItemResponse, UpdatePluginConfigRequest,
    ValidatePluginFromPathRequest, ValidateProviderSettingsRequest,
    ValidateProviderSettingsResponse,
};
pub use error::CommandErrorPayload;
pub use evals::{run_eval_case_payload, run_eval_case_with_runtime_state};
pub use mcp::{
    clear_mcp_diagnostics_with_runtime_state, delete_mcp_server_with_runtime_state,
    delete_mcp_server_with_store, get_mcp_server_config_with_runtime_state,
    get_mcp_server_config_with_store, list_browser_mcp_presets_with_runtime_state,
    list_browser_mcp_presets_with_store, list_mcp_diagnostics_with_runtime_state,
    list_mcp_diagnostics_with_store, list_mcp_servers_with_runtime_state,
    mcp_diagnostic_record_from_event, restart_mcp_server_with_runtime_state,
    save_browser_mcp_preset_with_runtime_state, save_browser_mcp_preset_with_store,
    save_mcp_server_with_runtime_state, save_mcp_server_with_store,
    set_mcp_server_enabled_with_runtime_state,
    subscribe_mcp_diagnostics_for_window_with_runtime_state,
    subscribe_mcp_diagnostics_with_runtime_state,
    unsubscribe_mcp_diagnostics_for_window_with_runtime_state,
    unsubscribe_mcp_diagnostics_with_runtime_state,
};
pub use memory::{
    approve_memory_candidate_with_runtime_state, delete_memory_item_with_runtime_state,
    export_memory_items_with_runtime_state, get_memory_item_with_runtime_state,
    get_memory_recall_trace_with_runtime_state, get_model_request_preview_with_runtime_state,
    list_memory_candidates_with_runtime_state, list_memory_items_with_runtime_state,
    list_memory_recall_traces_with_runtime_state, merge_memory_candidate_with_runtime_state,
    reject_memory_candidate_with_runtime_state, update_memory_item_with_runtime_state,
};
pub use memory_settings::{
    get_memory_settings_with_runtime_state, get_thread_memory_settings_with_runtime_state,
    update_memory_settings_with_runtime_state, update_thread_memory_settings_with_runtime_state,
};
pub use model_settings::{
    get_model_settings_page_with_runtime_state, get_model_usage_summary_with_runtime_state,
    list_official_quota_snapshots_with_runtime_state,
    list_provider_probe_snapshots_with_runtime_state, probe_provider_config_with_provider,
    probe_provider_config_with_runtime_state, refresh_model_provider_catalog_with_runtime_state,
    refresh_official_quota_with_runtime_state,
};
pub use plugins::{
    get_plugin_detail_with_runtime_state, install_plugin_from_path_with_runtime_state,
    list_plugins_with_runtime_state, reload_plugin_with_runtime_state,
    set_plugin_enabled_with_runtime_state, set_project_plugins_enabled_with_runtime_state,
    uninstall_plugin_with_runtime_state, update_plugin_config_with_runtime_state,
    validate_plugin_from_path_with_runtime_state,
};
pub use projects::{
    add_project_payload, delete_project_payload, get_default_workspace_payload,
    list_projects_payload, move_project_payload, rename_project_payload, switch_project_payload,
    DefaultWorkspaceResponse, DeleteProjectResponse, ListProjectsResponse, ProjectMoveDirection,
    SwitchProjectResponse,
};
pub use providers::{
    delete_provider_capability_route_with_store, desktop_provider_credential_resolver_with_stores,
    get_execution_settings_for_request, get_execution_settings_for_state_request,
    get_execution_settings_with_store, get_provider_config_api_key_with_runtime_state,
    get_provider_config_api_key_with_store, list_model_provider_catalog_payload,
    list_model_provider_catalog_payload_with_remote,
    list_provider_capability_route_options_from_inputs, list_provider_capability_routes_with_store,
    list_provider_settings_with_store, request_provider_config_api_key_reveal_with_runtime_state,
    request_provider_config_api_key_reveal_with_store, resolve_effective_execution_settings,
    save_provider_capability_route_settings_with_store, save_provider_capability_route_with_store,
    save_provider_settings_with_runtime_state, save_provider_settings_with_store,
    set_execution_settings_with_store, validate_provider_settings_payload,
    AgentCapabilitiesPayload, DesktopConversationMetadataStore, DesktopExecutionSettingsStore,
    DesktopProviderCapabilityRouteStore, DesktopProviderSettingsStore,
    NoWorkspaceProviderCapabilityRouteStore,
};
pub use runtime::{
    managed_runtime_state, runtime_state, runtime_state_async, runtime_state_for_workspace,
    runtime_state_with_provider_settings_store_for_test, ManagedDesktopRuntime,
};
pub use runtime_tools::*;
pub use skills::{
    delete_skill_with_runtime_state, get_skill_catalog_entry_with_runtime_state,
    get_skill_catalog_file_with_runtime_state, get_skill_detail_with_runtime_state,
    get_skill_file_with_runtime_state, import_skill_with_runtime_state,
    install_skill_from_catalog_package_with_runtime_state,
    install_skill_from_catalog_with_progress, install_skill_from_catalog_with_runtime_state,
    list_skill_catalog_entries_with_runtime_state,
    list_skill_catalog_install_tasks_with_runtime_state,
    list_skill_catalog_sources_with_runtime_state, list_skills_with_runtime_state,
    set_skill_enabled_with_runtime_state, start_skill_catalog_install_task_with_runtime_state,
};
pub use stores::{
    DesktopAutomationStore, DesktopMcpDiagnosticStore, DesktopModelUsageRollupStore,
    DesktopPluginStore, DesktopProviderCatalogSnapshotStore, DesktopProviderDiagnosticsStore,
    DesktopProviderQuotaCacheStore, DesktopRuntimeState, DesktopSkillStore,
};

#[tauri::command]
pub fn get_app_info() -> AppInfoPayload {
    get_app_info_payload()
}

#[tauri::command]
pub fn list_projects(project_registry: tauri::State<'_, ProjectRegistry>) -> ListProjectsResponse {
    list_projects_payload(&project_registry)
}

#[tauri::command]
pub fn get_default_workspace() -> Result<DefaultWorkspaceResponse, CommandErrorPayload> {
    get_default_workspace_payload()
}

#[tauri::command(rename_all = "camelCase")]
pub fn rename_project(
    path: String,
    name: String,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    rename_project_payload(path, name, &project_registry)
}

#[tauri::command(rename_all = "camelCase")]
pub fn move_project(
    path: String,
    direction: ProjectMoveDirection,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<ListProjectsResponse, CommandErrorPayload> {
    move_project_payload(path, direction, &project_registry)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn switch_project(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    switch_project_payload(path, &runtime_handle, &project_registry).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_project(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<DeleteProjectResponse, CommandErrorPayload> {
    delete_project_payload(path, &runtime_handle, &project_registry).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_project(
    path: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
    project_registry: tauri::State<'_, ProjectRegistry>,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    add_project_payload(path, &runtime_handle, &project_registry).await
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
    get_execution_settings_for_state_request(
        GetExecutionSettingsRequest { workspace_path },
        &runtime_state,
        &project_registry,
        None,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_execution_settings(
    permission_mode: PermissionMode,
    tool_profile: ToolProfile,
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
            tool_profile,
            context_compression_trigger_ratio,
            subagents_enabled,
            agent_teams_enabled,
            background_agents_enabled,
        },
        runtime_state.execution_settings_store.as_ref(),
        None,
    )
}

#[tauri::command]
pub async fn list_automations(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListAutomationsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_automations_with_runtime_state(&runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_automation(
    automation: AutomationSpec,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveAutomationResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    save_automation_with_runtime_state(SaveAutomationRequest { automation }, &runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_automation(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteAutomationResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    delete_automation_with_runtime_state(id, &runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_automation_enabled(
    id: String,
    enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SetAutomationEnabledResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    set_automation_enabled_with_runtime_state(
        SetAutomationEnabledRequest { id, enabled },
        &runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_automation_runs(
    automation_id: Option<String>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListAutomationRunsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_automation_runs_with_runtime_state(automation_id, &runtime_state).await
}

#[tauri::command]
pub async fn list_provider_settings(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListProviderSettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_provider_settings_with_store(runtime_state.provider_settings_store.as_ref()).await
}

#[tauri::command]
pub async fn list_provider_capability_routes(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListProviderCapabilityRoutesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let (store, provider_settings, provider_catalog, adapter_availability) =
        provider_capability_route_runtime_context(&runtime_state).await?;
    list_provider_capability_routes_with_store(
        store.as_ref(),
        &provider_settings,
        &provider_catalog,
        &adapter_availability,
    )
    .await
}

#[tauri::command]
pub async fn list_provider_capability_route_options(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListProviderCapabilityRouteOptionsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let (store, provider_settings, provider_catalog, adapter_availability) =
        provider_capability_route_runtime_context(&runtime_state).await?;
    list_provider_capability_route_options_from_inputs(
        store.as_ref(),
        &provider_settings,
        &provider_catalog,
        &adapter_availability,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_provider_capability_route(
    route: ProviderCapabilityRoute,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveProviderCapabilityRouteResponse, CommandErrorPayload> {
    validate_provider_capability_route(&route).map_err(invalid_payload)?;
    let runtime_state = runtime_handle.read().await;
    let (store, provider_settings, provider_catalog, adapter_availability) =
        provider_capability_route_runtime_context(&runtime_state).await?;
    let response = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest { route },
        store.as_ref(),
        &provider_settings,
        &provider_catalog,
        &adapter_availability,
    )
    .await?;
    sync_runtime_provider_capability_routes(
        &runtime_state,
        &ProviderCapabilityRouteSettings {
            version: response.version,
            routes: response.routes.clone(),
        },
    );
    Ok(response)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_provider_capability_route(
    kind: CapabilityRouteKind,
    config_id: String,
    provider_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteProviderCapabilityRouteResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &config_id)?;
    ensure_non_empty("providerId", &provider_id)?;
    let runtime_state = runtime_handle.read().await;
    let (store, provider_settings, provider_catalog, adapter_availability) =
        provider_capability_route_runtime_context(&runtime_state).await?;
    let response = delete_provider_capability_route_with_store(
        DeleteProviderCapabilityRouteRequest {
            kind,
            config_id,
            provider_id,
        },
        store.as_ref(),
        &provider_settings,
        &provider_catalog,
        &adapter_availability,
    )
    .await?;
    sync_runtime_provider_capability_routes(
        &runtime_state,
        &ProviderCapabilityRouteSettings {
            version: response.version,
            routes: response.routes.clone(),
        },
    );
    Ok(response)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn request_provider_config_api_key_reveal(
    config_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &config_id)?;
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
    ensure_non_empty("configId", &config_id)?;
    ensure_non_empty("revealToken", &reveal_token)?;
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
pub async fn probe_provider_config(
    config_id: String,
    timeout_ms: Option<u64>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ProbeProviderConfigResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &config_id)?;
    let runtime_state = runtime_handle.read().await;
    model_settings::probe_provider_config_with_runtime_state(
        ProbeProviderConfigRequest {
            config_id,
            timeout_ms,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_provider_probe_snapshots(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListProviderProbeSnapshotsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    model_settings::list_provider_probe_snapshots_with_runtime_state(&*runtime_state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_model_usage_summary(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetModelUsageSummaryResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    model_settings::get_model_usage_summary_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_model_settings_page(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ModelSettingsPageResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    model_settings::get_model_settings_page_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn refresh_model_provider_catalog(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RefreshModelProviderCatalogResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    model_settings::refresh_model_provider_catalog_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn refresh_official_quota(
    config_id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RefreshOfficialQuotaResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &config_id)?;
    let runtime_state = runtime_handle.read().await;
    model_settings::refresh_official_quota_with_runtime_state(&config_id, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_official_quota_snapshots(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListOfficialQuotaSnapshotsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    Ok(model_settings::list_official_quota_snapshots_with_runtime_state(&*runtime_state)?)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_provider_settings(
    api_key: Option<String>,
    base_url: Option<String>,
    config_id: Option<String>,
    display_name: Option<String>,
    model_id: String,
    model_options: Option<harness_contracts::ModelRequestOptions>,
    official_quota_api_key: Option<String>,
    provider_id: String,
    protocol: Option<ModelProtocol>,
    provider_defaults: Option<ProviderDefaultsRecord>,
    set_default: Option<bool>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let request = ProviderSettingsRequest {
        api_key,
        base_url,
        config_id,
        display_name,
        model_id,
        model_options,
        official_quota_api_key,
        provider_id,
        protocol,
        provider_defaults,
        set_default: set_default.unwrap_or(true),
    };
    ensure_provider_settings(&request)?;
    let runtime_state = runtime_handle.read().await;
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    let response =
        save_provider_settings_with_runtime_state_unlocked(request, &runtime_state).await?;
    if response.config.is_default {
        let layout = runtime_state.runtime_layout().clone();
        let (settings_runtime, model_id, protocol, model_options) = build_desktop_settings_runtime(
            &layout,
            Some(&response.config.id),
            Arc::clone(&runtime_state.provider_capability_routes),
            Some(Arc::clone(&runtime_state.provider_settings_store)),
        )
        .await?;
        let _settings_reload_guard = runtime_state.settings_reload_lock.lock().await;
        runtime_state.replace_settings_runtime(
            Arc::new(settings_runtime),
            model_id,
            protocol,
            model_options,
        );
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

#[tauri::command]
pub async fn list_browser_mcp_presets(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListBrowserMcpPresetsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_browser_mcp_presets_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_browser_mcp_preset(
    preset_id: BrowserMcpPresetId,
    enabled: Option<bool>,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveBrowserMcpPresetResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _mcp_server_guard = runtime_state.mcp_server_lock.lock().await;
    save_browser_mcp_preset_with_runtime_state(
        SaveBrowserMcpPresetRequest {
            preset_id,
            enabled: enabled.unwrap_or(false),
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_mcp_server(
    enabled: Option<bool>,
    display_name: String,
    id: String,
    scope: String,
    transport: SaveMcpServerTransportConfig,
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

#[tauri::command(rename_all = "camelCase")]
pub async fn list_agent_profiles(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListAgentProfilesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_agent_profiles_with_runtime_state(&*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_agent_profile(
    profile: AgentProfile,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<SaveAgentProfileResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    save_agent_profile_with_runtime_state(profile, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_agent_profile(
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteAgentProfileResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    delete_agent_profile_with_runtime_state(DeleteAgentProfileRequest { id }, &*runtime_state).await
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
    action_plan_id: Option<String>,
    content: String,
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    update_memory_item_with_runtime_state(
        UpdateMemoryItemRequest {
            action_plan_id,
            content,
            id,
        },
        &*runtime_state,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_memory_item(
    action_plan_id: Option<String>,
    id: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<DeleteMemoryItemResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    delete_memory_item_with_runtime_state(
        DeleteMemoryItemRequest { action_plan_id, id },
        &*runtime_state,
    )
    .await
}

#[tauri::command]
pub async fn export_memory_items(
    request: ExportMemoryItemsRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    export_memory_items_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_memory_settings(
    request: GetMemorySettingsRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetMemorySettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_memory_settings_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_memory_settings(
    request: UpdateMemorySettingsRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<UpdateMemorySettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    update_memory_settings_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_thread_memory_settings(
    request: GetThreadMemorySettingsRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetThreadMemorySettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_thread_memory_settings_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_thread_memory_settings(
    request: UpdateThreadMemorySettingsRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<UpdateThreadMemorySettingsResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    update_thread_memory_settings_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_memory_candidates(
    request: ListMemoryCandidatesRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListMemoryCandidatesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_memory_candidates_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn approve_memory_candidate(
    request: ApproveMemoryCandidateRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ApproveMemoryCandidateResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    approve_memory_candidate_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reject_memory_candidate(
    request: RejectMemoryCandidateRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RejectMemoryCandidateResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    reject_memory_candidate_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn merge_memory_candidate(
    request: MergeMemoryCandidateRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<MergeMemoryCandidateResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    let _memory_guard = runtime_state.memory_lock.lock().await;
    merge_memory_candidate_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_memory_recall_traces(
    request: ListMemoryRecallTracesRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListMemoryRecallTracesResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    list_memory_recall_traces_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_memory_recall_trace(
    request: GetMemoryRecallTraceRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetMemoryRecallTraceResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_memory_recall_trace_with_runtime_state(request, &*runtime_state).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_model_request_preview(
    request: GetModelRequestPreviewRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<GetModelRequestPreviewResponse, CommandErrorPayload> {
    let runtime_state = runtime_handle.read().await;
    get_model_request_preview_with_runtime_state(request, &*runtime_state).await
}
