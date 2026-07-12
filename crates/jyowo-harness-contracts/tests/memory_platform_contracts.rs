//! Memory platform contract tests.
//!
//! Tests that every type defined in "Target Contracts" is present and has stable serde tags.

use harness_contracts::*;
use serde_json::json;

fn make_meta() -> MemoryMetadata {
    MemoryMetadata {
        ttl: None,
        tags: vec![],
        source_trust: 0.5,
    }
}

fn new_session_id() -> SessionId {
    SessionId::new()
}
fn new_run_id() -> RunId {
    RunId::new()
}
fn new_message_id() -> MessageId {
    MessageId::new()
}
fn new_tool_use_id() -> ToolUseId {
    ToolUseId::new()
}
fn new_memory_id() -> MemoryId {
    MemoryId::new()
}
fn new_tenant_id() -> TenantId {
    TenantId::SINGLE
}

// ── MemorySource ──

#[test]
fn memory_source_variants_roundtrip() {
    let variants: Vec<MemorySource> = vec![
        MemorySource::UserInput,
        MemorySource::AgentDerived,
        MemorySource::SubagentDerived {
            child_session: new_session_id(),
        },
        MemorySource::ToolOutput,
        MemorySource::McpToolOutput,
        MemorySource::PluginOutput,
        MemorySource::WebRetrieval,
        MemorySource::WorkspaceFile,
        MemorySource::ExternalRetrieval,
        MemorySource::Imported,
        MemorySource::Consolidated {
            from: vec![new_memory_id()],
        },
    ];
    for v in &variants {
        let json = serde_json::to_value(v).expect("serialize MemorySource");
        let back: MemorySource = serde_json::from_value(json).expect("deserialize MemorySource");
        assert_eq!(v, &back);
    }
}

// ── MemoryEvidenceOrigin ──

#[test]
fn memory_evidence_origin_roundtrip() {
    let sid = new_session_id();
    let rid = new_run_id();
    let mid = new_message_id();
    let tool_id = new_tool_use_id();
    let mem_id = new_memory_id();

    let variants: Vec<MemoryEvidenceOrigin> = vec![
        MemoryEvidenceOrigin::UserMessage {
            session_id: sid,
            run_id: rid,
            message_id: mid,
        },
        MemoryEvidenceOrigin::AssistantMessage {
            session_id: sid,
            run_id: rid,
            message_id: mid,
        },
        MemoryEvidenceOrigin::SubagentOutput {
            parent_session_id: sid,
            child_session_id: sid,
            run_id: rid,
            agent_id: None,
        },
        MemoryEvidenceOrigin::BuiltinToolOutput {
            tool_name: "memory".to_owned(),
            tool_use_id: tool_id,
        },
        MemoryEvidenceOrigin::McpToolOutput {
            server_id: "srv".to_owned(),
            tool_name: "mem".to_owned(),
            tool_use_id: tool_id,
        },
        MemoryEvidenceOrigin::PluginOutput {
            plugin_id: "p".to_owned(),
            tool_name: Some("t".to_owned()),
            tool_use_id: Some(tool_id),
        },
        MemoryEvidenceOrigin::WebRetrieval {
            url_hash: ContentHash([1u8; 32]),
            fetch_tool_use_id: Some(tool_id),
        },
        MemoryEvidenceOrigin::WorkspaceFile {
            workspace_id: WorkspaceId::new(),
            path_hash: ContentHash([2u8; 32]),
            snapshot_id: None,
        },
        MemoryEvidenceOrigin::Imported {
            importer: "dreams-transition".to_owned(),
            import_id: "import-1".to_owned(),
        },
        MemoryEvidenceOrigin::Consolidated { from: vec![mem_id] },
    ];
    for v in &variants {
        let json = serde_json::to_value(v).expect("serialize");
        let back: MemoryEvidenceOrigin = serde_json::from_value(json).expect("deserialize");
        assert_eq!(v, &back);
    }
}

// ── MemoryRecord / MemoryRecordDraft ──

#[test]
fn memory_record_roundtrip() {
    let record = MemoryRecord {
        id: new_memory_id(),
        tenant_id: new_tenant_id(),
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::User {
            user_id: "u1".to_owned(),
        },
        content: "test content".to_owned(),
        metadata: make_meta(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        expires_at: None,
        deleted_at: None,
    };
    let json = serde_json::to_value(&record).expect("serialize");
    let back: MemoryRecord = serde_json::from_value(json).expect("deserialize");
    assert_eq!(record.id, back.id);
    assert_eq!(record.content, back.content);
}

#[test]
fn memory_record_draft_roundtrip() {
    let draft = MemoryRecordDraft {
        kind: MemoryKind::Feedback,
        visibility: MemoryVisibility::Private {
            session_id: new_session_id(),
        },
        content: "draft content".to_owned(),
        metadata: make_meta(),
        expires_at: None,
    };
    let json = serde_json::to_value(&draft).expect("serialize");
    let back: MemoryRecordDraft = serde_json::from_value(json).expect("deserialize");
    assert_eq!(draft.content, back.content);
}

// ── MemoryEvidence ──

#[test]
fn memory_evidence_roundtrip() {
    let evidence = MemoryEvidence {
        source: MemorySource::UserInput,
        origin: MemoryEvidenceOrigin::UserMessage {
            session_id: new_session_id(),
            run_id: new_run_id(),
            message_id: new_message_id(),
        },
        content_hash: ContentHash([3u8; 32]),
        session_id: Some(new_session_id()),
        run_id: Some(new_run_id()),
        message_id: Some(new_message_id()),
        tool_use_id: None,
    };
    let json = serde_json::to_value(&evidence).expect("serialize");
    let back: MemoryEvidence = serde_json::from_value(json).expect("deserialize");
    assert_eq!(evidence.content_hash, back.content_hash);
}

// ── MemoryCandidate ──

#[test]
fn memory_candidate_roundtrip() {
    let candidate = MemoryCandidate {
        id: MemoryCandidateId::new(),
        tenant_id: new_tenant_id(),
        state: MemoryCandidateState::Proposed,
        operation: MemoryCandidateOperation::Update {
            memory_id: new_memory_id(),
        },
        proposed_record: MemoryRecordDraft {
            kind: MemoryKind::Reference,
            visibility: MemoryVisibility::Tenant,
            content: "candidate content".to_owned(),
            metadata: make_meta(),
            expires_at: None,
        },
        evidence: MemoryEvidence {
            source: MemorySource::AgentDerived,
            origin: MemoryEvidenceOrigin::AssistantMessage {
                session_id: new_session_id(),
                run_id: new_run_id(),
                message_id: new_message_id(),
            },
            content_hash: ContentHash([4u8; 32]),
            session_id: None,
            run_id: None,
            message_id: None,
            tool_use_id: None,
        },
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        expires_at: None,
    };
    let json = serde_json::to_value(&candidate).expect("serialize");
    let back: MemoryCandidate = serde_json::from_value(json).expect("deserialize");
    assert_eq!(candidate.state, back.state);
    assert_eq!(candidate.operation, back.operation);
}

// ── MemoryCandidateState ──

#[test]
fn memory_candidate_state_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryCandidateState::Proposed).unwrap(),
        json!("proposed")
    );
    assert_eq!(
        serde_json::to_value(MemoryCandidateState::Approved).unwrap(),
        json!("approved")
    );
    assert_eq!(
        serde_json::to_value(MemoryCandidateState::Rejected).unwrap(),
        json!("rejected")
    );
    assert_eq!(
        serde_json::to_value(MemoryCandidateState::Promoted).unwrap(),
        json!("promoted")
    );
    assert_eq!(
        serde_json::to_value(MemoryCandidateState::Merged).unwrap(),
        json!("merged")
    );
    assert_eq!(
        serde_json::to_value(MemoryCandidateState::Expired).unwrap(),
        json!("expired")
    );
}

// ── MemoryThreadMode ──

#[test]
fn memory_thread_mode_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryThreadMode::Off).unwrap(),
        json!("off")
    );
    assert_eq!(
        serde_json::to_value(MemoryThreadMode::ReadOnly).unwrap(),
        json!("read_only")
    );
    assert_eq!(
        serde_json::to_value(MemoryThreadMode::ReadWrite).unwrap(),
        json!("read_write")
    );
    assert_eq!(
        serde_json::to_value(MemoryThreadMode::CandidateOnly).unwrap(),
        json!("candidate_only")
    );
}

// ── MemoryDropReason ──

#[test]
fn memory_drop_reason_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryDropReason::Expired).unwrap(),
        json!("expired")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::Deleted).unwrap(),
        json!("deleted")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::VisibilityDenied).unwrap(),
        json!("visibility_denied")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::PolicyDenied).unwrap(),
        json!("policy_denied")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::ThreatBlocked).unwrap(),
        json!("threat_blocked")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::BudgetExceeded).unwrap(),
        json!("budget_exceeded")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::Duplicate).unwrap(),
        json!("duplicate")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::ProviderTimeout).unwrap(),
        json!("provider_timeout")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::ProviderError).unwrap(),
        json!("provider_error")
    );
    assert_eq!(
        serde_json::to_value(MemoryDropReason::ScoreBelowThreshold).unwrap(),
        json!("score_below_threshold")
    );
}

// ── MemoryPolicyDecision ──

#[test]
fn memory_policy_decision_roundtrip() {
    let allow = MemoryPolicyDecision::Allow;
    let deny = MemoryPolicyDecision::Deny {
        reason: MemoryPolicyDenyReason::GlobalUseDisabled,
    };
    let cand = MemoryPolicyDecision::CandidateOnly {
        reason: MemoryPolicyDenyReason::ExternalContextGenerationDisabled,
    };

    for v in [allow, deny, cand] {
        let json = serde_json::to_value(&v).expect("serialize");
        let back: MemoryPolicyDecision = serde_json::from_value(json).expect("deserialize");
        assert_eq!(v, back);
    }
}

// ── MemoryPolicyDenyReason ──

#[test]
fn memory_policy_deny_reason_serde_tags() {
    let reasons = [
        (
            "global_use_disabled",
            MemoryPolicyDenyReason::GlobalUseDisabled,
        ),
        (
            "thread_use_disabled",
            MemoryPolicyDenyReason::ThreadUseDisabled,
        ),
        (
            "global_generation_disabled",
            MemoryPolicyDenyReason::GlobalGenerationDisabled,
        ),
        (
            "thread_generation_disabled",
            MemoryPolicyDenyReason::ThreadGenerationDisabled,
        ),
        (
            "external_context_generation_disabled",
            MemoryPolicyDenyReason::ExternalContextGenerationDisabled,
        ),
        ("missing_policy", MemoryPolicyDenyReason::MissingPolicy),
        (
            "visibility_escalation_denied",
            MemoryPolicyDenyReason::VisibilityEscalationDenied,
        ),
        (
            "provider_not_writable",
            MemoryPolicyDenyReason::ProviderNotWritable,
        ),
        ("tenant_mismatch", MemoryPolicyDenyReason::TenantMismatch),
        (
            "tombstone_matched",
            MemoryPolicyDenyReason::TombstoneMatched,
        ),
        (
            "permission_required",
            MemoryPolicyDenyReason::PermissionRequired,
        ),
        ("threat_blocked", MemoryPolicyDenyReason::ThreatBlocked),
    ];
    for (expected_tag, reason) in &reasons {
        assert_eq!(serde_json::to_value(reason).unwrap(), json!(expected_tag));
    }
}

// ── MemoryGlobalSettings / MemoryThreadSettings ──

#[test]
fn memory_global_settings_roundtrip() {
    let settings = MemoryGlobalSettings {
        use_memories: true,
        generate_memories: false,
        disable_generation_when_external_context_used: true,
        retention_days: Some(90),
        max_memory_bytes: 10_000_000,
        max_recall_records_per_turn: 20,
        max_recall_chars_per_turn: 50_000,
    };
    let json = serde_json::to_value(&settings).expect("serialize");
    let back: MemoryGlobalSettings = serde_json::from_value(json).expect("deserialize");
    assert_eq!(
        settings.max_recall_records_per_turn,
        back.max_recall_records_per_turn
    );
}

#[test]
fn memory_thread_settings_roundtrip() {
    let settings = MemoryThreadSettings {
        session_id: new_session_id(),
        use_memories: Some(false),
        generate_memories: None,
        memory_mode: MemoryThreadMode::ReadOnly,
    };
    let json = serde_json::to_value(&settings).expect("serialize");
    let back: MemoryThreadSettings = serde_json::from_value(json).expect("deserialize");
    assert_eq!(settings.memory_mode, back.memory_mode);
}

// ── MemoryToolArgs / MemoryToolRequest / MemoryToolRuntimeContext ──

#[test]
fn memory_tool_args_roundtrip() {
    let args = MemoryToolArgs {
        action: MemoryToolAction::Search(MemorySearchRequest {
            query: "test query".to_owned(),
            max_records: 10,
            visibility: Some(MemoryToolVisibility::User),
            cursor: None,
        }),
    };
    let json = serde_json::to_value(&args).expect("serialize");
    assert_eq!(json["action"], "search");
    assert_eq!(json["query"], "test query");
    let back: MemoryToolArgs = serde_json::from_value(json).expect("deserialize");
    match back.action {
        MemoryToolAction::Search(req) => assert_eq!(req.query, "test query"),
        _ => panic!("expected Search"),
    }
}

#[test]
fn memory_tool_request_fields() {
    let request = MemoryToolRequest {
        args: MemoryToolArgs {
            action: MemoryToolAction::List(MemoryListRequest {
                visibility: None,
                include_expired: false,
                include_deleted: false,
                limit: 10,
                cursor: None,
            }),
        },
        runtime: MemoryToolRuntimeContext {
            actor: MemoryActor::User { user_label: None },
            permission_context: MemoryPermissionContext {
                explicit_user_instruction: false,
                include_raw_content: false,
                action_plan_id: None,
                authorization_ticket_id: None,
                non_interactive_policy_grant: false,
            },
            tenant_id: new_tenant_id(),
            session_id: new_session_id(),
            run_id: new_run_id(),
            provider_policy: MemoryProviderSelectionPolicy::PolicySelected,
        },
    };
    let json = serde_json::to_value(&request).expect("serialize");
    let back: MemoryToolRequest = serde_json::from_value(json).expect("deserialize");
    assert!(matches!(back.args.action, MemoryToolAction::List(..)));
}

// ── All tool actions ──

#[test]
fn memory_tool_all_actions_roundtrip() {
    let mem_id = new_memory_id();
    let draft = MemoryToolDraft {
        kind: MemoryKind::UserPreference,
        visibility: MemoryToolVisibility::User,
        content: "test".to_owned(),
        metadata: make_meta(),
    };

    let actions: Vec<MemoryToolAction> = vec![
        MemoryToolAction::Search(MemorySearchRequest {
            query: "q".to_owned(),
            max_records: 5,
            visibility: None,
            cursor: None,
        }),
        MemoryToolAction::Read(MemoryReadRequest { memory_id: mem_id }),
        MemoryToolAction::Create(MemoryToolCreateArgs {
            draft: draft.clone(),
        }),
        MemoryToolAction::Update(MemoryToolUpdateArgs {
            memory_id: mem_id,
            draft: draft.clone(),
        }),
        MemoryToolAction::Delete(MemoryDeleteRequest {
            memory_id: mem_id,
            reason: "cleanup".to_owned(),
        }),
        MemoryToolAction::List(MemoryListRequest {
            visibility: None,
            include_expired: false,
            include_deleted: false,
            limit: 10,
            cursor: None,
        }),
        MemoryToolAction::Propose(MemoryToolProposeArgs {
            draft: draft.clone(),
        }),
    ];
    for action in &actions {
        let json = serde_json::to_value(action).expect("serialize action");
        let back: MemoryToolAction = serde_json::from_value(json).expect("deserialize action");
        let _ = back; // prove deser worked
    }
}

// ── MemoryToolResponse ──

#[test]
fn memory_tool_response_roundtrip() {
    let response = MemoryToolResponse {
        action: "search".to_owned(),
        state: MemoryToolState::Completed,
        memory_ids: vec![new_memory_id()],
        candidate_ids: vec![],
        records: vec![],
        next_cursor: None,
        action_plan_id: Some(ActionPlanId::new()),
        denial: None,
        redaction: MemoryRedactionSummary {
            redacted_count: 0,
            dropped_count: 0,
        },
        trace_id: None,
        takes_effect: MemoryTakesEffect::CurrentTurn,
    };
    let json = serde_json::to_value(&response).expect("serialize");
    let back: MemoryToolResponse = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.state, MemoryToolState::Completed);
}

// ── MemoryScoreBreakdown ──

#[test]
fn memory_score_breakdown_roundtrip() {
    let score = MemoryScoreBreakdown {
        lexical_score: 0.85,
        vector_score: Some(0.72),
        confidence_score: 0.9,
        recency_score: 0.5,
        access_score: 0.3,
        source_trust_score: 0.8,
        explicit_selection_boost: 0.0,
        final_score: 0.78,
    };
    let json = serde_json::to_value(&score).expect("serialize");
    let back: MemoryScoreBreakdown = serde_json::from_value(json).expect("deserialize");
    assert!((back.final_score - 0.78).abs() < 0.001);
}

// ── MemoryRecallTrace ──

#[test]
fn memory_recall_trace_no_raw_content() {
    let trace = MemoryRecallTrace {
        trace_id: MemoryTraceId::new(),
        tenant_id: new_tenant_id(),
        session_id: new_session_id(),
        run_id: new_run_id(),
        turn: 1,
        query_text_hash: ContentHash([5u8; 32]),
        provider_results: vec![MemoryProviderTrace {
            provider_id: "local".to_owned(),
            trust_level: MemoryProviderTrust::BuiltIn,
            readable: true,
            writable: true,
            requested_count: 10,
            returned_count: 8,
            timed_out: false,
            error_kind: None,
            latency_ms: 42,
        }],
        candidates: vec![MemoryCandidateTrace {
            memory_id: new_memory_id(),
            provider_id: "local".to_owned(),
            content_hash: ContentHash([6u8; 32]),
            score: MemoryScoreBreakdown {
                lexical_score: 1.0,
                vector_score: None,
                confidence_score: 1.0,
                recency_score: 1.0,
                access_score: 0.0,
                source_trust_score: 1.0,
                explicit_selection_boost: 0.0,
                final_score: 0.9,
            },
            policy_decision: MemoryPolicyDecision::Allow,
        }],
        injected: vec![MemoryInjectedTrace {
            memory_id: new_memory_id(),
            provider_id: "local".to_owned(),
            content_hash: ContentHash([7u8; 32]),
            injected_chars: 100,
            fence_id: "memory_recall_turn_1".to_owned(),
        }],
        dropped: vec![MemoryDroppedTrace {
            memory_id: Some(new_memory_id()),
            provider_id: Some("local".to_owned()),
            content_hash: Some(ContentHash([8u8; 32])),
            reason: MemoryDropReason::ScoreBelowThreshold,
        }],
        redacted_count: 0,
        injected_chars: 100,
        deadline_used_ms: 500,
        at: chrono::Utc::now(),
    };
    let json = serde_json::to_value(&trace).expect("serialize");
    let back: MemoryRecallTrace = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.turn, 1);
    assert_eq!(back.injected.len(), 1);

    // Verify no raw content fields leaked via field names in the serialized JSON
    let json_str = serde_json::to_string(&trace).unwrap();
    assert!(!json_str.contains("\"content\""));
    assert!(!json_str.contains("\"raw_content\""));
    assert!(!json_str.contains("\"prompt\""));
    assert!(!json_str.contains("\"message_text\""));
}

// ── MemoryProviderDescriptor ──

#[test]
fn memory_provider_descriptor_roundtrip() {
    let desc = MemoryProviderDescriptor {
        provider_id: "local".to_owned(),
        provider_kind: MemoryProviderKind::Local,
        priority: 100,
        trust_level: MemoryProviderTrust::BuiltIn,
        tenant_scope: Some(TenantId::SINGLE),
        workspace_scope: None,
        durability: MemoryProviderDurability::Durable,
        readable: true,
        writable: true,
        allowed_visibility: vec![MemoryVisibilityClass::Private, MemoryVisibilityClass::User],
        supports_evidence: true,
        supports_raw_content_export: false,
        timeout_ms: 5000,
        max_records_per_recall: 50,
        max_chars_per_recall: 100_000,
        max_bytes_per_record: 1024 * 1024,
    };
    let json = serde_json::to_value(&desc).expect("serialize");
    let back: MemoryProviderDescriptor = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.provider_id, "local");
}

// ── MemoryProviderTrust ──

#[test]
fn memory_provider_trust_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryProviderTrust::BuiltIn).unwrap(),
        json!("built_in")
    );
    assert_eq!(
        serde_json::to_value(MemoryProviderTrust::Workspace).unwrap(),
        json!("workspace")
    );
    assert_eq!(
        serde_json::to_value(MemoryProviderTrust::Team).unwrap(),
        json!("team")
    );
    assert_eq!(
        serde_json::to_value(MemoryProviderTrust::Plugin).unwrap(),
        json!("plugin")
    );
    assert_eq!(
        serde_json::to_value(MemoryProviderTrust::External).unwrap(),
        json!("external")
    );
}

// ── MemoryVisibilityClass ──

#[test]
fn memory_visibility_class_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryVisibilityClass::Private).unwrap(),
        json!("private")
    );
    assert_eq!(
        serde_json::to_value(MemoryVisibilityClass::User).unwrap(),
        json!("user")
    );
    assert_eq!(
        serde_json::to_value(MemoryVisibilityClass::Team).unwrap(),
        json!("team")
    );
    assert_eq!(
        serde_json::to_value(MemoryVisibilityClass::Tenant).unwrap(),
        json!("tenant")
    );
}

// ── MemoryTakesEffect ──

#[test]
fn memory_takes_effect_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryTakesEffect::CurrentTurn).unwrap(),
        json!("current_turn")
    );
    assert_eq!(
        serde_json::to_value(MemoryTakesEffect::NextTurn).unwrap(),
        json!("next_turn")
    );
    assert_eq!(
        serde_json::to_value(MemoryTakesEffect::NextSession).unwrap(),
        json!("next_session")
    );
    assert_eq!(
        serde_json::to_value(MemoryTakesEffect::Never).unwrap(),
        json!("never")
    );
}

// ── MemoryToolState ──

#[test]
fn memory_tool_state_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryToolState::Completed).unwrap(),
        json!("completed")
    );
    assert_eq!(
        serde_json::to_value(MemoryToolState::CandidateCreated).unwrap(),
        json!("candidate_created")
    );
}

// ── MemoryActorContext (new enum definition) ──

#[test]
fn memory_actor_serde_tags() {
    assert_eq!(
        serde_json::to_value(MemoryActor::User { user_label: None }).unwrap(),
        json!({"user": {"user_label": null}})
    );
    assert_eq!(
        serde_json::to_value(MemoryActor::Model).unwrap(),
        json!("model")
    );
    assert_eq!(
        serde_json::to_value(MemoryActor::System).unwrap(),
        json!("system")
    );
}

// ── MemoryProviderSelectionPolicy ──

#[test]
fn memory_provider_selection_policy_roundtrip() {
    assert_eq!(
        serde_json::to_value(MemoryProviderSelectionPolicy::PolicySelected).unwrap(),
        json!("policy_selected")
    );
    assert_eq!(
        serde_json::to_value(MemoryProviderSelectionPolicy::RequireProvider {
            provider_id: "local".to_owned()
        })
        .unwrap(),
        json!({"require_provider": {"provider_id": "local"}})
    );
    assert_eq!(
        serde_json::to_value(MemoryProviderSelectionPolicy::DenyModelSelectedProvider).unwrap(),
        json!("deny_model_selected_provider")
    );
}

// ── IPC contracts ──

#[test]
fn ipc_memory_settings_contracts_roundtrip() {
    // GetMemorySettings
    let req = GetMemorySettingsRequest {
        tenant_id: new_tenant_id(),
    };
    let json = serde_json::to_value(&req).unwrap();
    let _: GetMemorySettingsRequest = serde_json::from_value(json).unwrap();

    let resp = GetMemorySettingsResponse {
        settings: MemoryGlobalSettings {
            use_memories: true,
            generate_memories: true,
            disable_generation_when_external_context_used: false,
            retention_days: None,
            max_memory_bytes: 1024 * 1024,
            max_recall_records_per_turn: 10,
            max_recall_chars_per_turn: 10000,
        },
    };
    let json = serde_json::to_value(&resp).unwrap();
    let _: GetMemorySettingsResponse = serde_json::from_value(json).unwrap();
}

#[test]
fn ipc_thread_memory_settings_contracts_roundtrip() {
    let sid = new_session_id();
    let req = UpdateThreadMemorySettingsRequest {
        tenant_id: new_tenant_id(),
        settings: MemoryThreadSettings {
            session_id: sid,
            use_memories: Some(true),
            generate_memories: None,
            memory_mode: MemoryThreadMode::ReadWrite,
        },
    };
    let json = serde_json::to_value(&req).unwrap();
    let _: UpdateThreadMemorySettingsRequest = serde_json::from_value(json).unwrap();
}

#[test]
fn ipc_memory_candidates_contracts_roundtrip() {
    let req = ListMemoryCandidatesRequest {
        tenant_id: new_tenant_id(),
        session_id: None,
        state: Some(MemoryCandidateState::Proposed),
        limit: 20,
        cursor: None,
    };
    let json = serde_json::to_value(&req).unwrap();
    let _: ListMemoryCandidatesRequest = serde_json::from_value(json).unwrap();

    let item = MemoryCandidateListItem {
        id: MemoryCandidateId::new(),
        state: MemoryCandidateState::Proposed,
        operation: MemoryCandidateOperation::Create,
        proposed_record: MemoryRecordDraft {
            kind: MemoryKind::ProjectFact,
            visibility: MemoryVisibility::Tenant,
            content: "test".to_owned(),
            metadata: make_meta(),
            expires_at: None,
        },
        evidence: MemoryEvidence {
            source: MemorySource::AgentDerived,
            origin: MemoryEvidenceOrigin::AssistantMessage {
                session_id: new_session_id(),
                run_id: new_run_id(),
                message_id: new_message_id(),
            },
            content_hash: ContentHash([9u8; 32]),
            session_id: None,
            run_id: None,
            message_id: None,
            tool_use_id: None,
        },
        created_at: chrono::Utc::now(),
        expires_at: None,
    };
    let resp = ListMemoryCandidatesResponse {
        candidates: vec![item],
        next_cursor: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    let _: ListMemoryCandidatesResponse = serde_json::from_value(json).unwrap();
}

#[test]
fn ipc_approve_reject_merge_contracts_roundtrip() {
    let approve = ApproveMemoryCandidateRequest {
        tenant_id: new_tenant_id(),
        candidate_id: MemoryCandidateId::new(),
        action_plan_id: Some(ActionPlanId::new()),
    };
    let _: ApproveMemoryCandidateRequest =
        serde_json::from_value(serde_json::to_value(&approve).unwrap()).unwrap();
    let approve_without_action_plan = ApproveMemoryCandidateRequest {
        action_plan_id: None,
        ..approve
    };
    let approve_json = serde_json::to_value(&approve_without_action_plan).unwrap();
    assert!(approve_json.get("action_plan_id").is_none());
    let _: ApproveMemoryCandidateRequest = serde_json::from_value(approve_json).unwrap();

    let reject = RejectMemoryCandidateRequest {
        tenant_id: new_tenant_id(),
        candidate_id: MemoryCandidateId::new(),
        reason: "outdated".to_owned(),
    };
    let _: RejectMemoryCandidateRequest =
        serde_json::from_value(serde_json::to_value(&reject).unwrap()).unwrap();

    let merge = MergeMemoryCandidateRequest {
        tenant_id: new_tenant_id(),
        candidate_ids: vec![MemoryCandidateId::new(), MemoryCandidateId::new()],
        merged_record: MemoryRecordDraft {
            kind: MemoryKind::ProjectFact,
            visibility: MemoryVisibility::Tenant,
            content: "merged".to_owned(),
            metadata: make_meta(),
            expires_at: None,
        },
        action_plan_id: None,
    };
    let merge_json = serde_json::to_value(&merge).unwrap();
    assert!(merge_json.get("action_plan_id").is_none());
    assert!(merge_json.get("evidence").is_none());
    let _: MergeMemoryCandidateRequest = serde_json::from_value(merge_json).unwrap();
}

#[test]
fn ipc_recall_traces_contracts_roundtrip() {
    let tenant_id = new_tenant_id();
    let req = ListMemoryRecallTracesRequest {
        tenant_id,
        session_id: None,
        run_id: None,
        limit: 10,
        cursor: None,
    };
    let _: ListMemoryRecallTracesRequest =
        serde_json::from_value(serde_json::to_value(&req).unwrap()).unwrap();

    let summary = MemoryRecallTraceSummary {
        trace_id: MemoryTraceId::new(),
        tenant_id,
        session_id: new_session_id(),
        run_id: new_run_id(),
        injected_count: 3,
        dropped_count: 1,
        redacted_count: 0,
        at: chrono::Utc::now(),
    };
    let resp = ListMemoryRecallTracesResponse {
        traces: vec![summary],
        next_cursor: None,
    };
    let _: ListMemoryRecallTracesResponse =
        serde_json::from_value(serde_json::to_value(&resp).unwrap()).unwrap();

    let get_resp = GetMemoryRecallTraceResponse {
        trace: MemoryRecallTrace {
            trace_id: MemoryTraceId::new(),
            tenant_id,
            session_id: new_session_id(),
            run_id: new_run_id(),
            turn: 1,
            query_text_hash: ContentHash([11u8; 32]),
            provider_results: vec![],
            candidates: vec![],
            injected: vec![],
            dropped: vec![],
            redacted_count: 0,
            injected_chars: 0,
            deadline_used_ms: 100,
            at: chrono::Utc::now(),
        },
    };
    let _: GetMemoryRecallTraceResponse =
        serde_json::from_value(serde_json::to_value(&get_resp).unwrap()).unwrap();
}

#[test]
fn ipc_model_request_preview_contracts_roundtrip() {
    let req = GetModelRequestPreviewRequest {
        tenant_id: new_tenant_id(),
        session_id: new_session_id(),
        run_id: new_run_id(),
        trace_id: None,
    };
    let _: GetModelRequestPreviewRequest =
        serde_json::from_value(serde_json::to_value(&req).unwrap()).unwrap();

    let preview = MemoryModelRequestPreview {
        session_id: new_session_id(),
        run_id: new_run_id(),
        trace_id: Some(MemoryTraceId::new()),
        sections: vec![MemoryModelRequestPreviewSection {
            source: MemorySource::UserInput,
            provider_id: Some("local".to_owned()),
            memory_ids: vec![new_memory_id()],
            redacted_content: "redacted preview".to_owned(),
        }],
        redacted_count: 1,
        token_estimate: 4,
        tool_names: vec!["memory".to_owned()],
        policy_decisions: vec!["Allow".to_owned()],
        content_hash: ContentHash([12u8; 32]),
    };
    let resp = GetModelRequestPreviewResponse { preview };
    let _: GetModelRequestPreviewResponse =
        serde_json::from_value(serde_json::to_value(&resp).unwrap()).unwrap();
}

// ── MemoryMetadata ──

#[test]
fn memory_metadata_defaults_and_fields() {
    let meta = make_meta();
    assert!(meta.ttl.is_none());
    assert!(meta.tags.is_empty());

    let meta = MemoryMetadata {
        ttl: Some(std::time::Duration::from_secs(3600)),
        tags: vec!["important".to_owned()],
        source_trust: 0.8,
        ..make_meta()
    };
    let json = serde_json::to_value(&meta).unwrap();
    let back: MemoryMetadata = serde_json::from_value(json).unwrap();
    assert_eq!(back.tags, vec!["important"]);
}

// ── Memory tool denial ──

#[test]
fn memory_tool_denial_fields() {
    let denial = MemoryToolDenial {
        reason: MemoryPolicyDenyReason::PermissionRequired,
        safe_message: "Permission required to delete memory".to_owned(),
        action_plan_id: Some(ActionPlanId::new()),
    };
    let json = serde_json::to_value(&denial).unwrap();
    let back: MemoryToolDenial = serde_json::from_value(json).unwrap();
    assert_eq!(back.reason, MemoryPolicyDenyReason::PermissionRequired);
}

// ── MemoryToolRecordView ──

#[test]
fn memory_tool_record_view_fields() {
    let view = MemoryToolRecordView {
        memory_id: new_memory_id(),
        provider_id: "local".to_owned(),
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        redacted_content: Some("safe content".to_owned()),
        content_hash: ContentHash([13u8; 32]),
        score: None,
    };
    let json = serde_json::to_value(&view).unwrap();
    let back: MemoryToolRecordView = serde_json::from_value(json).unwrap();
    assert_eq!(back.redacted_content, Some("safe content".to_owned()));
}

// ── MemoryPermissionContext ──

#[test]
fn memory_permission_context_roundtrip() {
    let ctx = MemoryPermissionContext {
        explicit_user_instruction: true,
        include_raw_content: true,
        action_plan_id: Some(ActionPlanId::new()),
        authorization_ticket_id: Some(AuthorizationTicketId::new()),
        non_interactive_policy_grant: false,
    };
    let json = serde_json::to_value(&ctx).unwrap();
    let back: MemoryPermissionContext = serde_json::from_value(json).unwrap();
    assert!(back.explicit_user_instruction);
    assert!(back.include_raw_content);
}

// ── New ID types ──

#[test]
fn memory_trace_id_and_candidate_id_are_typed_ulids() {
    let trace_id = MemoryTraceId::new();
    let candidate_id = MemoryCandidateId::new();

    // Roundtrip via string
    let trace_str = trace_id.to_string();
    let parsed: MemoryTraceId = trace_str.parse().unwrap();
    assert_eq!(trace_id, parsed);

    let cand_str = candidate_id.to_string();
    let parsed: MemoryCandidateId = cand_str.parse().unwrap();
    assert_eq!(candidate_id, parsed);

    // Type safety: different scopes are not interchangeable at type level
    let mem_id = MemoryId::new();
    // These would fail to compile if uncommented:
    // let _: MemoryTraceId = mem_id;
    // let _: MemoryId = trace_id;
    let _ = mem_id;
}

// ── Type aliases ──

#[test]
fn memory_type_aliases_exist() {
    let pid: MemoryProviderId = "local".to_owned();
    let _: MemoryOriginName = "memory".to_owned();
    let _: MemoryOriginLabel = "Memory Tool".to_owned();
    let _: MemoryPageCursor = "cursor-abc".to_owned();
    let _ = pid;
}

// ── DREAMs transition guard ──

#[test]
fn consolidation_event_has_no_dreams_field() {
    // MemoryConsolidationRanEvent must not contain draft_dreams_chars
    let event = MemoryConsolidationRanEvent {
        session_id: new_session_id(),
        hook_id: "consolidate".to_owned(),
        promoted: vec![],
        demoted: vec![],
        inbox_candidates_created: 2,
        duration_ms: 100,
        at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("draft_dreams_chars"));
    assert!(!json.contains("dreams"));
    assert!(json.contains("inbox_candidates_created"));
}
