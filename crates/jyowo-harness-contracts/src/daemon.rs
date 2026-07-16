//! Versioned local daemon protocol contracts.

use chrono::{DateTime, Utc};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ActionPlanId, ActorId, ApproveMemoryCandidateRequest, ApproveMemoryCandidateResponse,
    AutomationDeletedResponse, AutomationEnabledResponse, AutomationRunResponse,
    AutomationRunsResponse, AutomationSavedResponse, AutomationSpec, AutomationsResponse, BlobId,
    CheckpointId, ClientId, CommandId, ConversationContextReference, EventId,
    GetMemoryRecallTraceRequest, GetMemoryRecallTraceResponse, GetMemorySettingsRequest,
    GetMemorySettingsResponse, GetModelRequestPreviewRequest, GetModelRequestPreviewResponse,
    GetThreadMemorySettingsRequest, GetThreadMemorySettingsResponse, ListMemoryCandidatesRequest,
    ListMemoryCandidatesResponse, ListMemoryRecallTracesRequest, ListMemoryRecallTracesResponse,
    MemoryId, MergeMemoryCandidateRequest, MergeMemoryCandidateResponse, PermissionMode,
    QueueItemId, RejectMemoryCandidateRequest, RejectMemoryCandidateResponse, RequestId,
    RunSegmentId, SessionId, SkillId, SkillSourceKind, SubagentId, TaskId,
    UpdateMemorySettingsRequest, UpdateMemorySettingsResponse, UpdateThreadMemorySettingsRequest,
    UpdateThreadMemorySettingsResponse, WorkspaceLeaseId,
};

pub const PROTOCOL_VERSION: u16 = 5;

/// Maximum JSON body accepted by the length-prefixed local daemon transport.
pub const MAX_DAEMON_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Maximum serialized `events` array body in one task audit page.
///
/// The four-KiB reserve covers the response envelope, maximum request ID,
/// task ID, cursor, and JSON syntax within [`MAX_DAEMON_FRAME_BYTES`].
pub const MAX_DAEMON_TASK_EVENT_PAGE_BYTES: usize = MAX_DAEMON_FRAME_BYTES - 4 * 1024;

/// Maximum printable ASCII request ID size reserved in every daemon response envelope.
pub const MAX_DAEMON_REQUEST_ID_BYTES: usize = 128;

/// Maximum raw blob body that can be returned inline after base64 expansion.
///
/// Base64 consumes four bytes for every three input bytes. The additional
/// four-KiB reserve keeps the response envelope, request ID, blob metadata,
/// and JSON syntax within [`MAX_DAEMON_FRAME_BYTES`].
pub const MAX_DAEMON_BLOB_BYTES: usize = 6 * 1024 * 1024 - 4 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientFrame {
    #[schemars(
        length(min = 1, max = "MAX_DAEMON_REQUEST_ID_BYTES"),
        pattern(r"^[\x20-\x7E]+$")
    )]
    pub request_id: String,
    pub protocol_version: u16,
    pub request: ClientRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ClientRequest {
    Handshake(HandshakeRequest),
    CreateTask(CreateTaskCommand),
    RenameTask(RenameTaskCommand),
    SetTaskPinned(SetTaskPinnedCommand),
    SetTaskArchived(SetTaskArchivedCommand),
    RemoveTask(RemoveTaskCommand),
    SubmitMessage(SubmitMessageCommand),
    EditQueuedMessage(EditQueuedMessageCommand),
    DeleteQueuedMessage(DeleteQueuedMessageCommand),
    PromoteQueuedMessage(PromoteQueuedMessageCommand),
    StopRun(StopRunCommand),
    ContinueTask(ContinueTaskCommand),
    ResolvePermission(ResolvePermissionCommand),
    SubscribeEvents {
        after_offset: u64,
    },
    LoadEvents {
        after_global_offset: u64,
        limit: u16,
    },
    LoadTask {
        task_id: TaskId,
    },
    LoadTaskEvents {
        task_id: TaskId,
        before_global_offset: Option<u64>,
        limit: u16,
    },
    ListTasks,
    ListRuntimeTools {
        workspace_root: Option<String>,
    },
    ListSkillReferenceCandidates {
        task_id: TaskId,
    },
    ListMemoryItems {
        workspace_root: Option<String>,
    },
    GetMemoryItem {
        workspace_root: Option<String>,
        memory_id: MemoryId,
    },
    UpdateMemoryItem {
        workspace_root: Option<String>,
        memory_id: MemoryId,
        content: String,
        action_plan_id: Option<ActionPlanId>,
    },
    DeleteMemoryItem {
        workspace_root: Option<String>,
        memory_id: MemoryId,
        action_plan_id: Option<ActionPlanId>,
    },
    ExportMemoryItems {
        workspace_root: Option<String>,
        request: ExportMemoryItemsRequest,
    },
    ListMemoryCandidates {
        workspace_root: Option<String>,
        request: ListMemoryCandidatesRequest,
    },
    ApproveMemoryCandidate {
        workspace_root: Option<String>,
        request: ApproveMemoryCandidateRequest,
    },
    RejectMemoryCandidate {
        workspace_root: Option<String>,
        request: RejectMemoryCandidateRequest,
    },
    MergeMemoryCandidate {
        workspace_root: Option<String>,
        request: MergeMemoryCandidateRequest,
    },
    ListMemoryRecallTraces {
        workspace_root: Option<String>,
        request: ListMemoryRecallTracesRequest,
    },
    GetMemoryRecallTrace {
        workspace_root: Option<String>,
        request: GetMemoryRecallTraceRequest,
    },
    GetModelRequestPreview {
        workspace_root: Option<String>,
        request: GetModelRequestPreviewRequest,
    },
    GetMemorySettings {
        workspace_root: Option<String>,
        request: GetMemorySettingsRequest,
    },
    UpdateMemorySettings {
        workspace_root: Option<String>,
        request: UpdateMemorySettingsRequest,
    },
    GetThreadMemorySettings {
        workspace_root: Option<String>,
        request: GetThreadMemorySettingsRequest,
    },
    UpdateThreadMemorySettings {
        workspace_root: Option<String>,
        request: UpdateThreadMemorySettingsRequest,
    },
    ListAutomations {
        workspace_root: Option<String>,
    },
    SaveAutomation {
        workspace_root: Option<String>,
        automation: AutomationSpec,
    },
    SetAutomationEnabled {
        workspace_root: Option<String>,
        automation_id: String,
        enabled: bool,
    },
    DeleteAutomation {
        workspace_root: Option<String>,
        automation_id: String,
    },
    RunAutomationNow {
        workspace_root: Option<String>,
        automation_id: String,
    },
    ListAutomationRuns {
        workspace_root: Option<String>,
        automation_id: Option<String>,
    },
    Browser {
        task_id: TaskId,
        command: BrowserCommand,
    },
    StageBlob(StageBlobCommand),
    ReadBlob {
        blob_id: BlobId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerFrame {
    pub request_id: Option<String>,
    pub protocol_version: u16,
    pub message: ServerMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ServerMessage {
    Handshake(HandshakeResponse),
    CommandAccepted(CommandAccepted),
    CommandRejected(CommandRejected),
    TaskSnapshot(TaskSnapshot),
    TaskEventPage(TaskEventPage),
    EventHistoryPage(TaskEventHistoryPage),
    TaskList { tasks: Vec<TaskProjection> },
    RuntimeTools(ListRuntimeToolsResponse),
    SkillReferenceCandidates(ListSkillReferenceCandidatesResponse),
    MemoryItems(ListMemoryItemsResponse),
    MemoryItem(GetMemoryItemResponse),
    MemoryUpdated(UpdateMemoryItemResponse),
    MemoryDeleted(DeleteMemoryItemResponse),
    MemoryExported(ExportMemoryItemsResponse),
    MemoryCandidates(ListMemoryCandidatesResponse),
    MemoryCandidateApproved(ApproveMemoryCandidateResponse),
    MemoryCandidateRejected(RejectMemoryCandidateResponse),
    MemoryCandidatesMerged(MergeMemoryCandidateResponse),
    MemoryRecallTraces(ListMemoryRecallTracesResponse),
    MemoryRecallTrace(GetMemoryRecallTraceResponse),
    ModelRequestPreview(GetModelRequestPreviewResponse),
    MemorySettings(GetMemorySettingsResponse),
    MemorySettingsUpdated(UpdateMemorySettingsResponse),
    ThreadMemorySettings(GetThreadMemorySettingsResponse),
    ThreadMemorySettingsUpdated(UpdateThreadMemorySettingsResponse),
    Automations(AutomationsResponse),
    AutomationSaved(AutomationSavedResponse),
    AutomationEnabled(AutomationEnabledResponse),
    AutomationDeleted(AutomationDeletedResponse),
    AutomationRun(AutomationRunResponse),
    AutomationRuns(AutomationRunsResponse),
    BrowserSession(BrowserSessionState),
    EventBatch(TaskEventBatch),
    Blob(BlobPayload),
    Error(ProtocolError),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BrowserCommand {
    Open { url: Option<String> },
    Status,
    Close,
    Show,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionStatus {
    Unavailable,
    Starting,
    Ready,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSessionState {
    pub task_id: TaskId,
    pub status: BrowserSessionStatus,
    pub dashboard_url: Option<String>,
    pub current_url: Option<String>,
    pub title: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonProtocol {
    pub client: ClientFrame,
    pub server: ServerFrame,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListRuntimeToolsResponse {
    pub generation: u64,
    pub tools: Vec<RuntimeToolSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeToolServiceBindingSummary {
    pub provider_id: String,
    pub operation_id: String,
    pub route_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListSkillReferenceCandidatesResponse {
    pub skills: Vec<SkillReferenceCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillReferenceCandidate {
    pub skill_id: SkillId,
    pub label: String,
    pub source: SkillSourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListMemoryItemsResponse {
    pub items: Vec<DaemonMemoryItemSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetMemoryItemResponse {
    pub item: DaemonMemoryItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMemoryItemResponse {
    pub item: DaemonMemoryItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteMemoryItemResponse {
    pub memory_id: MemoryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonMemoryItemSummary {
    pub id: MemoryId,
    pub provider_id: Option<String>,
    pub kind: String,
    pub visibility: String,
    pub content_preview: String,
    pub content_hash: String,
    pub source: String,
    pub tags: Vec<String>,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub deleted: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonMemoryItem {
    pub id: MemoryId,
    pub provider_id: Option<String>,
    pub kind: String,
    pub visibility: String,
    pub content: String,
    pub content_hash: String,
    pub source: String,
    pub tags: Vec<String>,
    pub confidence: f32,
    pub access_count: u32,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub deleted: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportMemoryItemsRequest {
    pub session_id: Option<SessionId>,
    pub scope: String,
    pub format: String,
    pub include_raw_content: bool,
    pub include_metadata: bool,
    pub include_hashes: bool,
    pub explicit_user_action: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportMemoryItemsResponse {
    pub exported_at: DateTime<Utc>,
    pub format: String,
    pub scope: String,
    pub include_raw_content: bool,
    pub include_metadata: bool,
    pub include_hashes: bool,
    pub item_count: u32,
    pub path: String,
    pub audit_hash: String,
}

#[must_use]
pub fn daemon_protocol_schema() -> Schema {
    schemars::schema_for!(DaemonProtocol)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandshakeRequest {
    pub client_id: ClientId,
    pub client_version: String,
    pub user_instance_id: String,
    pub connection_token: String,
    pub last_acknowledged_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandshakeResponse {
    pub daemon_version: String,
    pub user_instance_id: String,
    pub latest_global_offset: u64,
    pub agent_capabilities: AgentCapabilities,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCapabilities {
    pub subagents: bool,
    pub agent_teams: bool,
    pub background_agents: bool,
}

impl AgentCapabilities {
    #[must_use]
    pub const fn daemon_native() -> Self {
        Self {
            subagents: true,
            agent_teams: true,
            background_agents: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandMetadata {
    pub command_id: CommandId,
    pub idempotency_key: String,
    pub expected_stream_version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTaskCommand {
    pub metadata: CommandMetadata,
    pub title: String,
    pub workspace: WorkspaceSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameTaskCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetTaskPinnedCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetTaskArchivedCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveTaskCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitMessageCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub content: String,
    pub attachments: Vec<BlobId>,
    pub context_references: Vec<ConversationContextReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    #[serde(default)]
    pub permission_mode: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditQueuedMessageCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub queue_item_id: QueueItemId,
    pub expected_revision: u64,
    pub content: String,
    pub attachments: Vec<BlobId>,
    pub context_references: Vec<ConversationContextReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteQueuedMessageCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub queue_item_id: QueueItemId,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromoteQueuedMessageCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub queue_item_id: QueueItemId,
    pub expected_revision: u64,
    pub mode: PromotionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopRunCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub mode: StopMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContinueTaskCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub indeterminate_tools: Vec<IndeterminateToolDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvePermissionCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub permission_request_id: RequestId,
    pub request_revision: u64,
    pub option_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageBlobCommand {
    pub task_id: TaskId,
    pub media_type: String,
    pub base64_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromotionMode {
    SafePoint,
    ForceStop,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopMode {
    SafePoint,
    Force,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndeterminateToolResolution {
    TreatAsFailed,
    ExecuteAgain,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IndeterminateToolDecision {
    pub tool_use_id: String,
    pub resolution: IndeterminateToolResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandAccepted {
    pub command_id: CommandId,
    pub task_id: TaskId,
    pub stream_version: u64,
    pub committed_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandRejected {
    pub command_id: Option<CommandId>,
    pub task_id: Option<TaskId>,
    pub reason: CommandRejectionReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub current_stream_version: Option<u64>,
    pub latest_queue_item: Option<QueueItemProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandRejectionReason {
    InvalidCommand,
    WrongExpectedVersion,
    StaleQueueRevision,
    InvalidTransition,
    PermissionExpired,
    WorkspaceConflict,
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Idle,
    Running,
    WaitingPermission,
    Yielding,
    Interrupted,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemState {
    Queued,
    Promoting,
    Consumed,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    WaitingPermission,
    Yielding,
    Interrupted,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunTerminalReason {
    Completed,
    Superseded,
    ForcedInterruption,
    InterruptedByRestart,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    Current,
    ManagedWorktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLeaseState {
    Preparing,
    Waiting,
    Active,
    CleanupPending,
    CleanupBlocked,
    Released,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSelection {
    pub mode: WorkspaceMode,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRoute {
    ForegroundTask,
    SavedPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonPermissionKind {
    Command,
    Filesystem,
    Network,
    Mcp,
    Automation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionOption {
    pub option_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionRequestDetails {
    pub kind: DaemonPermissionKind,
    pub segment_id: RunSegmentId,
    pub action_plan_hash: String,
    pub sandbox_policy_hash: String,
    pub workspace: String,
    pub subject: Value,
    pub actor_source: Value,
    pub options: Vec<PermissionOption>,
    pub preview: String,
    #[serde(
        serialize_with = "strict_rfc3339::serialize",
        deserialize_with = "strict_rfc3339::deserialize"
    )]
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventKind {
    UserMessage,
    AssistantText,
    ToolActivity,
    Command,
    Diff,
    File,
    Artifact,
    Image,
    Permission,
    Compaction,
    Subagent,
    Notice,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineTextFormat {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineNoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineArtifactSurface {
    Inline,
    Card,
    Workbench,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineArtifactPresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_surface: Option<TimelineArtifactSurface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_blob_id: Option<BlobId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineArtifactProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<BlobId>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<String>,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<TimelineArtifactPresentation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineContentBlock {
    Text {
        format: TimelineTextFormat,
        text: String,
    },
    Artifact {
        artifact: TimelineArtifactProjection,
    },
    ToolActivity {
        activity: TimelineToolProjection,
    },
    Notice {
        level: TimelineNoticeLevel,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineToolOperation {
    Read,
    Edit,
    Search,
    Command,
    Browse,
    Generate,
    Delegate,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineToolStatus {
    Requested,
    Running,
    Completed,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineToolProjection {
    pub tool_use_id: String,
    pub tool_name: String,
    pub operation: TimelineToolOperation,
    pub status: TimelineToolStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventSourceKind {
    User,
    Assistant,
    Engine,
    Tool,
    PermissionBroker,
    Supervisor,
    Subagent,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventSource {
    pub kind: EventSourceKind,
    pub actor_id: Option<ActorId>,
    pub client_id: Option<ClientId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskEventEnvelope {
    pub global_offset: u64,
    pub task_id: TaskId,
    pub stream_sequence: u64,
    pub event_id: EventId,
    pub event_type: String,
    pub schema_version: u16,
    #[serde(
        serialize_with = "strict_rfc3339::serialize",
        deserialize_with = "strict_rfc3339::deserialize"
    )]
    pub recorded_at: DateTime<Utc>,
    pub source: EventSource,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskEventBatch {
    pub after_offset: u64,
    pub latest_offset: u64,
    pub gap: bool,
    pub events: Vec<TaskEventEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskEventHistoryPage {
    pub after_global_offset: u64,
    pub latest_global_offset: u64,
    pub next_after_global_offset: u64,
    pub has_more: bool,
    pub events: Vec<TaskEventEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSnapshot {
    pub projection: TaskProjection,
    pub snapshot_offset: u64,
    pub timeline: Vec<TimelineItemProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskEventPage {
    pub task_id: TaskId,
    pub events: Vec<TaskEventEnvelope>,
    pub next_before_offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskProjection {
    pub task_id: TaskId,
    pub title: String,
    pub state: TaskState,
    #[serde(default)]
    pub pinned: bool,
    pub archived: bool,
    #[serde(default)]
    pub removed: bool,
    pub stream_version: u64,
    pub last_global_offset: u64,
    pub current_run: Option<RunProjection>,
    pub pending_permission: Option<PermissionProjection>,
    pub queue: Vec<QueueItemProjection>,
    #[serde(default)]
    pub workspace: Option<WorkspaceSelection>,
    #[serde(default)]
    pub actor_id: Option<ActorId>,
    #[serde(default)]
    pub context_cursor: u64,
    #[serde(default)]
    pub parent: Option<SubagentParentProjection>,
    #[serde(default)]
    pub subagents: Vec<SubagentProjection>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentActorState {
    Starting,
    Running,
    Yielding,
    Background,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubagentParentProjection {
    pub parent_task_id: TaskId,
    pub parent_segment_id: RunSegmentId,
    pub delegation_id: SubagentId,
    #[serde(default)]
    pub attachment: ChildAttachment,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChildAttachment {
    #[default]
    Attached,
    Detached,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubagentProjection {
    pub child_task_id: TaskId,
    pub actor_id: ActorId,
    pub segment_id: RunSegmentId,
    pub parent_task_id: TaskId,
    pub parent_segment_id: RunSegmentId,
    pub delegation_id: SubagentId,
    pub context_cursor: u64,
    pub workspace_lease_id: Option<WorkspaceLeaseId>,
    pub state: SubagentActorState,
    pub detached: bool,
    pub summary: Option<String>,
    #[serde(
        serialize_with = "strict_rfc3339::serialize",
        deserialize_with = "strict_rfc3339::deserialize"
    )]
    pub started_at: DateTime<Utc>,
    #[serde(
        default,
        serialize_with = "strict_rfc3339::option::serialize",
        deserialize_with = "strict_rfc3339::option::deserialize"
    )]
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionProjection {
    pub request_id: RequestId,
    pub revision: u64,
    pub route: PermissionRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<PermissionRequestDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunProjection {
    pub segment_id: RunSegmentId,
    pub state: RunState,
    #[serde(default)]
    pub promotion_mode: Option<PromotionMode>,
    pub terminal_reason: Option<RunTerminalReason>,
    #[serde(
        serialize_with = "strict_rfc3339::serialize",
        deserialize_with = "strict_rfc3339::deserialize"
    )]
    pub started_at: DateTime<Utc>,
    #[serde(
        default,
        serialize_with = "strict_rfc3339::option::serialize",
        deserialize_with = "strict_rfc3339::option::deserialize"
    )]
    pub ended_at: Option<DateTime<Utc>>,
    pub incomplete_output: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueItemProjection {
    pub queue_item_id: QueueItemId,
    pub state: QueueItemState,
    pub revision: u64,
    pub content: String,
    pub attachments: Vec<BlobId>,
    pub context_references: Vec<ConversationContextReference>,
    #[serde(
        serialize_with = "strict_rfc3339::serialize",
        deserialize_with = "strict_rfc3339::deserialize"
    )]
    pub created_at: DateTime<Utc>,
    pub created_global_offset: u64,
    pub consumed_by: Option<RunSegmentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineItemProjection {
    pub id: String,
    pub kind: TimelineEventKind,
    pub global_offset: u64,
    pub run_segment_id: Option<RunSegmentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_group_id: Option<String>,
    pub summary: String,
    pub blob_id: Option<BlobId>,
    pub incomplete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<TimelineToolProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_blocks: Vec<TimelineContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceLeaseProjection {
    pub lease_id: WorkspaceLeaseId,
    pub task_id: TaskId,
    pub actor_id: ActorId,
    pub mode: WorkspaceMode,
    pub canonical_root: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub writable: bool,
    pub state: WorkspaceLeaseState,
    pub requested_at: DateTime<Utc>,
    pub acquired_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub baseline_commit: Option<String>,
    pub baseline_status: String,
    pub patch_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointProjection {
    pub checkpoint_id: CheckpointId,
    pub task_id: TaskId,
    pub run_segment_id: RunSegmentId,
    pub global_offset: u64,
    pub context_cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlobPayload {
    pub blob_id: BlobId,
    pub media_type: String,
    pub size: u64,
    #[schemars(length(min = 32, max = 32))]
    pub content_hash: Vec<u8>,
    pub base64_data: Option<String>,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolError {
    pub code: ProtocolErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolErrorCode {
    InvalidFrame,
    FrameTooLarge,
    ProtocolMismatch,
    AuthenticationFailed,
    NotFound,
    Internal,
}

mod strict_rfc3339 {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_rfc3339_opts(SecondsFormat::AutoSi, true))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse(&value).map_err(D::Error::custom)
    }

    fn parse(value: &str) -> Result<DateTime<Utc>, String> {
        if !has_strict_shape(value) {
            return Err("timestamp must use RFC 3339 with `T` and a colonized offset".into());
        }
        DateTime::parse_from_rfc3339(value)
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .map_err(|error| error.to_string())
    }

    fn has_strict_shape(value: &str) -> bool {
        let bytes = value.as_bytes();
        if bytes.len() < 20
            || bytes.get(4) != Some(&b'-')
            || bytes.get(7) != Some(&b'-')
            || bytes.get(10) != Some(&b'T')
            || bytes.get(13) != Some(&b':')
            || bytes.get(16) != Some(&b':')
        {
            return false;
        }

        let timezone_start = if bytes.last() == Some(&b'Z') {
            bytes.len() - 1
        } else if bytes.len() >= 25
            && matches!(bytes[bytes.len() - 6], b'+' | b'-')
            && bytes[bytes.len() - 3] == b':'
        {
            bytes.len() - 6
        } else {
            return false;
        };

        let fixed_digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
        if fixed_digits
            .iter()
            .any(|&index| !bytes.get(index).is_some_and(u8::is_ascii_digit))
        {
            return false;
        }

        if timezone_start > 19
            && (bytes.get(19) != Some(&b'.')
                || bytes[20..timezone_start]
                    .iter()
                    .any(|byte| !byte.is_ascii_digit()))
        {
            return false;
        }

        let seconds = (bytes[17] - b'0') * 10 + bytes[18] - b'0';
        seconds <= 59
    }

    pub mod option {
        use super::{parse, DateTime, SecondsFormat, Utc};
        use serde::de::Error as _;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(value: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match value {
                Some(timestamp) => serializer
                    .serialize_some(&timestamp.to_rfc3339_opts(SecondsFormat::AutoSi, true)),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            Option::<String>::deserialize(deserializer)?
                .map(|value| parse(&value).map_err(D::Error::custom))
                .transpose()
        }
    }
}
