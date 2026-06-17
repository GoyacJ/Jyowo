use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamCreatedEvent {
    pub team_id: TeamId,
    pub tenant_id: TenantId,
    pub name: String,
    pub topology_kind: TopologyKind,
    pub member_specs_hash: [u8; 32],
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberJoinedEvent {
    pub team_id: TeamId,
    pub agent_id: AgentId,
    pub role: String,
    pub session_id: SessionId,
    pub visibility: ContextVisibility,
    pub spec_snapshot_id: BlobRef,
    pub spec_hash: [u8; 32],
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberLeftEvent {
    pub team_id: TeamId,
    pub agent_id: AgentId,
    pub reason: MemberLeaveReason,
    pub left_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberStalledEvent {
    pub team_id: TeamId,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub last_activity_at: DateTime<Utc>,
    pub stalled_for: Duration,
    pub action: StalledAction,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AgentMessageSentEvent {
    pub team_id: TeamId,
    pub from: AgentId,
    pub to: Recipient,
    pub payload: MessagePayload,
    pub message_id: MessageId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AgentMessageRoutedEvent {
    pub team_id: TeamId,
    pub message_id: MessageId,
    pub resolved_recipients: Vec<AgentId>,
    pub routing_policy: RoutingPolicyKind,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTurnCompletedEvent {
    pub team_id: TeamId,
    pub turn_id: RunId,
    pub participating_agents: Vec<AgentId>,
    pub usage: UsageSnapshot,
    pub transcript_ref: Option<TranscriptRef>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTerminatedEvent {
    pub team_id: TeamId,
    pub reason: TeamTerminationReason,
    pub at: DateTime<Utc>,
}
