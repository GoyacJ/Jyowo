//! JSON Schema export.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.9

use std::collections::BTreeMap;

use schemars::{schema_for, Schema};

use crate::*;

macro_rules! insert_schema {
    ($schemas:ident, $name:literal, $ty:ty) => {
        $schemas.insert($name.to_owned(), schema_for!($ty));
    };
}

pub fn generate_schema() -> Schema {
    schema_for!(Event)
}

pub fn export_all_schemas() -> BTreeMap<String, Schema> {
    let mut schemas = BTreeMap::new();

    insert_schema!(schemas, "event", Event);
    insert_schema!(schemas, "event_kind", EventKind);
    insert_schema!(schemas, "session_id", SessionId);
    insert_schema!(schemas, "run_id", RunId);
    insert_schema!(schemas, "message_id", MessageId);
    insert_schema!(schemas, "tool_use_id", ToolUseId);
    insert_schema!(schemas, "tenant_id", TenantId);
    insert_schema!(schemas, "decision", Decision);
    insert_schema!(schemas, "decision_scope", DecisionScope);
    insert_schema!(schemas, "decided_by", DecidedBy);
    insert_schema!(schemas, "permission_subject", PermissionSubject);
    insert_schema!(schemas, "tool_result_part", ToolResultPart);
    insert_schema!(schemas, "message", Message);
    insert_schema!(schemas, "message_part", MessagePart);
    insert_schema!(schemas, "blob_ref", BlobRef);
    insert_schema!(schemas, "blob_meta", BlobMeta);
    insert_schema!(schemas, "redact_rules", RedactRules);
    insert_schema!(schemas, "harness_error", HarnessError);
    insert_schema!(schemas, "message_content", MessageContent);
    insert_schema!(schemas, "delta_chunk", DeltaChunk);
    insert_schema!(schemas, "thought_chunk", ThoughtChunk);
    insert_schema!(schemas, "tool_properties", ToolProperties);
    insert_schema!(schemas, "tool_descriptor", ToolDescriptor);
    insert_schema!(schemas, "skill_filter", SkillFilter);
    insert_schema!(schemas, "skill_summary", SkillSummary);
    insert_schema!(schemas, "skill_status", SkillStatus);
    insert_schema!(schemas, "skill_view", SkillView);
    insert_schema!(schemas, "skill_parameter_info", SkillParameterInfo);
    insert_schema!(schemas, "skill_injection_id", SkillInjectionId);
    insert_schema!(schemas, "skill_invocation_receipt", SkillInvocationReceipt);
    insert_schema!(schemas, "rendered_skill", RenderedSkill);
    insert_schema!(schemas, "skill_shell_invocation", SkillShellInvocation);
    insert_schema!(schemas, "context_patch_request", ContextPatchRequest);
    insert_schema!(schemas, "context_patch_source", ContextPatchSource);
    insert_schema!(schemas, "context_patch_lifecycle", ContextPatchLifecycle);
    insert_schema!(schemas, "deny_reason", DenyReason);
    insert_schema!(schemas, "tool_error_payload", ToolErrorPayload);
    insert_schema!(schemas, "hook_event_kind", HookEventKind);
    insert_schema!(schemas, "transport_failure_kind", TransportFailureKind);
    insert_schema!(
        schemas,
        "hook_outcome_discriminant",
        HookOutcomeDiscriminant
    );
    insert_schema!(schemas, "pricing_snapshot_id", PricingSnapshotId);
    insert_schema!(schemas, "model_ref", ModelRef);
    insert_schema!(schemas, "context_stage_id", ContextStageId);
    insert_schema!(schemas, "context_stage_outcome", ContextStageOutcome);
    insert_schema!(schemas, "budget_exceedance_source", BudgetExceedanceSource);
    insert_schema!(schemas, "agent_ref", AgentRef);
    insert_schema!(schemas, "context_visibility", ContextVisibility);
    insert_schema!(schemas, "recipient", Recipient);
    insert_schema!(schemas, "message_payload", MessagePayload);
    insert_schema!(schemas, "sandbox_exit_status", SandboxExitStatus);
    insert_schema!(schemas, "sandbox_output_stream", SandboxOutputStream);
    insert_schema!(schemas, "container_ref", ContainerRef);
    insert_schema!(
        schemas,
        "container_lifecycle_state",
        ContainerLifecycleState
    );
    insert_schema!(
        schemas,
        "container_lifecycle_reason",
        ContainerLifecycleReason
    );
    insert_schema!(
        schemas,
        "elicitation_schema_summary",
        ElicitationSchemaSummary
    );
    insert_schema!(schemas, "elicitation_outcome", ElicitationOutcome);
    insert_schema!(
        schemas,
        "tools_list_changed_disposition",
        ToolsListChangedDisposition
    );
    insert_schema!(schemas, "mcp_resource_update_kind", McpResourceUpdateKind);
    insert_schema!(schemas, "sampling_outcome", SamplingOutcome);
    insert_schema!(
        schemas,
        "plugin_capabilities_summary",
        PluginCapabilitiesSummary
    );
    insert_schema!(schemas, "manifest_origin_ref", ManifestOriginRef);
    insert_schema!(schemas, "rejection_reason", RejectionReason);
    insert_schema!(schemas, "clarify_prompt", ClarifyPrompt);
    insert_schema!(schemas, "clarify_choice", ClarifyChoice);
    insert_schema!(schemas, "clarify_answer", ClarifyAnswer);
    insert_schema!(schemas, "outbound_user_message", OutboundUserMessage);
    insert_schema!(schemas, "user_message_delivery", UserMessageDelivery);

    insert_schema!(schemas, "session_created", SessionCreatedEvent);
    insert_schema!(schemas, "session_forked", SessionForkedEvent);
    insert_schema!(schemas, "session_ended", SessionEndedEvent);
    insert_schema!(
        schemas,
        "session_reload_requested",
        SessionReloadRequestedEvent
    );
    insert_schema!(schemas, "session_reload_applied", SessionReloadAppliedEvent);
    insert_schema!(schemas, "run_started", RunStartedEvent);
    insert_schema!(schemas, "run_ended", RunEndedEvent);
    insert_schema!(schemas, "grace_call_triggered", GraceCallTriggeredEvent);
    insert_schema!(schemas, "user_message_appended", UserMessageAppendedEvent);
    insert_schema!(
        schemas,
        "assistant_delta_produced",
        AssistantDeltaProducedEvent
    );
    insert_schema!(
        schemas,
        "assistant_message_completed",
        AssistantMessageCompletedEvent
    );
    insert_schema!(schemas, "tool_use_requested", ToolUseRequestedEvent);
    insert_schema!(schemas, "tool_use_approved", ToolUseApprovedEvent);
    insert_schema!(schemas, "tool_use_denied", ToolUseDeniedEvent);
    insert_schema!(schemas, "tool_use_completed", ToolUseCompletedEvent);
    insert_schema!(schemas, "tool_use_failed", ToolUseFailedEvent);
    insert_schema!(schemas, "tool_use_heartbeat", ToolUseHeartbeatEvent);
    insert_schema!(schemas, "tool_result_offloaded", ToolResultOffloadedEvent);
    insert_schema!(
        schemas,
        "tool_registration_shadowed",
        ToolRegistrationShadowedEvent
    );
    insert_schema!(schemas, "permission_requested", PermissionRequestedEvent);
    insert_schema!(schemas, "permission_resolved", PermissionResolvedEvent);
    insert_schema!(
        schemas,
        "permission_persistence_tampered",
        PermissionPersistenceTamperedEvent
    );
    insert_schema!(
        schemas,
        "permission_request_suppressed",
        PermissionRequestSuppressedEvent
    );
    insert_schema!(
        schemas,
        "permission_awaiting_heartbeat",
        PermissionAwaitingHeartbeatEvent
    );
    insert_schema!(
        schemas,
        "credential_pool_shared_across_tenants",
        CredentialPoolSharedAcrossTenantsEvent
    );
    insert_schema!(schemas, "hook_triggered", HookTriggeredEvent);
    insert_schema!(schemas, "hook_rewrote_input", HookRewroteInputEvent);
    insert_schema!(
        schemas,
        "hook_returned_additional_context",
        HookContextPatchEvent
    );
    insert_schema!(schemas, "hook_failed", HookFailedEvent);
    insert_schema!(
        schemas,
        "hook_returned_unsupported",
        HookReturnedUnsupportedEvent
    );
    insert_schema!(
        schemas,
        "hook_outcome_inconsistent",
        HookOutcomeInconsistentEvent
    );
    insert_schema!(schemas, "hook_panicked", HookPanickedEvent);
    insert_schema!(
        schemas,
        "hook_permission_conflict",
        HookPermissionConflictEvent
    );
    insert_schema!(schemas, "compaction_applied", CompactionAppliedEvent);
    insert_schema!(
        schemas,
        "context_budget_exceeded",
        ContextBudgetExceededEvent
    );
    insert_schema!(
        schemas,
        "context_stage_transitioned",
        ContextStageTransitionedEvent
    );
    insert_schema!(schemas, "mcp_tool_injected", McpToolInjectedEvent);
    insert_schema!(schemas, "mcp_connection_lost", McpConnectionLostEvent);
    insert_schema!(
        schemas,
        "mcp_connection_recovered",
        McpConnectionRecoveredEvent
    );
    insert_schema!(schemas, "mcp_oauth_refresh", McpOAuthRefreshEvent);
    insert_schema!(schemas, "mcp_oauth_refresh_phase", McpOAuthRefreshPhase);
    insert_schema!(schemas, "mcp_oauth_refresh_outcome", McpOAuthRefreshOutcome);
    insert_schema!(
        schemas,
        "mcp_elicitation_requested",
        McpElicitationRequestedEvent
    );
    insert_schema!(
        schemas,
        "mcp_elicitation_resolved",
        McpElicitationResolvedEvent
    );
    insert_schema!(schemas, "mcp_tools_list_changed", McpToolsListChangedEvent);
    insert_schema!(schemas, "mcp_resource_updated", McpResourceUpdatedEvent);
    insert_schema!(schemas, "mcp_sampling_requested", McpSamplingRequestedEvent);
    insert_schema!(
        schemas,
        "tool_deferred_pool_changed",
        ToolDeferredPoolChangedEvent
    );
    insert_schema!(schemas, "tool_search_queried", ToolSearchQueriedEvent);
    insert_schema!(
        schemas,
        "tool_schema_materialized",
        ToolSchemaMaterializedEvent
    );
    insert_schema!(schemas, "subagent_spawned", SubagentSpawnedEvent);
    insert_schema!(schemas, "subagent_announced", SubagentAnnouncedEvent);
    insert_schema!(schemas, "subagent_context_report", SubagentContextReport);
    insert_schema!(schemas, "subagent_terminated", SubagentTerminatedEvent);
    insert_schema!(schemas, "subagent_stalled", SubagentStalledEvent);
    insert_schema!(schemas, "subagent_spawn_paused", SubagentSpawnPausedEvent);
    insert_schema!(
        schemas,
        "subagent_permission_forwarded",
        SubagentPermissionForwardedEvent
    );
    insert_schema!(
        schemas,
        "subagent_permission_resolved",
        SubagentPermissionResolvedEvent
    );
    insert_schema!(schemas, "team_created", TeamCreatedEvent);
    insert_schema!(schemas, "team_member_joined", TeamMemberJoinedEvent);
    insert_schema!(schemas, "team_member_left", TeamMemberLeftEvent);
    insert_schema!(schemas, "team_member_stalled", TeamMemberStalledEvent);
    insert_schema!(schemas, "agent_message_sent", AgentMessageSentEvent);
    insert_schema!(schemas, "agent_message_routed", AgentMessageRoutedEvent);
    insert_schema!(schemas, "team_turn_completed", TeamTurnCompletedEvent);
    insert_schema!(schemas, "team_terminated", TeamTerminatedEvent);
    insert_schema!(schemas, "memory_upserted", MemoryUpsertedEvent);
    insert_schema!(schemas, "memory_exported", MemoryExportedEvent);
    insert_schema!(schemas, "memory_recalled", MemoryRecalledEvent);
    insert_schema!(schemas, "memory_recall_degraded", MemoryRecallDegradedEvent);
    insert_schema!(schemas, "memory_recall_skipped", MemoryRecallSkippedEvent);
    insert_schema!(schemas, "memory_threat_detected", MemoryThreatDetectedEvent);
    insert_schema!(schemas, "memdir_overflow", MemdirOverflowEvent);
    insert_schema!(
        schemas,
        "memory_consolidation_ran",
        MemoryConsolidationRanEvent
    );
    insert_schema!(schemas, "skill_loaded", SkillLoadedEvent);
    insert_schema!(schemas, "skill_rejected", SkillRejectedEvent);
    insert_schema!(schemas, "skill_rejection_reason", SkillRejectionReason);
    insert_schema!(schemas, "skill_threat_detected", SkillThreatDetectedEvent);
    insert_schema!(schemas, "skill_invoked", SkillInvokedEvent);
    insert_schema!(
        schemas,
        "skill_prerequisite_missing",
        SkillPrerequisiteMissingEvent
    );
    insert_schema!(
        schemas,
        "skill_prerequisite_advisory",
        SkillPrerequisiteAdvisoryEvent
    );
    insert_schema!(schemas, "usage_accumulated", UsageAccumulatedEvent);
    insert_schema!(schemas, "trace_span_completed", TraceSpanCompletedEvent);
    insert_schema!(schemas, "plugin_loaded", PluginLoadedEvent);
    insert_schema!(schemas, "plugin_rejected", PluginRejectedEvent);
    insert_schema!(
        schemas,
        "manifest_validation_failed",
        ManifestValidationFailedEvent
    );
    insert_schema!(
        schemas,
        "sandbox_execution_started",
        SandboxExecutionStartedEvent
    );
    insert_schema!(
        schemas,
        "sandbox_execution_completed",
        SandboxExecutionCompletedEvent
    );
    insert_schema!(
        schemas,
        "sandbox_activity_heartbeat",
        SandboxActivityHeartbeatEvent
    );
    insert_schema!(
        schemas,
        "sandbox_activity_timeout_fired",
        SandboxActivityTimeoutFiredEvent
    );
    insert_schema!(schemas, "sandbox_output_spilled", SandboxOutputSpilledEvent);
    insert_schema!(
        schemas,
        "sandbox_backpressure_applied",
        SandboxBackpressureAppliedEvent
    );
    insert_schema!(
        schemas,
        "sandbox_snapshot_created",
        SandboxSnapshotCreatedEvent
    );
    insert_schema!(
        schemas,
        "sandbox_container_lifecycle_transition",
        SandboxContainerLifecycleTransitionEvent
    );
    insert_schema!(schemas, "sandbox_backend_failed", SandboxBackendFailedEvent);
    insert_schema!(
        schemas,
        "sandbox_post_execution_failed",
        SandboxPostExecutionFailedEvent
    );
    insert_schema!(
        schemas,
        "steering_message_queued",
        SteeringMessageQueuedEvent
    );
    insert_schema!(
        schemas,
        "steering_message_applied",
        SteeringMessageAppliedEvent
    );
    insert_schema!(
        schemas,
        "steering_message_dropped",
        SteeringMessageDroppedEvent
    );
    insert_schema!(
        schemas,
        "execute_code_step_invoked",
        ExecuteCodeStepInvokedEvent
    );
    insert_schema!(
        schemas,
        "execute_code_whitelist_extended",
        ExecuteCodeWhitelistExtendedEvent
    );
    insert_schema!(schemas, "engine_failed", EngineFailedEvent);
    insert_schema!(schemas, "unexpected_error", UnexpectedErrorEvent);

    schemas
}
