//! Tests for the memory policy engine.

use harness_contracts::*;
use harness_memory::MemoryPolicyEngine;

fn global_on() -> MemoryGlobalSettings {
    MemoryGlobalSettings {
        use_memories: true,
        generate_memories: true,
        disable_generation_when_external_context_used: false,
        retention_days: None,
        max_memory_bytes: 1_000_000,
        max_recall_records_per_turn: 20,
        max_recall_chars_per_turn: 50_000,
    }
}

fn global_off() -> MemoryGlobalSettings {
    MemoryGlobalSettings {
        use_memories: false,
        generate_memories: false,
        ..global_on()
    }
}

fn thread(session_id: SessionId, mode: MemoryThreadMode) -> MemoryThreadSettings {
    MemoryThreadSettings {
        session_id,
        use_memories: None,
        generate_memories: None,
        memory_mode: mode,
    }
}

fn user_actor() -> MemoryActor {
    MemoryActor::User {
        user_label: Some("tester".to_owned()),
    }
}

fn model_actor() -> MemoryActor {
    MemoryActor::Model
}

fn make_evidence_origin(source: MemorySource) -> MemoryEvidenceOrigin {
    let sid = SessionId::new();
    let rid = RunId::new();
    let mid = MessageId::new();
    match source {
        MemorySource::UserInput => MemoryEvidenceOrigin::UserMessage {
            session_id: sid,
            run_id: rid,
            message_id: mid,
        },
        _ => MemoryEvidenceOrigin::AssistantMessage {
            session_id: sid,
            run_id: rid,
            message_id: mid,
        },
    }
}

fn make_evidence(source: MemorySource, origin: MemoryEvidenceOrigin) -> MemoryEvidence {
    MemoryEvidence {
        source,
        origin,
        content_hash: ContentHash([0u8; 32]),
        session_id: None,
        run_id: None,
        message_id: None,
        tool_use_id: None,
    }
}

fn no_memory_permission() -> MemoryPermissionContext {
    MemoryPermissionContext {
        explicit_user_instruction: false,
        action_plan_id: None,
        authorization_ticket_id: None,
        non_interactive_policy_grant: false,
    }
}

fn explicit_memory_permission() -> MemoryPermissionContext {
    MemoryPermissionContext {
        explicit_user_instruction: true,
        action_plan_id: Some(ActionPlanId::new()),
        authorization_ticket_id: Some(AuthorizationTicketId::new()),
        non_interactive_policy_grant: false,
    }
}

fn action_plan_only_permission() -> MemoryPermissionContext {
    MemoryPermissionContext {
        explicit_user_instruction: false,
        action_plan_id: Some(ActionPlanId::new()),
        authorization_ticket_id: None,
        non_interactive_policy_grant: false,
    }
}

// ── Global off ──

#[test]
fn global_off_prevents_recall() {
    let engine = MemoryPolicyEngine::new(global_off());
    let sid = SessionId::new();
    let result = engine.evaluate_recall(&thread(sid, MemoryThreadMode::ReadWrite), &user_actor());
    assert!(matches!(
        result,
        MemoryPolicyDecision::Deny {
            reason: MemoryPolicyDenyReason::GlobalUseDisabled
        }
    ));
}

#[test]
fn global_off_prevents_generation() {
    let engine = MemoryPolicyEngine::new(global_off());
    let sid = SessionId::new();
    let evidence = make_evidence(
        MemorySource::AgentDerived,
        MemoryEvidenceOrigin::AssistantMessage {
            session_id: sid,
            run_id: RunId::new(),
            message_id: MessageId::new(),
        },
    );
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &model_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::User {
            user_id: "u".to_owned(),
        },
    );
    assert!(matches!(result, MemoryPolicyDecision::Deny { .. }));
}

// ── Thread overrides ──

#[test]
fn thread_off_overrides_global_on() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    let result = engine.evaluate_recall(&thread(sid, MemoryThreadMode::Off), &user_actor());
    assert!(matches!(
        result,
        MemoryPolicyDecision::Deny {
            reason: MemoryPolicyDenyReason::ThreadUseDisabled
        }
    ));
}

#[test]
fn thread_read_only_allows_recall_but_not_write() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    // Recall allowed in read-only
    let result = engine.evaluate_recall(&thread(sid, MemoryThreadMode::ReadOnly), &user_actor());
    assert!(matches!(result, MemoryPolicyDecision::Allow));

    // Write denied in read-only
    let evidence = make_evidence(
        MemorySource::AgentDerived,
        MemoryEvidenceOrigin::AssistantMessage {
            session_id: sid,
            run_id: RunId::new(),
            message_id: MessageId::new(),
        },
    );
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadOnly),
        &model_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::User {
            user_id: "u".to_owned(),
        },
    );
    assert!(matches!(result, MemoryPolicyDecision::Deny { .. }));
}

// ── User explicit instruction ──

#[test]
fn user_explicit_remember_allows_private_write() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    let origin = MemoryEvidenceOrigin::UserMessage {
        session_id: sid,
        run_id: RunId::new(),
        message_id: MessageId::new(),
    };
    let evidence = make_evidence(MemorySource::UserInput, origin);
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &user_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::Private { session_id: sid },
    );
    assert!(matches!(result, MemoryPolicyDecision::Allow));
}

// ── Model-derived external fact → candidate ──

#[test]
fn model_derived_fact_becomes_candidate() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    let origin = MemoryEvidenceOrigin::AssistantMessage {
        session_id: sid,
        run_id: RunId::new(),
        message_id: MessageId::new(),
    };
    let evidence = make_evidence(MemorySource::AgentDerived, origin);
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &model_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::User {
            user_id: "u".to_owned(),
        },
    );
    assert!(matches!(result, MemoryPolicyDecision::CandidateOnly { .. }));
}

#[test]
fn memory_tool_permission_context_allows_direct_tool_write() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    let evidence = make_evidence(
        MemorySource::ToolOutput,
        MemoryEvidenceOrigin::BuiltinToolOutput {
            tool_name: "memory".to_owned(),
            tool_use_id: ToolUseId::new(),
        },
    );
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &model_actor(),
        &evidence,
        &explicit_memory_permission(),
        &MemoryVisibility::Tenant,
    );
    assert!(matches!(result, MemoryPolicyDecision::Allow));
}

#[test]
fn action_plan_id_alone_does_not_grant_memory_write() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    let evidence = make_evidence(
        MemorySource::UserInput,
        MemoryEvidenceOrigin::UserMessage {
            session_id: sid,
            run_id: RunId::new(),
            message_id: MessageId::new(),
        },
    );
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &user_actor(),
        &evidence,
        &action_plan_only_permission(),
        &MemoryVisibility::Tenant,
    );
    assert!(matches!(
        result,
        MemoryPolicyDecision::Deny {
            reason: MemoryPolicyDenyReason::PermissionRequired
        }
    ));
}

// ── External context blocks generation ──

#[test]
fn external_context_blocks_generation_when_configured() {
    let mut settings = global_on();
    settings.disable_generation_when_external_context_used = true;
    let engine = MemoryPolicyEngine::new(settings);
    let sid = SessionId::new();

    // Model-derived with external source → blocked
    let origin = MemoryEvidenceOrigin::WebRetrieval {
        url_hash: ContentHash([1u8; 32]),
        fetch_tool_use_id: None,
    };
    let evidence = make_evidence(MemorySource::WebRetrieval, origin);
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &model_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::User {
            user_id: "u".to_owned(),
        },
    );
    assert!(matches!(
        result,
        MemoryPolicyDecision::Deny {
            reason: MemoryPolicyDenyReason::ExternalContextGenerationDisabled
        }
    ));
}

// ── Team visibility requires policy ──

#[test]
fn team_visibility_write_denied_without_coordinator_policy() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    let origin = MemoryEvidenceOrigin::AssistantMessage {
        session_id: sid,
        run_id: RunId::new(),
        message_id: MessageId::new(),
    };
    let evidence = make_evidence(MemorySource::AgentDerived, origin);
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &model_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::Team {
            team_id: TeamId::new(),
        },
    );
    // Team write from model actor should be denied without explicit coordinator/supervisor policy
    assert!(matches!(result, MemoryPolicyDecision::Deny { .. }));
}

// ── Policy missing → fail closed ──

#[test]
fn missing_policy_fails_closed() {
    let engine = MemoryPolicyEngine::new(global_on());
    let sid = SessionId::new();
    // Create evidence with a source that requires policy but no policy is registered
    let origin = MemoryEvidenceOrigin::PluginOutput {
        plugin_id: "unknown-plugin".to_owned(),
        tool_name: None,
        tool_use_id: None,
    };
    let evidence = make_evidence(MemorySource::PluginOutput, origin);
    let result = engine.evaluate_write(
        &thread(sid, MemoryThreadMode::ReadWrite),
        &model_actor(),
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::Tenant,
    );
    // Plugin output + tenant visibility → deny without explicit policy
    assert!(!matches!(result, MemoryPolicyDecision::Allow));
}

// ── Subagent writes are candidate-only ──

#[test]
fn subagent_derived_becomes_candidate() {
    let engine = MemoryPolicyEngine::new(global_on());
    let parent_sid = SessionId::new();
    let child_sid = SessionId::new();
    let origin = MemoryEvidenceOrigin::SubagentOutput {
        parent_session_id: parent_sid,
        child_session_id: child_sid,
        run_id: RunId::new(),
        agent_id: None,
    };
    let evidence = make_evidence(
        MemorySource::SubagentDerived {
            child_session: child_sid,
        },
        origin,
    );
    let result = engine.evaluate_write(
        &thread(parent_sid, MemoryThreadMode::ReadWrite),
        &MemoryActor::Subagent {
            child_session_id: child_sid,
            agent_id: None,
        },
        &evidence,
        &no_memory_permission(),
        &MemoryVisibility::User {
            user_id: "u".to_owned(),
        },
    );
    assert!(matches!(result, MemoryPolicyDecision::CandidateOnly { .. }));
}
