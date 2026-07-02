use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPreflightStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxPreflightPassedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub status: SandboxPreflightStatus,
    pub policy: SandboxPolicySummary,
    pub policy_hash: SandboxPolicyHash,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxPreflightFailedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub status: SandboxPreflightStatus,
    pub policy: SandboxPolicySummary,
    pub policy_hash: SandboxPolicyHash,
    pub reason: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxExecutionStartedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub fingerprint: ExecFingerprint,
    pub policy: SandboxPolicySummary,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxExecutionCompletedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub fingerprint: ExecFingerprint,
    pub exit_status: SandboxExitStatus,
    pub stdout_bytes_observed: u64,
    pub stderr_bytes_observed: u64,
    pub duration_ms: u64,
    pub overflow: Option<SandboxOverflowSummary>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxActivityHeartbeatEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub since_last_io_ms: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxActivityTimeoutFiredEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub configured_timeout: Duration,
    pub kill_scope: KillScope,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxOutputSpilledEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub stream: SandboxOutputStream,
    pub blob_ref: BlobRef,
    pub head_bytes: u32,
    pub tail_bytes: u32,
    pub original_bytes: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxBackpressureAppliedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub queued_bytes: u64,
    pub paused_for_ms: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxSnapshotCreatedEvent {
    pub session_id: SessionId,
    pub backend_id: String,
    pub kind: SessionSnapshotKind,
    pub size_bytes: u64,
    pub content_hash: [u8; 32],
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxContainerLifecycleTransitionEvent {
    pub session_id: SessionId,
    pub backend_id: String,
    pub container_ref: ContainerRef,
    pub from: ContainerLifecycleState,
    pub to: ContainerLifecycleState,
    pub reason: ContainerLifecycleReason,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackendFailurePhase {
    Execute,
    Wait,
    Snapshot,
    Restore,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxBackendFailedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub phase: SandboxBackendFailurePhase,
    pub error: SandboxError,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxPostExecutionFailedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub backend_id: String,
    pub error: SandboxError,
    pub at: DateTime<Utc>,
}
