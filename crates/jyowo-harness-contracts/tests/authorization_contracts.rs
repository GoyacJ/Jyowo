use harness_contracts::*;
use serde_json::json;

fn sandbox_summary() -> SandboxPolicySummary {
    SandboxPolicySummary {
        mode: SandboxMode::OsLevel(LocalIsolationTag::None),
        scope: SandboxScope::WorkspaceOnly,
        network: NetworkAccess::None,
        resource_limits: ResourceLimits {
            max_memory_bytes: Some(268_435_456),
            max_cpu_cores: Some(1.0),
            max_pids: Some(64),
            max_wall_clock_ms: Some(30_000),
            max_open_files: Some(128),
        },
    }
}

fn permission_review() -> PermissionReview {
    PermissionReview {
        summary: "Write workspace file".to_owned(),
        details: vec![PermissionReviewDetail {
            label: "Target".to_owned(),
            value: "workspace://src/lib.rs".to_owned(),
            redacted: false,
        }],
        confirmation: PermissionConfirmation::TypeToConfirm {
            expected: "OVERWRITE".to_owned(),
        },
        redacted: false,
    }
}

#[test]
fn tool_action_plan_serializes_authorization_contract_shape() {
    let plan = ToolActionPlan {
        plan_id: ActionPlanId::from_u128(1),
        tool_use_id: ToolUseId::from_u128(2),
        tool_name: "write_file".to_owned(),
        actor_source: PermissionActorSource::Automation {
            automation_id: "daily-maintenance".to_owned(),
            conversation_id: SessionId::from_u128(3),
            run_id: Some(RunId::from_u128(4)),
        },
        subject: PermissionSubject::FileWrite {
            path: "/workspace/src/lib.rs".into(),
            bytes_preview: b"fn main() {}".to_vec(),
        },
        scope: DecisionScope::PathPrefix("/workspace/src".into()),
        severity: Severity::High,
        resources: vec![
            ActionResource::FileWrite {
                path: "/workspace/src/lib.rs".into(),
                content_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
            },
            ActionResource::Sandbox {
                backend_id: "local".to_owned(),
                policy_hash: SandboxPolicyHash::from_hex(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
                .expect("valid sandbox policy hash"),
            },
        ],
        sandbox_policy: SandboxPolicy {
            mode: SandboxMode::OsLevel(LocalIsolationTag::None),
            scope: SandboxScope::WorkspaceOnly,
            network: NetworkAccess::None,
            resource_limits: sandbox_summary().resource_limits,
            denied_host_paths: vec!["/Users".into()],
        },
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: vec!["/workspace/src".into()],
        },
        network_access: NetworkAccess::None,
        review: permission_review(),
        plan_hash: ActionPlanHash::from_hex(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .expect("valid action plan hash"),
        created_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    };

    let value = serde_json::to_value(&plan).expect("tool action plan serializes");

    assert_eq!(value["plan_id"], ActionPlanId::from_u128(1).to_string());
    assert_eq!(value["actor_source"]["type"], "automation");
    assert_eq!(value["resources"][0]["type"], "file_write");
    assert_eq!(
        value["resources"][1]["policy_hash"],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(value["review"]["confirmation"]["type"], "type_to_confirm");
    assert_eq!(value["review"]["confirmation"]["expected"], "OVERWRITE");

    let roundtrip: ToolActionPlan =
        serde_json::from_value(value).expect("tool action plan deserializes");
    assert_eq!(roundtrip, plan);
}

#[test]
fn permission_actor_source_adds_automation_and_mcp_server_variants() {
    let automation = PermissionActorSource::Automation {
        automation_id: "nightly".to_owned(),
        conversation_id: SessionId::from_u128(1),
        run_id: Some(RunId::from_u128(2)),
    };
    let automation_value = serde_json::to_value(&automation).expect("automation serializes");
    assert_eq!(automation_value["type"], "automation");
    assert_eq!(automation_value["automation_id"], "nightly");
    assert_eq!(
        serde_json::from_value::<PermissionActorSource>(automation_value).unwrap(),
        automation
    );

    let mcp_server = PermissionActorSource::McpServer {
        server_id: McpServerId("browser".to_owned()),
        origin: ManifestOriginRef::RemoteRegistry {
            endpoint: "https://registry.example/redacted".to_owned(),
        },
        scope: McpServerScope::Session(SessionId::from_u128(3)),
    };
    let mcp_value = serde_json::to_value(&mcp_server).expect("mcp server serializes");
    assert_eq!(mcp_value["type"], "mcp_server");
    assert_eq!(mcp_value["server_id"], "browser");
    assert_eq!(
        mcp_value["scope"]["session"],
        SessionId::from_u128(3).to_string()
    );
    assert_eq!(
        serde_json::from_value::<PermissionActorSource>(mcp_value).unwrap(),
        mcp_server
    );
}

#[test]
fn permission_events_include_review_mode_hashes_and_sandbox_summary_without_ticket() {
    let request = PermissionRequestedEvent {
        request_id: RequestId::from_u128(1),
        run_id: RunId::from_u128(2),
        session_id: SessionId::from_u128(3),
        tenant_id: TenantId::SINGLE,
        tool_use_id: ToolUseId::from_u128(4),
        tool_name: "write_file".to_owned(),
        subject: PermissionSubject::ToolInvocation {
            tool: "write_file".to_owned(),
            input: json!({ "path": "src/lib.rs" }),
        },
        severity: Severity::Medium,
        scope_hint: DecisionScope::PathPrefix("/workspace/src".into()),
        fingerprint: None,
        presented_options: vec![PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::AllowOnce,
            scope: DecisionScope::PathPrefix("/workspace/src".into()),
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: DecisionMatcherKind::PathPrefix,
                label: "/workspace/src".to_owned(),
            },
            label: "Allow write once".to_owned(),
            requires_confirmation: false,
            action_plan_hash: ActionPlanHash::default(),
            fingerprint: None,
        }],
        interactivity: InteractivityLevel::FullyInteractive,
        auto_resolved: false,
        actor_source: PermissionActorSource::ParentRun,
        action_plan_hash: ActionPlanHash::from_hex(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap(),
        review: permission_review(),
        effective_mode: PermissionMode::Default,
        sandbox_policy: sandbox_summary(),
        causation_id: EventId::from_u128(5),
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    };
    let value = serde_json::to_value(&Event::PermissionRequested(request))
        .expect("permission requested event serializes");
    assert_eq!(value["type"], "permission_requested");
    assert_eq!(
        value["action_plan_hash"],
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    );
    assert_eq!(value["effective_mode"], "default");
    assert!(value.get("authorization_ticket_id").is_none());
    assert!(value.get("ticket_id").is_none());

    let resolved = Event::PermissionResolved(PermissionResolvedEvent {
        request_id: RequestId::from_u128(1),
        decision: Decision::AllowOnce,
        decided_by: DecidedBy::User,
        scope: DecisionScope::PathPrefix("/workspace/src".into()),
        fingerprint: None,
        rationale: None,
        action_plan_hash: ActionPlanHash::from_hex(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap(),
        decision_id: DecisionId::from_u128(6),
        auto_resolved: false,
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    });
    let resolved_value = serde_json::to_value(resolved).unwrap();
    assert_eq!(
        resolved_value["action_plan_hash"],
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    );
    assert_eq!(
        resolved_value["decision_id"],
        DecisionId::from_u128(6).to_string()
    );
    assert_eq!(resolved_value["auto_resolved"], false);
}

#[test]
fn sandbox_preflight_events_are_public_contracts() {
    let passed = Event::SandboxPreflightPassed(SandboxPreflightPassedEvent {
        session_id: SessionId::from_u128(1),
        run_id: RunId::from_u128(2),
        tool_use_id: Some(ToolUseId::from_u128(3)),
        backend_id: "local".to_owned(),
        status: SandboxPreflightStatus::Passed,
        policy: sandbox_summary(),
        policy_hash: SandboxPolicyHash::from_hex(
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        )
        .unwrap(),
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    });
    assert_eq!(
        serde_json::to_value(passed).unwrap()["type"],
        "sandbox_preflight_passed"
    );

    let failed = Event::SandboxPreflightFailed(SandboxPreflightFailedEvent {
        session_id: SessionId::from_u128(1),
        run_id: RunId::from_u128(2),
        tool_use_id: Some(ToolUseId::from_u128(3)),
        backend_id: "local".to_owned(),
        status: SandboxPreflightStatus::Failed,
        policy: sandbox_summary(),
        policy_hash: SandboxPolicyHash::from_hex(
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        )
        .unwrap(),
        reason: "network policy unavailable".to_owned(),
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    });
    assert_eq!(
        serde_json::to_value(failed).unwrap()["type"],
        "sandbox_preflight_failed"
    );
}

#[test]
fn schema_export_contains_authorization_contracts() {
    let schemas = export_all_schemas();

    for key in [
        "action_plan_id",
        "action_plan_hash",
        "sandbox_policy_hash",
        "authorization_ticket_id",
        "action_resource",
        "tool_action_plan",
        "permission_review",
        "permission_confirmation",
        "sandbox_preflight_status",
        "sandbox_preflight_passed_event",
        "sandbox_preflight_failed_event",
        "mcp_server_scope",
    ] {
        assert!(schemas.contains_key(key), "missing schema: {key}");
    }
}
