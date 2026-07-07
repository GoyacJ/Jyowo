//! Conversation read model contracts.
//!
//! These types are the stable UI-facing query surface. They are projections of
//! redacted journal events, not replacements for the durable journal facts.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ArtifactSource, ArtifactStatus, ConversationAttachmentReference, EventId, RedactRules,
    Redactor, RunModelSnapshot,
};

const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct UiSafeText(String);

impl UiSafeText {
    #[must_use]
    pub fn from_trusted_redacted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn from_redacted_display(value: impl AsRef<str>, redactor: &dyn Redactor) -> Self {
        let redacted = redactor.redact(value.as_ref(), &RedactRules::default());
        if contains_obvious_secret(&redacted) {
            return Self(REDACTED.to_owned());
        }
        let redacted = redact_private_absolute_paths(&redacted);
        if contains_private_absolute_path(&redacted) {
            return Self(REDACTED.to_owned());
        }
        Self(redacted)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<UiSafeText> for String {
    fn from(value: UiSafeText) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCursor {
    pub event_id: EventId,
    pub conversation_sequence: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub is_empty: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_preview: Option<UiSafeText>,
    pub title: UiSafeText,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageAuthor {
    User,
    Assistant,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub author: ConversationMessageAuthor,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub conversation_sequence: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSnapshot {
    pub id: String,
    pub messages: Vec<ConversationMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    pub title: UiSafeText,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelineEvent {
    pub id: String,
    pub cursor: ConversationCursor,
    pub payload: Value,
    pub run_id: String,
    pub sequence: u64,
    pub source: String,
    pub timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub visibility: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelinePage {
    pub events: Vec<ConversationTimelineEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
    pub gap: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorktreePage {
    pub turns: Vec<ConversationTurn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_cursor: Option<ConversationTurnCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_cursor: Option<ConversationCursor>,
    pub has_more_before: bool,
    pub has_more_after: bool,
    pub gap: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationInspectorSelection {
    Turn {
        turn_id: String,
    },
    Event {
        event_id: String,
    },
    EvidenceRef {
        evidence_ref_id: EvidenceRefId,
    },
    Artifact {
        artifact_id: String,
    },
    ArtifactRevision {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact_id: Option<String>,
        revision_id: String,
    },
    Decision {
        request_id: String,
    },
    Tool {
        tool_use_id: String,
    },
    Command {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full_output_ref: Option<EvidenceRefId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    Diff {
        change_set_id: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationInspectorItem {
    Empty,
    Turn { turn: ConversationTurn },
    Decision { decision: DecisionRequestState },
    Tool { attempt: ToolAttempt },
    Command { command: CommandExecution },
    Diff { change_set: ChangeSet },
    Artifact { segment: ArtifactSegment },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationInspectorItemResponse {
    pub item: ConversationInspectorItem,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnCursor {
    pub turn_id: String,
    pub position: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurn {
    pub id: String,
    pub conversation_id: String,
    pub position: u64,
    pub user: ConversationTurnUserMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant: Option<AssistantWork>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnUserMessage {
    pub id: String,
    pub message_id: String,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<ConversationAttachmentReference>,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssistantWork {
    pub id: String,
    pub run_id: String,
    pub projection_version: u64,
    #[serde(default)]
    pub stream_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<AssistantWorkModelSnapshot>,
    pub status: AssistantWorkStatus,
    pub segments: Vec<AssistantSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AssistantWorkModelSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: crate::ModelProtocol,
}

impl From<&RunModelSnapshot> for AssistantWorkModelSnapshot {
    fn from(value: &RunModelSnapshot) -> Self {
        Self {
            model_config_id: value.model_config_id.clone(),
            provider_id: value.provider_id.clone(),
            model_id: value.model_id.clone(),
            display_name: value.display_name.clone(),
            protocol: value.protocol,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssistantWorkStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssistantSegment {
    Process(ProcessSegment),
    Text(TextSegment),
    ToolGroup(ToolGroupSegment),
    Artifact(ArtifactSegment),
    ReviewRequest(ReviewRequestSegment),
    ClarificationRequest(ClarificationRequestSegment),
    Notice(NoticeSegment),
    Error(ErrorSegment),
    AgentActivity(AgentActivitySegment),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AgentActivityKind {
    Subagent,
    AgentTeam,
    BackgroundAgent,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AgentActivityStatus {
    Loading,
    Running,
    WaitingPermission,
    WaitingInput,
    Completed,
    Failed,
    Cancelled,
    Stalled,
    Redacted,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivityPermissionState {
    pub id: String,
    pub request_id: String,
    pub status: DecisionRequestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamMemberActivity {
    pub agent_id: String,
    pub role: UiSafeText,
    pub status: AgentActivityStatus,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamTaskActivity {
    pub id: String,
    pub title: UiSafeText,
    pub status: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_profile_id: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamActivityDetails {
    pub topology: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<AgentTeamMemberActivity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<AgentTeamMemberActivity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_tasks: Vec<AgentTeamTaskActivity>,
    pub mailbox_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mailbox_summaries: Vec<UiSafeText>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivitySegment {
    pub id: String,
    pub order: u32,
    pub activity_kind: AgentActivityKind,
    pub agent_id: String,
    pub role: UiSafeText,
    pub task_summary: UiSafeText,
    pub status: AgentActivityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<AgentActivityPermissionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<AgentTeamActivityDetails>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSegment {
    pub id: String,
    pub order: u32,
    pub status: ProcessSegmentStatus,
    pub summary: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ProcessStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessSegmentStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
    Withheld,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UiVisibility {
    UserSafe,
    Withheld,
}

impl Default for UiVisibility {
    fn default() -> Self {
        Self::UserSafe
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStep {
    pub id: String,
    pub order: u32,
    pub kind: ProcessStepKind,
    pub status: ProcessStepStatus,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<ProcessStepDetail>,
    #[serde(default)]
    pub visibility: UiVisibility,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessStepKind {
    Reasoning,
    Activity,
    Command,
    FileRead,
    FileSearch,
    FileEdit,
    Diff,
    Tool,
    Artifact,
    Synthesis,
    Withheld,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessStepStatus {
    Running,
    Complete,
    Failed,
    Withheld,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProcessStepDetail {
    Activity {
        summary: UiSafeText,
        #[serde(rename = "itemCount", default, skip_serializing_if = "Option::is_none")]
        item_count: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        items: Vec<ProcessActivityItem>,
    },
    Command(CommandExecution),
    Diff(ChangeSet),
    Tool {
        #[serde(rename = "toolName")]
        tool_name: UiSafeText,
        #[serde(
            rename = "outputSummary",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        output_summary: Option<UiSafeText>,
        #[serde(
            rename = "durationMs",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        duration_ms: Option<u64>,
    },
    Artifact {
        #[serde(rename = "artifactId")]
        artifact_id: String,
        #[serde(
            rename = "revisionId",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        revision_id: Option<String>,
        media: ArtifactMediaPreview,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessActivityItemKind {
    File,
    Search,
    Tool,
    Command,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessActivityItem {
    pub kind: ProcessActivityItemKind,
    pub label: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<UiSafeText>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextSegment {
    pub id: String,
    pub order: u32,
    pub message_id: String,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolGroupSegment {
    pub id: String,
    pub order: u32,
    pub attempts: Vec<ToolAttempt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolAttempt {
    pub id: String,
    pub order: u32,
    pub tool_use_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub origin: ToolAttemptOrigin,
    pub status: ToolAttemptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_phase: Option<ToolFailurePhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<DecisionRequestState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolAttemptStatus {
    Queued,
    WaitingPermission,
    Running,
    Completed,
    Failed,
    Denied,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolAttemptOrigin {
    Builtin,
    Mcp,
    Plugin,
    App,
    Provider,
    Unknown,
}

impl Default for ToolAttemptOrigin {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolFailurePhase {
    Validation,
    Permission,
    Execution,
    Transport,
    Projection,
}

// ── Decision types ──

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DecisionKind {
    Approve,
    Deny,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DecisionLifetime {
    Once,
    Run,
    Session,
    Persisted,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DecisionMatcherKind {
    ExactCommand,
    ExactArgs,
    ToolName,
    Category,
    PathPrefix,
    GlobPattern,
    ExecuteCodeScript,
    Any,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionMatcherSummary {
    pub kind: DecisionMatcherKind,
    pub label: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionOption {
    pub id: String,
    pub decision: DecisionKind,
    pub label: String,
    pub lifetime: DecisionLifetime,
    pub matcher: DecisionMatcherSummary,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DecisionOperation {
    Read,
    Write,
    Execute,
    Network,
    Mcp,
    Artifact,
    Git,
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTarget {
    pub kind: DecisionTargetKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_label: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DecisionTargetKind {
    File,
    Directory,
    Command,
    Url,
    McpTool,
    Artifact,
    GitRef,
    Workspace,
    Unknown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionPolicy {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DataExposureSecretRisk {
    None,
    Redacted,
    Blocked,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataExposure {
    pub sends_workspace_data: bool,
    pub sends_network_data: bool,
    pub touches_private_path: bool,
    pub secret_risk: DataExposureSecretRisk,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionConfirmation {
    pub expected_text: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DecisionRequestStatus {
    Pending,
    Submitting,
    Approved,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRequestState {
    pub id: String,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub status: DecisionRequestStatus,
    pub operation: DecisionOperation,
    pub target: DecisionTarget,
    pub risk_level: RiskLevel,
    pub reason: String,
    pub policy: DecisionPolicy,
    pub decision_options: Vec<DecisionOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<ConversationEventRef>,
    pub data_exposure: DataExposure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<DecisionConfirmation>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSegment {
    pub id: String,
    pub order: u32,
    pub artifact_id: String,
    #[serde(rename = "artifactKind", default = "default_artifact_kind")]
    pub kind: String,
    #[serde(default = "default_artifact_segment_status")]
    pub status: ArtifactStatus,
    #[serde(default = "default_artifact_segment_source")]
    pub source: ArtifactSource,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<UiSafeText>,
    pub revision: ArtifactRevisionSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMediaPreview {
    pub kind: ArtifactMediaKind,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactMediaKind {
    Image,
    Video,
    Audio,
    File,
}

// ── CommandExecution ──

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecution {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_ref: Option<EvidenceRefId>,
    pub truncated: bool,
    pub redaction_state: EvidenceRedactionState,
    pub risk_level: RiskLevel,
}

// ── ChangeSet ──

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub id: String,
    pub summary: String,
    pub files: Vec<ChangeSetFile>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: ChangeSetFileStatus,
    pub added_lines: u32,
    pub removed_lines: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_patch_ref: Option<EvidenceRefId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_flags: Vec<ChangeSetRiskFlag>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChangeSetFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChangeSetRiskFlag {
    Delete,
    Chmod,
    Binary,
    Large,
    Generated,
}

// ── ArtifactRevisionSummary ──

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactRevisionKind {
    Code,
    Document,
    Image,
    Html,
    Data,
    Media,
    File,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactRevisionStatus {
    Pending,
    Running,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRevisionSummary {
    pub artifact_id: String,
    pub revision_id: String,
    pub kind: ArtifactRevisionKind,
    pub status: ArtifactRevisionStatus,
    pub source_run_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<EvidenceRefId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<ArtifactMediaPreview>,
}

// ── Evidence ref types ──

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct EvidenceRefId(String);

impl EvidenceRefId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for EvidenceRefId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<EvidenceRefId> for String {
    fn from(value: EvidenceRefId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceRefKind {
    CommandOutput,
    DiffPatch,
    ArtifactContent,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceRedactionState {
    Clean,
    Redacted,
    Withheld,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRefSummary {
    pub id: EvidenceRefId,
    pub kind: EvidenceRefKind,
    pub content_type: String,
    pub byte_length: u64,
    pub truncated: bool,
    pub redaction_state: EvidenceRedactionState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequestSegment {
    pub id: String,
    pub order: u32,
    pub request_id: String,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClarificationRequestSegment {
    pub id: String,
    pub order: u32,
    pub request_id: String,
    pub prompt: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssistantNoticeCode {
    ContextCompacted,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoticeSegment {
    pub id: String,
    pub order: u32,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<AssistantNoticeCode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSegment {
    pub id: String,
    pub order: u32,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEventRef {
    pub event_id: String,
    pub cursor: ConversationCursor,
}

fn default_artifact_kind() -> String {
    "file".to_owned()
}

fn default_artifact_segment_status() -> ArtifactStatus {
    ArtifactStatus::Ready
}

fn default_artifact_segment_source() -> ArtifactSource {
    ArtifactSource::Assistant
}

fn contains_private_absolute_path(value: &str) -> bool {
    value.contains("/Users/")
        || value.contains("/home/")
        || value.contains("/private/var/")
        || contains_windows_absolute_path(value)
}

fn redact_private_absolute_paths(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if is_private_path_start(value, index) {
            redacted.push_str(REDACTED);
            index = private_path_end(value, index);
            continue;
        }
        let Some(ch) = value[index..].chars().next() else {
            break;
        };
        redacted.push(ch);
        index += ch.len_utf8();
    }
    redacted
}

fn is_private_path_start(value: &str, index: usize) -> bool {
    let suffix = &value[index..];
    suffix.starts_with("/Users/")
        || suffix.starts_with("/home/")
        || suffix.starts_with("/private/var/")
        || is_windows_absolute_path_at(value, index)
}

fn is_windows_absolute_path_at(value: &str, index: usize) -> bool {
    let bytes = value.as_bytes();
    index + 2 < bytes.len()
        && bytes[index].is_ascii_alphabetic()
        && bytes[index + 1] == b':'
        && matches!(bytes[index + 2], b'\\' | b'/')
}

fn private_path_end(value: &str, start: usize) -> usize {
    let mut index = start;
    while index < value.len() {
        let Some(ch) = value[index..].chars().next() else {
            break;
        };
        if is_private_path_delimiter(ch) {
            break;
        }
        if ch.is_whitespace() {
            let next_index = index + ch.len_utf8();
            if private_path_continues_after_whitespace(value, next_index) {
                index = next_index;
                continue;
            }
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn is_private_path_delimiter(ch: char) -> bool {
    matches!(ch, '"' | '\'' | ')' | ']' | '}' | '<' | '>')
}

fn private_path_continues_after_whitespace(value: &str, start: usize) -> bool {
    let mut index = start;
    while index < value.len() {
        let Some(ch) = value[index..].chars().next() else {
            return false;
        };
        if matches!(ch, ' ' | '\t') {
            index += ch.len_utf8();
            continue;
        }
        break;
    }

    let token_start = index;
    while index < value.len() {
        let Some(ch) = value[index..].chars().next() else {
            break;
        };
        if ch.is_whitespace() || is_private_path_delimiter(ch) {
            break;
        }
        index += ch.len_utf8();
    }
    let token = &value[token_start..index];
    token.contains('/') || token.contains('\\') || token.starts_with('.')
}

fn contains_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    })
}

fn contains_obvious_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization: bearer ")
        || lower.contains("authorization bearer ")
        || lower.contains("authorization: basic ")
        || contains_auth_token(&lower, "bearer")
        || contains_auth_token(&lower, "basic")
        || contains_jwt_like_token(value)
        || contains_secret_assignment(&lower)
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("ghu_")
        || lower.contains("ghs_")
        || lower.contains("ghr_")
        || lower.contains("github_pat_")
        || lower.contains("xoxb-")
        || lower.contains("xoxp-")
        || lower.contains("xoxa-")
        || lower.contains("xoxr-")
        || lower.contains("xoxs-")
        || lower.contains("npm_")
        || lower.contains("lin_api_")
        || lower.contains("secret_")
        || lower.contains("sk_live_")
        || lower.contains("rk_live_")
        || contains_database_url(&lower)
        || lower.contains("-----begin ")
        || value.contains("AKIA")
        || contains_aws_key_like(value)
        || value.contains("AIza")
}

fn contains_auth_token(value: &str, scheme: &str) -> bool {
    value.split_ascii_whitespace().any(|token| token == scheme)
        && value
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0] == scheme && pair[1].len() >= 12)
}

fn contains_jwt_like_token(value: &str) -> bool {
    value.split_ascii_whitespace().any(|token| {
        let token = trim_secret_token_wrapper(token);
        if !token.starts_with("eyJ") {
            return false;
        }
        let parts = token.split('.').collect::<Vec<_>>();
        parts.len() >= 3 && parts[0].len() >= 8 && parts[1].len() >= 8 && parts[2].len() >= 8
    })
}

fn trim_secret_token_wrapper(value: &str) -> &str {
    value.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    })
}

fn contains_database_url(value: &str) -> bool {
    [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
        "amqps://",
    ]
    .iter()
    .any(|scheme| {
        value.find(scheme).is_some_and(|start| {
            let url = value[start..]
                .split_ascii_whitespace()
                .next()
                .unwrap_or_default();
            url.contains('@')
        })
    })
}

fn contains_aws_key_like(value: &str) -> bool {
    value.split_ascii_whitespace().any(|token| {
        let token = trim_secret_token_wrapper(token);
        (token.starts_with("ASIA") || token.starts_with("A3T"))
            && token.len() >= 20
            && token
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
    })
}

fn contains_secret_assignment(value: &str) -> bool {
    [
        "password",
        "passwd",
        "pwd",
        "token",
        "secret",
        "client_secret",
        "refresh_token",
        "access_token",
        "oauth_code",
        "code",
    ]
    .iter()
    .any(|name| contains_secret_assignment_name(value, name))
}

fn contains_secret_assignment_name(value: &str, name: &str) -> bool {
    let mut cursor = 0;
    while cursor < value.len() {
        let Some(relative_start) = value[cursor..].find(name) else {
            return false;
        };
        let start = cursor + relative_start;
        let mut offset = start + name.len();
        offset += ascii_whitespace_prefix_len(&value[offset..]);
        let Some(delimiter) = value[offset..].chars().next() else {
            return false;
        };
        if !matches!(delimiter, ':' | '=') {
            cursor = offset + delimiter.len_utf8();
            continue;
        }
        offset += delimiter.len_utf8();
        offset += ascii_whitespace_prefix_len(&value[offset..]);
        if let Some(quote) = value[offset..]
            .chars()
            .next()
            .filter(|ch| matches!(*ch, '"' | '\''))
        {
            offset += quote.len_utf8();
            let quoted_value_len = value[offset..].find(quote).unwrap_or(value[offset..].len());
            if quoted_value_len >= 8 {
                return true;
            }
            cursor = offset + quoted_value_len;
            continue;
        }
        let value_len = value[offset..]
            .char_indices()
            .take_while(|(_, ch)| !ch.is_ascii_whitespace() && !matches!(*ch, '"' | '\''))
            .last()
            .map_or(0, |(index, ch)| index + ch.len_utf8());
        if value_len >= 8 {
            return true;
        }
        cursor = offset + value_len;
    }
    false
}

fn ascii_whitespace_prefix_len(value: &str) -> usize {
    value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_whitespace())
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}
