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
    insert_schema!(schemas, "action_plan_id", ActionPlanId);
    insert_schema!(schemas, "action_plan_hash", ActionPlanHash);
    insert_schema!(schemas, "sandbox_policy_hash", SandboxPolicyHash);
    insert_schema!(schemas, "authorization_ticket_id", AuthorizationTicketId);
    insert_schema!(schemas, "decision", Decision);
    insert_schema!(schemas, "decision_scope", DecisionScope);
    insert_schema!(schemas, "decided_by", DecidedBy);
    insert_schema!(schemas, "permission_subject", PermissionSubject);
    insert_schema!(schemas, "permission_review", PermissionReview);
    insert_schema!(schemas, "permission_confirmation", PermissionConfirmation);
    insert_schema!(schemas, "action_resource", ActionResource);
    insert_schema!(schemas, "tool_execution_channel", ToolExecutionChannel);
    insert_schema!(schemas, "tool_action_plan", ToolActionPlan);
    insert_schema!(schemas, "mcp_resource_operation", McpResourceOperation);
    insert_schema!(schemas, "mcp_prompt_operation", McpPromptOperation);
    insert_schema!(schemas, "mcp_transport_target", McpTransportTarget);
    insert_schema!(schemas, "tool_result_part", ToolResultPart);
    insert_schema!(schemas, "message", Message);
    insert_schema!(schemas, "message_part", MessagePart);
    insert_schema!(schemas, "model_protocol", ModelProtocol);
    insert_schema!(schemas, "model_modality", ModelModality);
    insert_schema!(
        schemas,
        "conversation_model_capability",
        ConversationModelCapability
    );
    insert_schema!(schemas, "run_model_snapshot", RunModelSnapshot);
    insert_schema!(schemas, "agent_capability_kind", AgentCapabilityKind);
    insert_schema!(
        schemas,
        "agent_capability_unavailable_reason",
        AgentCapabilityUnavailableReason
    );
    insert_schema!(
        schemas,
        "agent_capabilities_payload",
        AgentCapabilitiesPayload
    );
    insert_schema!(schemas, "agent_profile", AgentProfile);
    insert_schema!(schemas, "agent_profile_scope", AgentProfileScope);
    insert_schema!(
        schemas,
        "agent_profile_model_override",
        AgentProfileModelOverride
    );
    insert_schema!(
        schemas,
        "agent_profile_sandbox_inheritance",
        AgentProfileSandboxInheritance
    );
    insert_schema!(
        schemas,
        "agent_profile_memory_scope",
        AgentProfileMemoryScope
    );
    insert_schema!(
        schemas,
        "agent_profile_context_mode",
        AgentProfileContextMode
    );
    insert_schema!(schemas, "agent_tool_policy", AgentToolPolicy);
    insert_schema!(schemas, "agent_team_run_config", AgentTeamRunConfig);
    insert_schema!(schemas, "agent_team_topology", AgentTeamTopology);
    insert_schema!(
        schemas,
        "agent_team_shared_memory_policy",
        AgentTeamSharedMemoryPolicy
    );
    insert_schema!(schemas, "agent_use_policy", AgentUsePolicy);
    insert_schema!(
        schemas,
        "background_agent_tool_start_request",
        BackgroundAgentToolStartRequest
    );
    insert_schema!(
        schemas,
        "background_agent_tool_session_snapshot",
        BackgroundAgentToolSessionSnapshot
    );
    insert_schema!(
        schemas,
        "background_agent_tool_start_response",
        BackgroundAgentToolStartResponse
    );
    insert_schema!(
        schemas,
        "agent_workspace_isolation_mode",
        AgentWorkspaceIsolationMode
    );
    insert_schema!(
        schemas,
        "provider_service_capability",
        ProviderServiceCapability
    );
    insert_schema!(schemas, "capability_route_kind", CapabilityRouteKind);
    insert_schema!(schemas, "provider_probe_status", ProviderProbeStatus);
    insert_schema!(schemas, "provider_probe_error_kind", ProviderProbeErrorKind);
    insert_schema!(schemas, "provider_probe_snapshot", ProviderProbeSnapshot);
    insert_schema!(schemas, "model_usage_bucket", ModelUsageBucket);
    insert_schema!(schemas, "model_usage_period", ModelUsagePeriod);
    insert_schema!(schemas, "model_usage_window", ModelUsageWindow);
    insert_schema!(schemas, "model_usage_summary", ModelUsageSummary);
    insert_schema!(schemas, "official_quota_scope", OfficialQuotaScope);
    insert_schema!(schemas, "official_quota_status", OfficialQuotaStatus);
    insert_schema!(schemas, "official_quota_snapshot", OfficialQuotaSnapshot);
    insert_schema!(schemas, "capability_route_health", CapabilityRouteHealth);
    insert_schema!(
        schemas,
        "provider_capability_route",
        ProviderCapabilityRoute
    );
    insert_schema!(
        schemas,
        "provider_capability_route_settings",
        ProviderCapabilityRouteSettings
    );
    insert_schema!(
        schemas,
        "provider_capability_route_option",
        ProviderCapabilityRouteOption
    );
    insert_schema!(
        schemas,
        "list_provider_capability_route_options_response",
        ListProviderCapabilityRouteOptionsResponse
    );
    insert_schema!(
        schemas,
        "provider_runtime_capability",
        ProviderRuntimeCapability
    );
    insert_schema!(
        schemas,
        "conversation_context_reference",
        ConversationContextReference
    );
    insert_schema!(
        schemas,
        "conversation_attachment_reference",
        ConversationAttachmentReference
    );
    insert_schema!(schemas, "conversation_turn_input", ConversationTurnInput);
    insert_schema!(schemas, "ui_safe_text", UiSafeText);
    insert_schema!(schemas, "conversation_cursor", ConversationCursor);
    insert_schema!(schemas, "conversation_summary", ConversationSummary);
    insert_schema!(schemas, "conversation_message", ConversationMessage);
    insert_schema!(schemas, "conversation_snapshot", ConversationSnapshot);
    insert_schema!(
        schemas,
        "conversation_timeline_event",
        ConversationTimelineEvent
    );
    insert_schema!(
        schemas,
        "conversation_timeline_page",
        ConversationTimelinePage
    );
    insert_schema!(
        schemas,
        "conversation_worktree_page",
        ConversationWorktreePage
    );
    insert_schema!(
        schemas,
        "conversation_inspector_selection",
        ConversationInspectorSelection
    );
    insert_schema!(
        schemas,
        "conversation_inspector_item",
        ConversationInspectorItem
    );
    insert_schema!(
        schemas,
        "conversation_inspector_item_response",
        ConversationInspectorItemResponse
    );
    insert_schema!(schemas, "conversation_turn_cursor", ConversationTurnCursor);
    insert_schema!(schemas, "conversation_turn", ConversationTurn);
    insert_schema!(
        schemas,
        "conversation_turn_user_message",
        ConversationTurnUserMessage
    );
    insert_schema!(schemas, "assistant_work", AssistantWork);
    insert_schema!(
        schemas,
        "assistant_work_model_snapshot",
        AssistantWorkModelSnapshot
    );
    insert_schema!(schemas, "assistant_work_status", AssistantWorkStatus);
    insert_schema!(schemas, "assistant_segment", AssistantSegment);
    insert_schema!(schemas, "agent_activity_segment", AgentActivitySegment);
    insert_schema!(schemas, "agent_activity_kind", AgentActivityKind);
    insert_schema!(schemas, "agent_activity_status", AgentActivityStatus);
    insert_schema!(
        schemas,
        "agent_activity_permission_state",
        AgentActivityPermissionState
    );
    insert_schema!(schemas, "process_segment", ProcessSegment);
    insert_schema!(schemas, "process_segment_status", ProcessSegmentStatus);
    insert_schema!(schemas, "process_step", ProcessStep);
    insert_schema!(schemas, "process_step_kind", ProcessStepKind);
    insert_schema!(schemas, "process_step_status", ProcessStepStatus);
    insert_schema!(schemas, "process_step_detail", ProcessStepDetail);
    insert_schema!(schemas, "text_segment", TextSegment);
    insert_schema!(schemas, "tool_group_segment", ToolGroupSegment);
    insert_schema!(schemas, "tool_attempt", ToolAttempt);
    insert_schema!(schemas, "tool_attempt_status", ToolAttemptStatus);
    insert_schema!(schemas, "tool_attempt_origin", ToolAttemptOrigin);
    insert_schema!(schemas, "tool_failure_phase", ToolFailurePhase);
    insert_schema!(schemas, "decision_kind", DecisionKind);
    insert_schema!(schemas, "decision_lifetime", DecisionLifetime);
    insert_schema!(schemas, "decision_matcher_kind", DecisionMatcherKind);
    insert_schema!(schemas, "decision_matcher_summary", DecisionMatcherSummary);
    insert_schema!(schemas, "decision_option", DecisionOption);
    insert_schema!(schemas, "decision_operation", DecisionOperation);
    insert_schema!(schemas, "decision_target_kind", DecisionTargetKind);
    insert_schema!(schemas, "decision_target", DecisionTarget);
    insert_schema!(schemas, "risk_level", RiskLevel);
    insert_schema!(schemas, "decision_policy", DecisionPolicy);
    insert_schema!(schemas, "data_exposure_secret_risk", DataExposureSecretRisk);
    insert_schema!(schemas, "data_exposure", DataExposure);
    insert_schema!(schemas, "decision_confirmation", DecisionConfirmation);
    insert_schema!(schemas, "decision_request_status", DecisionRequestStatus);
    insert_schema!(schemas, "decision_request_state", DecisionRequestState);
    insert_schema!(schemas, "command_execution", CommandExecution);
    insert_schema!(schemas, "change_set", ChangeSet);
    insert_schema!(schemas, "change_set_file", ChangeSetFile);
    insert_schema!(schemas, "change_set_file_status", ChangeSetFileStatus);
    insert_schema!(schemas, "change_set_risk_flag", ChangeSetRiskFlag);
    insert_schema!(schemas, "artifact_revision_kind", ArtifactRevisionKind);
    insert_schema!(schemas, "artifact_revision_status", ArtifactRevisionStatus);
    insert_schema!(
        schemas,
        "artifact_revision_summary",
        ArtifactRevisionSummary
    );
    insert_schema!(schemas, "evidence_ref_id", EvidenceRefId);
    insert_schema!(schemas, "evidence_ref_kind", EvidenceRefKind);
    insert_schema!(schemas, "evidence_redaction_state", EvidenceRedactionState);
    insert_schema!(schemas, "evidence_ref_summary", EvidenceRefSummary);
    insert_schema!(schemas, "ui_visibility", UiVisibility);
    insert_schema!(schemas, "artifact_segment", ArtifactSegment);
    insert_schema!(schemas, "artifact_media_preview", ArtifactMediaPreview);
    insert_schema!(schemas, "artifact_media_kind", ArtifactMediaKind);
    insert_schema!(schemas, "review_request_segment", ReviewRequestSegment);
    insert_schema!(
        schemas,
        "clarification_request_segment",
        ClarificationRequestSegment
    );
    insert_schema!(schemas, "notice_segment", NoticeSegment);
    insert_schema!(schemas, "error_segment", ErrorSegment);
    insert_schema!(schemas, "conversation_event_ref", ConversationEventRef);
    insert_schema!(schemas, "blob_ref", BlobRef);
    insert_schema!(schemas, "blob_meta", BlobMeta);
    insert_schema!(schemas, "redact_rules", RedactRules);
    insert_schema!(schemas, "harness_error", HarnessError);
    insert_schema!(schemas, "message_content", MessageContent);
    insert_schema!(schemas, "delta_chunk", DeltaChunk);
    insert_schema!(schemas, "thought_chunk", ThoughtChunk);
    insert_schema!(schemas, "reasoning_summary_chunk", ReasoningSummaryChunk);
    insert_schema!(schemas, "tool_properties", ToolProperties);
    insert_schema!(schemas, "tool_descriptor", ToolDescriptor);
    insert_schema!(schemas, "tool_service_binding", ToolServiceBinding);
    insert_schema!(schemas, "tool_profile", ToolProfile);
    insert_schema!(schemas, "diagnostics_request", DiagnosticsRequest);
    insert_schema!(schemas, "diagnostics_result", DiagnosticsResult);
    insert_schema!(schemas, "process_start_request", ProcessStartRequest);
    insert_schema!(schemas, "process_read_request", ProcessReadRequest);
    insert_schema!(schemas, "process_stop_request", ProcessStopRequest);
    insert_schema!(schemas, "process_start_result", ProcessStartResult);
    insert_schema!(schemas, "process_read_result", ProcessReadResult);
    insert_schema!(schemas, "process_stop_result", ProcessStopResult);
    insert_schema!(schemas, "diagnostic_item", DiagnosticItem);
    insert_schema!(schemas, "automation_spec", AutomationSpec);
    insert_schema!(schemas, "automation_run_record", AutomationRunRecord);
    insert_schema!(
        schemas,
        "provider_service_adapter_availability",
        ProviderServiceAdapterAvailability
    );
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
    insert_schema!(schemas, "mcp_server_scope", McpServerScope);
    insert_schema!(schemas, "sandbox_preflight_status", SandboxPreflightStatus);
    insert_schema!(
        schemas,
        "sandbox_preflight_passed_event",
        SandboxPreflightPassedEvent
    );
    insert_schema!(
        schemas,
        "sandbox_preflight_failed_event",
        SandboxPreflightFailedEvent
    );
    insert_schema!(schemas, "rejection_reason", RejectionReason);
    insert_schema!(schemas, "plugin_summary", PluginSummary);
    insert_schema!(schemas, "plugin_detail", PluginDetail);
    insert_schema!(schemas, "plugin_install_report", PluginInstallReport);
    insert_schema!(schemas, "plugin_operation_status", PluginOperationStatus);
    insert_schema!(schemas, "plugin_operation_result", PluginOperationResult);
    insert_schema!(schemas, "plugin_config_update", PluginConfigUpdate);
    insert_schema!(schemas, "plugin_recent_event", PluginRecentEvent);
    insert_schema!(
        schemas,
        "plugin_runtime_capability",
        PluginRuntimeCapability
    );
    insert_schema!(
        schemas,
        "plugin_runtime_rpc_request",
        PluginRuntimeRpcRequest
    );
    insert_schema!(
        schemas,
        "plugin_runtime_rpc_response",
        PluginRuntimeRpcResponse
    );
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
    insert_schema!(
        schemas,
        "assistant_review_requested",
        AssistantReviewRequestedEvent
    );
    insert_schema!(
        schemas,
        "assistant_clarification_requested",
        AssistantClarificationRequestedEvent
    );
    insert_schema!(schemas, "assistant_notice", AssistantNoticeEvent);
    insert_schema!(schemas, "artifact_created", ArtifactCreatedEvent);
    insert_schema!(schemas, "artifact_updated", ArtifactUpdatedEvent);
    insert_schema!(schemas, "artifact_status", ArtifactStatus);
    insert_schema!(schemas, "artifact_source", ArtifactSource);
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
        "permission_decision_option",
        PermissionDecisionOption
    );
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
    insert_schema!(schemas, "team_task_updated", TeamTaskUpdatedEvent);
    insert_schema!(schemas, "team_terminated", TeamTerminatedEvent);
    insert_schema!(schemas, "background_agent_state", BackgroundAgentState);
    insert_schema!(
        schemas,
        "background_agent_started",
        BackgroundAgentStartedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_state_changed",
        BackgroundAgentStateChangedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_input_requested",
        BackgroundAgentInputRequestedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_input_submitted",
        BackgroundAgentInputSubmittedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_permission_requested",
        BackgroundAgentPermissionRequestedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_permission_resolved",
        BackgroundAgentPermissionResolvedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_cancelled",
        BackgroundAgentCancelledEvent
    );
    insert_schema!(
        schemas,
        "background_agent_completed",
        BackgroundAgentCompletedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_failed",
        BackgroundAgentFailedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_interrupted",
        BackgroundAgentInterruptedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_archived",
        BackgroundAgentArchivedEvent
    );
    insert_schema!(
        schemas,
        "background_agent_deleted",
        BackgroundAgentDeletedEvent
    );
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
    insert_schema!(schemas, "plugin_failed", PluginFailedEvent);
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

    insert_schema!(schemas, "memory_trace_id", MemoryTraceId);
    insert_schema!(schemas, "memory_candidate_id", MemoryCandidateId);
    insert_schema!(schemas, "memory_record", MemoryRecord);
    insert_schema!(schemas, "memory_record_draft", MemoryRecordDraft);
    insert_schema!(schemas, "memory_evidence", MemoryEvidence);
    insert_schema!(schemas, "memory_evidence_origin", MemoryEvidenceOrigin);
    insert_schema!(schemas, "memory_candidate", MemoryCandidate);
    insert_schema!(
        schemas,
        "memory_candidate_operation",
        MemoryCandidateOperation
    );
    insert_schema!(schemas, "memory_candidate_state", MemoryCandidateState);
    insert_schema!(
        schemas,
        "memory_candidate_list_item",
        MemoryCandidateListItem
    );
    insert_schema!(schemas, "memory_score_breakdown", MemoryScoreBreakdown);
    insert_schema!(schemas, "memory_recall_trace", MemoryRecallTrace);
    insert_schema!(
        schemas,
        "memory_recall_trace_summary",
        MemoryRecallTraceSummary
    );
    insert_schema!(schemas, "memory_provider_trace", MemoryProviderTrace);
    insert_schema!(schemas, "memory_candidate_trace", MemoryCandidateTrace);
    insert_schema!(schemas, "memory_injected_trace", MemoryInjectedTrace);
    insert_schema!(schemas, "memory_dropped_trace", MemoryDroppedTrace);
    insert_schema!(schemas, "memory_drop_reason", MemoryDropReason);
    insert_schema!(schemas, "memory_policy_decision", MemoryPolicyDecision);
    insert_schema!(schemas, "memory_policy_deny_reason", MemoryPolicyDenyReason);
    insert_schema!(schemas, "memory_global_settings", MemoryGlobalSettings);
    insert_schema!(schemas, "memory_thread_settings", MemoryThreadSettings);
    insert_schema!(schemas, "memory_thread_mode", MemoryThreadMode);
    insert_schema!(schemas, "memory_actor", MemoryActor);
    insert_schema!(schemas, "memory_provider_trust", MemoryProviderTrust);
    insert_schema!(schemas, "memory_provider_kind", MemoryProviderKind);
    insert_schema!(
        schemas,
        "memory_provider_durability",
        MemoryProviderDurability
    );
    insert_schema!(schemas, "memory_visibility_class", MemoryVisibilityClass);
    insert_schema!(
        schemas,
        "memory_provider_descriptor",
        MemoryProviderDescriptor
    );
    insert_schema!(
        schemas,
        "memory_provider_selection_policy",
        MemoryProviderSelectionPolicy
    );
    insert_schema!(schemas, "memory_tool_args", MemoryToolArgs);
    insert_schema!(schemas, "memory_tool_action", MemoryToolAction);
    insert_schema!(schemas, "memory_tool_request", MemoryToolRequest);
    insert_schema!(schemas, "memory_tool_response", MemoryToolResponse);
    insert_schema!(schemas, "memory_tool_state", MemoryToolState);
    insert_schema!(schemas, "memory_tool_record_view", MemoryToolRecordView);
    insert_schema!(schemas, "memory_tool_denial", MemoryToolDenial);
    insert_schema!(schemas, "memory_redaction_summary", MemoryRedactionSummary);
    insert_schema!(schemas, "memory_takes_effect", MemoryTakesEffect);
    insert_schema!(
        schemas,
        "memory_permission_context",
        MemoryPermissionContext
    );
    insert_schema!(schemas, "memory_metadata", MemoryMetadata);
    insert_schema!(
        schemas,
        "get_memory_settings_request",
        GetMemorySettingsRequest
    );
    insert_schema!(
        schemas,
        "get_memory_settings_response",
        GetMemorySettingsResponse
    );
    insert_schema!(
        schemas,
        "update_memory_settings_request",
        UpdateMemorySettingsRequest
    );
    insert_schema!(
        schemas,
        "update_memory_settings_response",
        UpdateMemorySettingsResponse
    );
    insert_schema!(
        schemas,
        "get_thread_memory_settings_request",
        GetThreadMemorySettingsRequest
    );
    insert_schema!(
        schemas,
        "get_thread_memory_settings_response",
        GetThreadMemorySettingsResponse
    );
    insert_schema!(
        schemas,
        "update_thread_memory_settings_request",
        UpdateThreadMemorySettingsRequest
    );
    insert_schema!(
        schemas,
        "update_thread_memory_settings_response",
        UpdateThreadMemorySettingsResponse
    );
    insert_schema!(
        schemas,
        "list_memory_candidates_request",
        ListMemoryCandidatesRequest
    );
    insert_schema!(
        schemas,
        "list_memory_candidates_response",
        ListMemoryCandidatesResponse
    );
    insert_schema!(
        schemas,
        "approve_memory_candidate_request",
        ApproveMemoryCandidateRequest
    );
    insert_schema!(
        schemas,
        "approve_memory_candidate_response",
        ApproveMemoryCandidateResponse
    );
    insert_schema!(
        schemas,
        "reject_memory_candidate_request",
        RejectMemoryCandidateRequest
    );
    insert_schema!(
        schemas,
        "reject_memory_candidate_response",
        RejectMemoryCandidateResponse
    );
    insert_schema!(
        schemas,
        "merge_memory_candidate_request",
        MergeMemoryCandidateRequest
    );
    insert_schema!(
        schemas,
        "merge_memory_candidate_response",
        MergeMemoryCandidateResponse
    );
    insert_schema!(
        schemas,
        "list_memory_recall_traces_request",
        ListMemoryRecallTracesRequest
    );
    insert_schema!(
        schemas,
        "list_memory_recall_traces_response",
        ListMemoryRecallTracesResponse
    );
    insert_schema!(
        schemas,
        "get_memory_recall_trace_request",
        GetMemoryRecallTraceRequest
    );
    insert_schema!(
        schemas,
        "get_memory_recall_trace_response",
        GetMemoryRecallTraceResponse
    );
    insert_schema!(
        schemas,
        "get_model_request_preview_request",
        GetModelRequestPreviewRequest
    );
    insert_schema!(
        schemas,
        "get_model_request_preview_response",
        GetModelRequestPreviewResponse
    );
    insert_schema!(
        schemas,
        "memory_model_request_preview",
        MemoryModelRequestPreview
    );
    insert_schema!(
        schemas,
        "memory_model_request_preview_section",
        MemoryModelRequestPreviewSection
    );

    schemas
}
