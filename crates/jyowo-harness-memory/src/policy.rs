//! Memory policy engine.
//!
//! Centralizes memory use/generation/write policy decisions.
//! Resolves global settings, thread settings, provider trust, actor,
//! source evidence, visibility, and external-context state into typed
//! `MemoryPolicyDecision` outcomes.

use harness_contracts::{
    MemoryActor, MemoryEvidence, MemoryEvidenceOrigin, MemoryGlobalSettings,
    MemoryPermissionContext, MemoryPolicyDecision, MemoryPolicyDenyReason, MemorySource,
    MemoryThreadMode, MemoryThreadSettings, MemoryVisibility,
};

/// Central policy engine for memory operations.
///
/// Policy resolution order:
/// 1. Global settings
/// 2. Thread settings (override global)
/// 3. Actor permissions
/// 4. Source trust
/// 5. Visibility escalation
#[derive(Debug, Clone)]
pub struct MemoryPolicyEngine {
    global_settings: MemoryGlobalSettings,
}

impl MemoryPolicyEngine {
    #[must_use]
    pub fn new(global_settings: MemoryGlobalSettings) -> Self {
        Self { global_settings }
    }

    /// Evaluate whether memory recall is allowed.
    pub fn evaluate_recall(
        &self,
        thread: &MemoryThreadSettings,
        _actor: &MemoryActor,
    ) -> MemoryPolicyDecision {
        // 1. Global off
        if !self.global_settings.use_memories {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::GlobalUseDisabled,
            };
        }

        // 2. Thread mode
        match thread.memory_mode {
            MemoryThreadMode::Off => {
                return MemoryPolicyDecision::Deny {
                    reason: MemoryPolicyDenyReason::ThreadUseDisabled,
                };
            }
            MemoryThreadMode::CandidateOnly => {
                return MemoryPolicyDecision::CandidateOnly {
                    reason: MemoryPolicyDenyReason::ThreadUseDisabled,
                };
            }
            _ => {}
        }

        // 3. Thread override: use_memories
        if thread.use_memories == Some(false) {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::ThreadUseDisabled,
            };
        }

        MemoryPolicyDecision::Allow
    }

    /// Evaluate whether memory export is allowed.
    pub fn evaluate_export(
        &self,
        thread: &MemoryThreadSettings,
        _actor: &MemoryActor,
        permission: &MemoryPermissionContext,
    ) -> MemoryPolicyDecision {
        if !self.global_settings.use_memories {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::GlobalUseDisabled,
            };
        }

        if matches!(thread.memory_mode, MemoryThreadMode::Off) || thread.use_memories == Some(false)
        {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::ThreadUseDisabled,
            };
        }

        if !permission.explicit_user_instruction {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::PermissionRequired,
            };
        }

        if permission.include_raw_content && permission.non_interactive_policy_grant {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::PermissionRequired,
            };
        }

        MemoryPolicyDecision::Allow
    }

    /// Evaluate whether a memory write (create/update) is allowed.
    ///
    /// Returns `CandidateOnly` when the write should be staged as a candidate
    /// rather than committed directly into long-term memory.
    pub fn evaluate_write(
        &self,
        thread: &MemoryThreadSettings,
        actor: &MemoryActor,
        evidence: &MemoryEvidence,
        permission: &MemoryPermissionContext,
        target_visibility: &MemoryVisibility,
    ) -> MemoryPolicyDecision {
        // 1. Global generation off
        if !self.global_settings.generate_memories {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::GlobalGenerationDisabled,
            };
        }

        // 2. Thread mode
        match thread.memory_mode {
            MemoryThreadMode::Off | MemoryThreadMode::ReadOnly => {
                return MemoryPolicyDecision::Deny {
                    reason: MemoryPolicyDenyReason::ThreadGenerationDisabled,
                };
            }
            MemoryThreadMode::CandidateOnly => {
                return MemoryPolicyDecision::CandidateOnly {
                    reason: MemoryPolicyDenyReason::ThreadGenerationDisabled,
                };
            }
            _ => {}
        }

        // 3. Thread override: generate_memories
        if thread.generate_memories == Some(false) {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::ThreadGenerationDisabled,
            };
        }

        // 4. External context gate
        if self
            .global_settings
            .disable_generation_when_external_context_used
        {
            if is_external_context_source(&evidence.source)
                || is_external_context_origin(&evidence.origin)
            {
                return MemoryPolicyDecision::Deny {
                    reason: MemoryPolicyDenyReason::ExternalContextGenerationDisabled,
                };
            }
        }

        // 5. Source trust — user input is trusted
        if evidence.source == MemorySource::UserInput
            && matches!(evidence.origin, MemoryEvidenceOrigin::UserMessage { .. })
            && matches!(actor, MemoryActor::User { .. })
        {
            // User explicitly remembering → allow direct write for private/user visibility
            if matches!(
                target_visibility,
                MemoryVisibility::Private { .. } | MemoryVisibility::User { .. }
            ) {
                return MemoryPolicyDecision::Allow;
            }
        }

        // 6. Visibility escalation — team/tenant requires policy
        if matches!(
            target_visibility,
            MemoryVisibility::Team { .. } | MemoryVisibility::Tenant
        ) {
            // Model actors cannot write team/tenant memory without explicit policy.
            if matches!(actor, MemoryActor::Model) && !has_memory_write_grant(permission) {
                return MemoryPolicyDecision::Deny {
                    reason: MemoryPolicyDenyReason::VisibilityEscalationDenied,
                };
            }
            // User actors need explicit instruction for team writes.
            if matches!(actor, MemoryActor::User { .. }) && !has_memory_write_grant(permission) {
                return MemoryPolicyDecision::Deny {
                    reason: MemoryPolicyDenyReason::PermissionRequired,
                };
            }
        }

        // 7. Subagent-derived → candidate by default.
        // A non-interactive grant is valid only for audited team shared-memory
        // writes where the subagent actor and evidence describe the same child.
        if matches!(actor, MemoryActor::Subagent { .. })
            || matches!(evidence.source, MemorySource::SubagentDerived { .. })
        {
            if has_memory_write_grant(permission)
                || has_scoped_subagent_team_grant(permission, actor, evidence, target_visibility)
            {
                return MemoryPolicyDecision::Allow;
            }
            return MemoryPolicyDecision::CandidateOnly {
                reason: MemoryPolicyDenyReason::PermissionRequired,
            };
        }

        // 8. Model-derived / external content → candidate by default
        if !is_trusted_source(&evidence.source) && !has_memory_write_grant(permission) {
            return MemoryPolicyDecision::CandidateOnly {
                reason: MemoryPolicyDenyReason::MissingPolicy,
            };
        }

        // 9. Tool/MCP/Plugin output → candidate by default
        if matches!(
            evidence.origin,
            MemoryEvidenceOrigin::BuiltinToolOutput { .. }
                | MemoryEvidenceOrigin::McpToolOutput { .. }
                | MemoryEvidenceOrigin::PluginOutput { .. }
        ) {
            if has_memory_write_grant(permission) {
                return MemoryPolicyDecision::Allow;
            }
            return MemoryPolicyDecision::CandidateOnly {
                reason: MemoryPolicyDenyReason::MissingPolicy,
            };
        }

        MemoryPolicyDecision::Allow
    }

    /// Evaluate whether generation is allowed (for extraction/consolidation).
    pub fn evaluate_generation(
        &self,
        thread: &MemoryThreadSettings,
        has_external_context: bool,
        permission: &MemoryPermissionContext,
    ) -> MemoryPolicyDecision {
        if !self.global_settings.generate_memories {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::GlobalGenerationDisabled,
            };
        }

        if thread.generate_memories == Some(false) {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::ThreadGenerationDisabled,
            };
        }

        if has_external_context
            && self
                .global_settings
                .disable_generation_when_external_context_used
        {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::ExternalContextGenerationDisabled,
            };
        }

        if !has_memory_generation_grant(permission) {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::PermissionRequired,
            };
        }

        MemoryPolicyDecision::Allow
    }

    /// Evaluate whether a deletion is allowed.
    pub fn evaluate_delete(
        &self,
        thread: &MemoryThreadSettings,
        _actor: &MemoryActor,
        permission: &MemoryPermissionContext,
    ) -> MemoryPolicyDecision {
        if !self.global_settings.use_memories {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::GlobalUseDisabled,
            };
        }

        match thread.memory_mode {
            MemoryThreadMode::Off | MemoryThreadMode::ReadOnly => {
                return MemoryPolicyDecision::Deny {
                    reason: MemoryPolicyDenyReason::ThreadUseDisabled,
                };
            }
            MemoryThreadMode::CandidateOnly => {
                return MemoryPolicyDecision::CandidateOnly {
                    reason: MemoryPolicyDenyReason::ThreadUseDisabled,
                };
            }
            _ => {}
        }

        if !has_memory_write_grant(permission) {
            return MemoryPolicyDecision::Deny {
                reason: MemoryPolicyDenyReason::PermissionRequired,
            };
        }

        MemoryPolicyDecision::Allow
    }
}

/// Check if the source type indicates externally-retrieved content.
fn is_external_context_source(source: &MemorySource) -> bool {
    matches!(
        source,
        MemorySource::WebRetrieval
            | MemorySource::ExternalRetrieval
            | MemorySource::McpToolOutput
            | MemorySource::PluginOutput
    )
}

fn has_memory_write_grant(permission: &MemoryPermissionContext) -> bool {
    permission.explicit_user_instruction
        || (permission.action_plan_id.is_some() && permission.authorization_ticket_id.is_some())
}

fn has_memory_generation_grant(permission: &MemoryPermissionContext) -> bool {
    has_memory_write_grant(permission) || permission.non_interactive_policy_grant
}

fn has_scoped_subagent_team_grant(
    permission: &MemoryPermissionContext,
    actor: &MemoryActor,
    evidence: &MemoryEvidence,
    target_visibility: &MemoryVisibility,
) -> bool {
    if !permission.non_interactive_policy_grant
        || !matches!(target_visibility, MemoryVisibility::Team { .. })
    {
        return false;
    }

    let MemoryActor::Subagent {
        child_session_id: actor_child_session_id,
        agent_id: actor_agent_id,
    } = actor
    else {
        return false;
    };
    let MemorySource::SubagentDerived {
        child_session: source_child_session_id,
    } = &evidence.source
    else {
        return false;
    };
    let MemoryEvidenceOrigin::SubagentOutput {
        child_session_id: origin_child_session_id,
        agent_id: origin_agent_id,
        ..
    } = &evidence.origin
    else {
        return false;
    };

    actor_child_session_id == source_child_session_id
        && actor_child_session_id == origin_child_session_id
        && actor_agent_id == origin_agent_id
}

/// Check if the evidence origin indicates externally-retrieved content.
fn is_external_context_origin(origin: &MemoryEvidenceOrigin) -> bool {
    matches!(
        origin,
        MemoryEvidenceOrigin::WebRetrieval { .. }
            | MemoryEvidenceOrigin::McpToolOutput { .. }
            | MemoryEvidenceOrigin::PluginOutput { .. }
    )
}

/// Check if a memory source is trusted for direct writes.
fn is_trusted_source(source: &MemorySource) -> bool {
    matches!(
        source,
        MemorySource::UserInput | MemorySource::WorkspaceFile | MemorySource::Imported
    )
}

/// Convenience: check if a permission context has explicit user instruction.
pub fn has_explicit_user_instruction(ctx: &MemoryPermissionContext) -> bool {
    ctx.explicit_user_instruction
}
