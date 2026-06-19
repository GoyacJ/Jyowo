//! Harness event contracts.
//!
//! SPEC: docs/architecture/harness/event-schema.md

pub mod context;
pub mod error;
pub mod execute_code;
pub mod hook;
pub mod mcp;
pub mod memory;
pub mod messages;
pub mod observability;
pub mod permission;
pub mod plugin;
pub mod run;
pub mod sandbox;
pub mod session;
pub mod skill;
pub mod steering;
pub mod subagent;
pub mod team;
pub mod tool;
pub mod types;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ToolLoadingBackendName, ToolName};

pub use context::*;
pub use error::*;
pub use execute_code::*;
pub use hook::*;
pub use mcp::*;
pub use memory::*;
pub use messages::*;
pub use observability::*;
pub use permission::*;
pub use plugin::*;
pub use run::*;
pub use sandbox::*;
pub use session::*;
pub use skill::*;
pub use steering::*;
pub use subagent::*;
pub use team::*;
pub use tool::*;
pub use types::*;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, strum::EnumDiscriminants)]
#[strum_discriminants(name(EventKind), derive(Hash, Serialize, Deserialize, JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SessionCreated(SessionCreatedEvent),
    SessionForked(SessionForkedEvent),
    SessionEnded(SessionEndedEvent),
    SessionReloadRequested(SessionReloadRequestedEvent),
    SessionReloadApplied(SessionReloadAppliedEvent),
    RunStarted(RunStartedEvent),
    RunEnded(RunEndedEvent),
    GraceCallTriggered(GraceCallTriggeredEvent),
    UserMessageAppended(UserMessageAppendedEvent),
    AssistantDeltaProduced(AssistantDeltaProducedEvent),
    AssistantMessageCompleted(AssistantMessageCompletedEvent),
    ToolUseRequested(ToolUseRequestedEvent),
    ToolUseApproved(ToolUseApprovedEvent),
    ToolUseDenied(ToolUseDeniedEvent),
    ToolUseCompleted(ToolUseCompletedEvent),
    ToolUseFailed(ToolUseFailedEvent),
    ToolUseHeartbeat(ToolUseHeartbeatEvent),
    ToolResultOffloaded(ToolResultOffloadedEvent),
    ToolRegistrationShadowed(ToolRegistrationShadowedEvent),
    PermissionRequested(PermissionRequestedEvent),
    PermissionResolved(PermissionResolvedEvent),
    PermissionPersistenceTampered(PermissionPersistenceTamperedEvent),
    PermissionRequestSuppressed(PermissionRequestSuppressedEvent),
    PermissionAwaitingHeartbeat(PermissionAwaitingHeartbeatEvent),
    CredentialPoolSharedAcrossTenants(CredentialPoolSharedAcrossTenantsEvent),
    HookTriggered(HookTriggeredEvent),
    HookRewroteInput(HookRewroteInputEvent),
    HookReturnedAdditionalContext(HookContextPatchEvent),
    HookFailed(HookFailedEvent),
    HookReturnedUnsupported(HookReturnedUnsupportedEvent),
    HookOutcomeInconsistent(HookOutcomeInconsistentEvent),
    HookPanicked(HookPanickedEvent),
    HookPermissionConflict(HookPermissionConflictEvent),
    CompactionApplied(CompactionAppliedEvent),
    ContextBudgetExceeded(ContextBudgetExceededEvent),
    ContextStageTransitioned(ContextStageTransitionedEvent),
    ContextPatchApplied(ContextPatchAppliedEvent),
    McpToolInjected(McpToolInjectedEvent),
    McpConnectionLost(McpConnectionLostEvent),
    McpConnectionRecovered(McpConnectionRecoveredEvent),
    McpOAuthRefresh(McpOAuthRefreshEvent),
    McpElicitationRequested(McpElicitationRequestedEvent),
    McpElicitationResolved(McpElicitationResolvedEvent),
    McpToolsListChanged(McpToolsListChangedEvent),
    McpResourceUpdated(McpResourceUpdatedEvent),
    McpSamplingRequested(McpSamplingRequestedEvent),
    ToolDeferredPoolChanged(ToolDeferredPoolChangedEvent),
    ToolSearchQueried(ToolSearchQueriedEvent),
    ToolSchemaMaterialized(ToolSchemaMaterializedEvent),
    SubagentSpawned(SubagentSpawnedEvent),
    SubagentAnnounced(SubagentAnnouncedEvent),
    SubagentTerminated(SubagentTerminatedEvent),
    SubagentStalled(SubagentStalledEvent),
    SubagentSpawnPaused(SubagentSpawnPausedEvent),
    SubagentPermissionForwarded(SubagentPermissionForwardedEvent),
    SubagentPermissionResolved(SubagentPermissionResolvedEvent),
    TeamCreated(TeamCreatedEvent),
    TeamMemberJoined(TeamMemberJoinedEvent),
    TeamMemberLeft(TeamMemberLeftEvent),
    TeamMemberStalled(TeamMemberStalledEvent),
    AgentMessageSent(AgentMessageSentEvent),
    AgentMessageRouted(AgentMessageRoutedEvent),
    TeamTurnCompleted(TeamTurnCompletedEvent),
    TeamTerminated(TeamTerminatedEvent),
    MemoryUpserted(MemoryUpsertedEvent),
    MemoryExported(MemoryExportedEvent),
    MemoryRecalled(MemoryRecalledEvent),
    MemoryRecallDegraded(MemoryRecallDegradedEvent),
    MemoryRecallSkipped(MemoryRecallSkippedEvent),
    MemoryThreatDetected(MemoryThreatDetectedEvent),
    MemdirOverflow(MemdirOverflowEvent),
    MemoryConsolidationRan(MemoryConsolidationRanEvent),
    SkillLoaded(SkillLoadedEvent),
    SkillRejected(SkillRejectedEvent),
    SkillThreatDetected(SkillThreatDetectedEvent),
    SkillInvoked(SkillInvokedEvent),
    SkillPrerequisiteMissing(SkillPrerequisiteMissingEvent),
    SkillPrerequisiteAdvisory(SkillPrerequisiteAdvisoryEvent),
    UsageAccumulated(UsageAccumulatedEvent),
    TraceSpanCompleted(TraceSpanCompletedEvent),
    PluginLoaded(PluginLoadedEvent),
    PluginRejected(PluginRejectedEvent),
    ManifestValidationFailed(ManifestValidationFailedEvent),
    SandboxExecutionStarted(SandboxExecutionStartedEvent),
    SandboxExecutionCompleted(SandboxExecutionCompletedEvent),
    SandboxActivityHeartbeat(SandboxActivityHeartbeatEvent),
    SandboxActivityTimeoutFired(SandboxActivityTimeoutFiredEvent),
    SandboxOutputSpilled(SandboxOutputSpilledEvent),
    SandboxBackpressureApplied(SandboxBackpressureAppliedEvent),
    SandboxSnapshotCreated(SandboxSnapshotCreatedEvent),
    SandboxContainerLifecycleTransition(SandboxContainerLifecycleTransitionEvent),
    SandboxBackendFailed(SandboxBackendFailedEvent),
    SandboxPostExecutionFailed(SandboxPostExecutionFailedEvent),
    SteeringMessageQueued(SteeringMessageQueuedEvent),
    SteeringMessageApplied(SteeringMessageAppliedEvent),
    SteeringMessageDropped(SteeringMessageDroppedEvent),
    ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent),
    ExecuteCodeWhitelistExtended(ExecuteCodeWhitelistExtendedEvent),
    EngineFailed(EngineFailedEvent),
    UnexpectedError(UnexpectedErrorEvent),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolDeferredPoolChangedEvent {
    pub session_id: crate::SessionId,
    pub added: Vec<DeferredToolHint>,
    pub removed: Vec<ToolName>,
    pub source: ToolPoolChangeSource,
    pub deferred_total: u32,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolSearchQueriedEvent {
    pub session_id: crate::SessionId,
    pub run_id: crate::RunId,
    pub tool_use_id: crate::ToolUseId,
    pub query: String,
    pub query_kind: crate::ToolSearchQueryKind,
    pub scored: Vec<(ToolName, u32)>,
    pub matched: Vec<ToolName>,
    pub truncated_by_max_results: bool,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolSchemaMaterializedEvent {
    pub session_id: crate::SessionId,
    pub run_id: crate::RunId,
    pub tool_use_id: crate::ToolUseId,
    pub names: Vec<ToolName>,
    pub backend: ToolLoadingBackendName,
    pub cache_impact: CacheImpact,
    pub triggered_session_reload: bool,
    pub coalesced_count: u32,
    pub at: chrono::DateTime<chrono::Utc>,
}
