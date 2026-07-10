//! Versioned local daemon protocol contracts.

use chrono::{DateTime, Utc};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ActorId, BlobId, CheckpointId, ClientId, CommandId, EventId, QueueItemId, RequestId,
    RunSegmentId, TaskId, WorkspaceLeaseId,
};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientFrame {
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
    SubmitMessage(SubmitMessageCommand),
    EditQueuedMessage(EditQueuedMessageCommand),
    DeleteQueuedMessage(DeleteQueuedMessageCommand),
    PromoteQueuedMessage(PromoteQueuedMessageCommand),
    StopRun(StopRunCommand),
    ContinueTask(ContinueTaskCommand),
    ResolvePermission(ResolvePermissionCommand),
    SubscribeEvents { after_offset: u64 },
    LoadTask { task_id: TaskId },
    ListTasks,
    ReadBlob { blob_id: BlobId },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerFrame {
    pub request_id: Option<String>,
    pub protocol_version: u16,
    pub message: ServerMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    Handshake(HandshakeResponse),
    CommandAccepted(CommandAccepted),
    CommandRejected(CommandRejected),
    TaskSnapshot(TaskSnapshot),
    TaskList { tasks: Vec<TaskProjection> },
    EventBatch(TaskEventBatch),
    Blob(BlobPayload),
    Error(ProtocolError),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonProtocol {
    pub client: ClientFrame,
    pub server: ServerFrame,
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
pub struct SubmitMessageCommand {
    pub metadata: CommandMetadata,
    pub task_id: TaskId,
    pub content: String,
    pub attachments: Vec<BlobId>,
    pub context_references: Vec<String>,
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
    pub context_references: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventKind {
    UserMessage,
    AssistantText,
    ToolActivity,
    Command,
    Diff,
    Image,
    Permission,
    Compaction,
    Subagent,
    Notice,
    Error,
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
pub struct TaskSnapshot {
    pub projection: TaskProjection,
    pub snapshot_offset: u64,
    pub timeline: Vec<TimelineItemProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskProjection {
    pub task_id: TaskId,
    pub title: String,
    pub state: TaskState,
    pub archived: bool,
    pub stream_version: u64,
    pub last_global_offset: u64,
    pub current_run: Option<RunProjection>,
    pub pending_permission: Option<PermissionProjection>,
    pub queue: Vec<QueueItemProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionProjection {
    pub request_id: RequestId,
    pub revision: u64,
    pub route: PermissionRoute,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunProjection {
    pub segment_id: RunSegmentId,
    pub state: RunState,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueItemProjection {
    pub queue_item_id: QueueItemId,
    pub state: QueueItemState,
    pub revision: u64,
    pub content: String,
    pub attachments: Vec<BlobId>,
    pub context_references: Vec<String>,
    #[serde(
        serialize_with = "strict_rfc3339::serialize",
        deserialize_with = "strict_rfc3339::deserialize"
    )]
    pub created_at: DateTime<Utc>,
    pub consumed_by: Option<RunSegmentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineItemProjection {
    pub id: String,
    pub kind: TimelineEventKind,
    pub global_offset: u64,
    pub run_segment_id: Option<RunSegmentId>,
    pub summary: String,
    pub blob_id: Option<BlobId>,
    pub incomplete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceLeaseProjection {
    pub lease_id: WorkspaceLeaseId,
    pub task_id: TaskId,
    pub mode: WorkspaceMode,
    pub canonical_root: String,
    pub worktree_path: Option<String>,
    pub writable: bool,
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
