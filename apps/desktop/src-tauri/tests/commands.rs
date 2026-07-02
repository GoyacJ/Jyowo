#![allow(dead_code)]
#![allow(unused_imports)]

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::stream;
use futures::StreamExt;
use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentUsePolicy, AgentWorkspaceIsolationMode,
    AssistantClarificationRequestedEvent, AssistantDeltaProducedEvent,
    AssistantMessageCompletedEvent, AssistantNoticeEvent, AssistantReviewRequestedEvent,
    AutomationRunStatus, AutomationSchedule, AutomationSpec, AutomationWorkspaceScope,
    CapabilityRouteKind, ConfigHash, ConversationAttachmentReference, ConversationModelCapability,
    CorrelationId, DecidedBy, EngineError, EngineFailedEvent, EventId, McpConnectionLostEvent,
    McpConnectionLostReason, MessageContent, MessageId, MessageMetadata, MissedRunPolicy,
    ModelModality, ModelProtocol, PermissionActorSource, PermissionRequestedEvent,
    PermissionResolvedEvent, ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
    ProviderServiceAdapterAvailability, ReasoningSummaryChunk, RunModelSnapshot, RunStartedEvent,
    SandboxMode, SnapshotId, StopReason, ToolErrorPayload, ToolServiceBinding, ToolUseFailedEvent,
    ToolUseRequestedEvent, ToolUseSummary, TurnInput, UiSafeText, UserMessageAppendedEvent,
    WorkspaceAccess,
};
use harness_journal::ReplayCursor;
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use harness_tool::BuiltinToolset;
use image::codecs::{gif::GifEncoder, jpeg::JpegEncoder, webp::WebPEncoder};
use image::{ExtendedColorType, ImageEncoder};
use jyowo_desktop_shell::commands::{
    archive_background_agent_with_runtime_state, cancel_background_agent_with_runtime_state,
    cancel_run_payload, cancel_run_with_runtime_state,
    create_attachment_from_path_with_runtime_state, create_conversation_with_runtime_state,
    delete_agent_profile_with_runtime_state, delete_automation_with_runtime_state,
    delete_background_agent_with_runtime_state, delete_conversation_with_runtime_state,
    delete_mcp_server_with_runtime_state, delete_mcp_server_with_store,
    delete_memory_item_with_runtime_state, delete_provider_capability_route_with_store,
    delete_skill_with_runtime_state, desktop_provider_credential_resolver_with_stores,
    export_memory_items_with_runtime_state, export_support_bundle_with_runtime_state,
    get_app_info_payload, get_artifact_media_preview_with_runtime_state,
    get_attachment_media_preview_with_runtime_state, get_background_agent_with_runtime_state,
    get_context_snapshot_with_runtime_state, get_conversation_with_runtime_state,
    get_execution_settings_for_request, get_execution_settings_with_store,
    get_mcp_server_config_with_runtime_state, get_mcp_server_config_with_store,
    get_memory_item_with_runtime_state, get_provider_config_api_key_with_runtime_state,
    get_provider_config_api_key_with_store, get_replay_timeline_with_runtime_state,
    get_skill_detail_with_runtime_state, get_skill_file_with_runtime_state,
    harness_healthcheck_payload, import_skill_with_runtime_state, list_activity_payload,
    list_activity_with_runtime_state, list_agent_profiles_with_runtime_state,
    list_artifacts_with_runtime_state, list_automation_runs_with_runtime_state,
    list_automations_with_runtime_state, list_background_agents_with_runtime_state,
    list_browser_mcp_presets_with_store, list_conversations_with_runtime_state,
    list_eval_cases_payload, list_eval_cases_with_runtime_state, list_mcp_diagnostics_with_store,
    list_mcp_servers_with_runtime_state, list_memory_items_with_runtime_state,
    list_model_provider_catalog_payload, list_provider_capability_route_options_from_inputs,
    list_provider_capability_routes_with_store, list_provider_settings_with_store,
    list_reference_candidates_with_runtime_state, list_skills_with_runtime_state,
    mcp_diagnostic_record_from_event, page_conversation_timeline_with_runtime_state,
    page_conversation_worktree_with_runtime_state, pause_background_agent_with_runtime_state,
    request_provider_config_api_key_reveal_with_runtime_state,
    request_provider_config_api_key_reveal_with_store,
    resolve_permission_for_window_with_runtime_state, resolve_permission_payload,
    resolve_permission_with_runtime_state, resolve_start_run_agent_policy,
    restart_mcp_server_with_runtime_state, resume_background_agent_with_runtime_state,
    run_automation_now_with_runtime_state, run_due_automations_once_with_runtime_state,
    run_eval_case_payload, run_eval_case_with_runtime_state, runtime_state_async,
    runtime_state_for_workspace, save_agent_profile_with_runtime_state,
    save_automation_with_runtime_state, save_browser_mcp_preset_with_store,
    save_mcp_server_with_runtime_state, save_mcp_server_with_store,
    save_provider_capability_route_settings_with_store, save_provider_capability_route_with_store,
    save_provider_settings_with_runtime_state, save_provider_settings_with_store,
    send_background_agent_input_with_runtime_state, set_execution_settings_with_store,
    set_mcp_server_enabled_with_runtime_state, set_skill_enabled_with_runtime_state,
    spawn_automation_scheduler, spawn_automation_scheduler_on_tauri_runtime, start_run_payload,
    start_run_with_runtime_state, subscribe_conversation_events_for_window_with_runtime_state,
    unsubscribe_conversation_events_for_window_with_runtime_state,
    update_memory_item_with_runtime_state, validate_provider_settings_payload,
    ArtifactSummaryPayload, AttachmentBlobRefPayload, AttachmentReferencePayload,
    BackgroundAgentIdRequest, BrowserMcpPresetId, CancelRunRequest, ContextReferencePayload,
    ConversationEventBatchPayload, ConversationModelCapabilityRecord,
    CreateAttachmentFromPathRequest, DeleteAgentProfileRequest, DeleteConversationRequest,
    DeleteMcpServerRequest, DeleteMemoryItemRequest, DeleteProviderCapabilityRouteRequest,
    DeleteSkillRequest, DesktopConversationMetadataStore, DesktopExecutionSettingsStore,
    DesktopMcpDiagnosticStore, DesktopProviderCapabilityRouteStore, DesktopProviderSettingsStore,
    DesktopRuntimeState, DesktopSkillStore, ExportSupportBundleRequest,
    GetArtifactMediaPreviewRequest, GetAttachmentMediaPreviewRequest, GetBackgroundAgentRequest,
    GetContextSnapshotRequest, GetConversationRequest, GetExecutionSettingsRequest,
    GetMcpServerConfigRequest, GetMemoryItemRequest, GetProviderConfigApiKeyRequest,
    GetSkillDetailRequest, GetSkillFileRequest, ImportSkillRequest, ListActivityRequest,
    ListArtifactsRequest, ListBackgroundAgentsRequest, ListReferenceCandidatesRequest,
    McpDiagnosticRecord, McpDiagnosticSeverity, McpDiagnosticStore, McpHeaderEnvRecord,
    McpNameValueRecord, McpServerConfigRecord, McpServerStore, McpServerTransportConfig,
    PageConversationTimelineRequest, PageConversationWorktreeDirection,
    PageConversationWorktreeRequest, PermissionDecision, ProviderCapabilityRouteStore,
    ProviderConfigRecord, ProviderModelDescriptorRecord, ProviderModelLifecycleRecord,
    ProviderModelModalityRecord, ProviderSettingsRecord, ProviderSettingsRequest,
    ProviderSettingsStore, ReplayTimelineRequest, RequestProviderConfigApiKeyRevealRequest,
    ResolvePermissionRequest, RestartMcpServerRequest, RunEvalCaseRequest, SaveAutomationRequest,
    SaveBrowserMcpPresetRequest, SaveMcpServerRequest, SaveProviderCapabilityRouteRequest,
    SendBackgroundAgentInputRequest, SetAutomationEnabledRequest, SetExecutionSettingsRequest,
    SetMcpServerEnabledRequest, SetSkillEnabledRequest, SkillStore, SkillStoreRecord,
    StartRunRequest, SubscribeConversationEventsRequest, UnsubscribeConversationEventsRequest,
    UpdateMemoryItemRequest, ValidateProviderSettingsRequest,
};
use jyowo_desktop_shell::project_registry::ProjectRegistry;
use jyowo_harness_sdk::builtin::{DefaultRedactor, FileBlobStore};
use jyowo_harness_sdk::ext::{
    now, ArtifactCreatedEvent, ArtifactSource, ArtifactStatus, ArtifactUpdatedEvent, BlobMeta,
    BlobRetention, BlobStore, BudgetMetric, Decision, DecisionScope, DeferPolicy, DeltaChunk,
    Event, EventStore, FallbackPolicy, InteractivityLevel, McpConnection, McpError, McpRegistry,
    McpServerId, McpServerScope, McpServerSource, McpServerSpec, McpToolDescriptor, McpToolResult,
    MemoryId, MemoryKind, MemoryMetadata, MemoryRecord, MemorySource, MemoryStore,
    MemoryVisibility, Message, MessagePart, MessageRole, ModelError, OverflowAction,
    PermissionCheck, PermissionContext, PermissionMode, PermissionRequest, PermissionSubject,
    ProviderCredentialResolveContext, ProviderRestriction, RedactPatternSet, RedactRules,
    RedactScope, Redactor, RequestId, ResultBudget, RuleSnapshot, RunId, SessionId, Severity,
    StreamBrokerConfig, TenantId, ThinkingDelta, Tool, ToolCapability, ToolContext, ToolDescriptor,
    ToolError, ToolEvent, ToolGroup, ToolProfile, ToolProperties, ToolRegistry, ToolResult,
    ToolStream, ToolUseId, TransportChoice, TrustLevel, UsageSnapshot, ValidationError,
};
use jyowo_harness_sdk::ext::{ContentDelta, ModelStreamEvent};
use jyowo_harness_sdk::testing::{
    InMemoryEventStore, InMemoryMemoryProvider, NoopRedactor, NoopSandbox, ScriptedProvider,
    ScriptedResponse, TestModelProvider,
};
use jyowo_harness_sdk::{
    AgentCapabilityResolutionContext, ConversationEventsPageRequest, Harness, HarnessOptions,
    McpConfig, StreamPermissionRuntime,
};
use parking_lot::RwLock as ParkingRwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static WORKSPACE_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());
static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
const WORKSPACE_ROOT_ENV: &str = "JYOWO_WORKSPACE_ROOT";
const HOME_ENV: &str = "HOME";
const TEST_MODEL_CONFIG_ID: &str = "test-model-config";

#[path = "commands/activity.rs"]
mod activity;
#[path = "commands/activity_redaction.rs"]
mod activity_redaction;
#[path = "commands/agent_run_policy.rs"]
mod agent_run_policy;
#[path = "commands/agents.rs"]
mod agents;
#[path = "commands/app_eval.rs"]
mod app_eval;
#[path = "commands/app_info.rs"]
mod app_info;
#[path = "commands/artifact_listing.rs"]
mod artifact_listing;
#[path = "commands/artifact_preview.rs"]
mod artifact_preview;
#[path = "commands/attachment_preview.rs"]
mod attachment_preview;
#[path = "commands/automation_support.rs"]
mod automation_support;
#[path = "commands/automations.rs"]
mod automations;
#[path = "commands/background_agents.rs"]
mod background_agents;
#[path = "commands/background_supervisor.rs"]
mod background_supervisor;
#[path = "commands/context_snapshot.rs"]
mod context_snapshot;
#[path = "commands/conversation_timeline.rs"]
mod conversation_timeline;
#[path = "commands/conversation_worktree.rs"]
mod conversation_worktree;
#[path = "commands/conversations.rs"]
mod conversations;
#[path = "commands/eval_lab.rs"]
mod eval_lab;
#[path = "commands/execution_settings.rs"]
mod execution_settings;
#[path = "commands/mcp.rs"]
mod mcp;
#[path = "commands/memory.rs"]
mod memory;
#[path = "commands/model_usage_summary.rs"]
mod model_usage_summary;
#[path = "commands/official_quota.rs"]
mod official_quota;
#[path = "commands/permissions.rs"]
mod permissions;
#[path = "commands/preview_support.rs"]
mod preview_support;
#[path = "commands/provider_credential_routes.rs"]
mod provider_credential_routes;
#[path = "commands/provider_probe.rs"]
mod provider_probe;
#[path = "commands/provider_quota_settings.rs"]
mod provider_quota_settings;
#[path = "commands/provider_route_support.rs"]
mod provider_route_support;
#[path = "commands/provider_routes.rs"]
mod provider_routes;
#[path = "commands/provider_settings.rs"]
mod provider_settings;
#[path = "commands/provider_settings_store.rs"]
mod provider_settings_store;
#[path = "commands/provider_support.rs"]
mod provider_support;
#[path = "commands/replay.rs"]
mod replay;
#[path = "commands/run_subscriptions.rs"]
mod run_subscriptions;
#[path = "commands/runs.rs"]
mod runs;
#[path = "commands/skills.rs"]
mod skills;
#[path = "commands/support.rs"]
mod support;
#[path = "commands/support_bundle.rs"]
mod support_bundle;

pub(crate) use automation_support::*;
pub(crate) use preview_support::*;
pub(crate) use provider_route_support::*;
pub(crate) use provider_support::*;
pub(crate) use support::*;
