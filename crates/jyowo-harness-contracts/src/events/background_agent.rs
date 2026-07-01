use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundAgentState {
    Queued,
    Running,
    WaitingForPermission,
    WaitingForInput,
    Paused,
    Cancelling,
    Cancelled,
    Succeeded,
    Failed,
    Interrupted,
    Recoverable,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentStartedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub conversation_id: SessionId,
    pub attempt_id: RunId,
    pub title: UiSafeText,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentStateChangedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub from: BackgroundAgentState,
    pub to: BackgroundAgentState,
    pub attempt_id: Option<RunId>,
    pub reason: Option<UiSafeText>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentInputRequestedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub request_id: RequestId,
    pub prompt: UiSafeText,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentInputSubmittedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub request_id: RequestId,
    pub input: UiSafeText,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentPermissionRequestedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub tenant_id: TenantId,
    pub conversation_id: SessionId,
    pub request_id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<RunId>,
    pub reason: UiSafeText,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentPermissionResolvedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub tenant_id: TenantId,
    pub conversation_id: SessionId,
    pub request_id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<RunId>,
    pub decision: Decision,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentCancelledEvent {
    pub background_agent_id: BackgroundAgentId,
    pub reason: Option<UiSafeText>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentCompletedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub summary: Option<UiSafeText>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentFailedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub error: UiSafeText,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentInterruptedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub reason: UiSafeText,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentArchivedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundAgentDeletedEvent {
    pub background_agent_id: BackgroundAgentId,
    pub at: DateTime<Utc>,
}
