use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use futures::{
    stream::{self, BoxStream},
    StreamExt,
};
use harness_contracts::{
    CorrelationId, DecidedBy, Decision, DecisionScope, Event, FallbackPolicy, ForkReason,
    InteractivityLevel, JournalError, JournalOffset, NoopRedactor, PermissionActorSource,
    PermissionMode, PermissionSubject, RequestId, RunId, SessionId, Severity, SubagentId,
    SubagentStatus, SubagentTerminationReason, TeamId, TenantId, TimeoutPolicy, ToolUseId,
    UsageSnapshot,
};
use harness_journal::{
    AppendMetadata, EventEnvelope, EventEnvelopePage, EventStore, InMemoryEventStore, PrunePolicy,
    PruneReport, ReplayCursor, SessionFilter, SessionSnapshot, SessionSummary,
};
use harness_permission::{
    NoopDecisionPersistence, PermissionAuthority, PermissionBroker, PermissionContext,
    PermissionRequest,
};
use harness_subagent::{
    ChildRunOutcome, ChildRunRequest, ChildSessionRunner, DefaultSubagentRunner, ParentContext,
    SubagentAdmin, SubagentError, SubagentPermissionBridge, SubagentRunner,
    SubagentRunnerCapAdapter, SubagentSpec,
};
use tokio::sync::Notify;

#[tokio::test]
async fn bridge_forwards_and_resolves_child_permission_requests() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let subagent_id = SubagentId::new();
    let child_run_id = RunId::new();
    let correlation_id = CorrelationId::new();
    let broker = Arc::new(AllowBroker);
    let bridge = permission_bridge(
        broker,
        store.clone(),
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        subagent_id,
    )
    .with_child_context(child_session_id, child_run_id, correlation_id);
    let request_id = RequestId::new();
    let subject = PermissionSubject::ToolInvocation {
        tool: "FileWrite".to_owned(),
        input: serde_json::json!({ "path": "README.md" }),
    };

    let decision = bridge
        .decide(
            PermissionRequest {
                request_id,
                tenant_id: TenantId::SINGLE,
                session_id: child_session_id,
                tool_use_id: ToolUseId::new(),
                tool_name: "FileWrite".to_owned(),
                subject: subject.clone(),
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                confirmation_expected: None,
                created_at: harness_contracts::now(),
            },
            PermissionContext {
                permission_mode: PermissionMode::Default,
                previous_mode: None,
                session_id: child_session_id,
                tenant_id: TenantId::SINGLE,
                run_id: None,
                interactivity: InteractivityLevel::FullyInteractive,
                timeout_policy: Some(TimeoutPolicy {
                    deadline_ms: 30_000,
                    default_on_timeout: Decision::DenyOnce,
                    heartbeat_interval_ms: None,
                }),
                fallback_policy: FallbackPolicy::DenyAll,
                hook_overrides: Vec::new(),
            },
        )
        .await;

    assert_eq!(decision, Decision::AllowOnce);
    let events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionForwarded(forwarded)
                if forwarded.parent_session_id == parent_session_id
                    && forwarded.subagent_id == subagent_id
                    && forwarded.original_request_id == request_id
                    && forwarded.subject == subject
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionResolved(resolved)
                if resolved.parent_session_id == parent_session_id
                    && resolved.subagent_id == subagent_id
                    && resolved.original_request_id == request_id
                    && resolved.decision == Decision::AllowOnce
                    && matches!(resolved.decided_by, DecidedBy::ParentForwarded { .. })
        )
    }));
    let parent_envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_envelopes
        .iter()
        .filter(|envelope| matches!(
            envelope.payload,
            Event::SubagentPermissionForwarded(_) | Event::SubagentPermissionResolved(_)
        ))
        .all(|envelope| envelope.correlation_id == correlation_id));

    let child_events: Vec<_> = store
        .read(TenantId::SINGLE, child_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(child_events.iter().any(|event| {
        matches!(
            event,
            Event::PermissionRequested(requested)
                if requested.request_id == request_id
                    && requested.session_id == child_session_id
                    && requested.run_id == child_run_id
                    && requested.actor_source == PermissionActorSource::Subagent {
                        subagent_id,
                        parent_session_id,
                        parent_run_id,
                        team_id: None,
                        team_member_profile_id: None
                    }
        )
    }));
    assert!(child_events.iter().any(|event| {
        matches!(
            event,
            Event::PermissionResolved(resolved)
                if resolved.request_id == request_id
                    && resolved.decision == Decision::AllowOnce
                    && matches!(resolved.decided_by, DecidedBy::ParentForwarded {
                        parent_session_id: forwarded_parent,
                        ..
                    } if forwarded_parent == parent_session_id)
        )
    }));
}

#[tokio::test]
async fn bridge_records_team_member_permission_attribution() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let child_run_id = RunId::new();
    let subagent_id = SubagentId::new();
    let team_id = TeamId::new();
    let request_id = RequestId::new();
    let bridge = permission_bridge(
        Arc::new(AllowBroker),
        store.clone(),
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        subagent_id,
    )
    .with_team_attribution(team_id, "reviewer")
    .with_child_context(child_session_id, child_run_id, CorrelationId::new());

    let decision = bridge
        .decide(
            PermissionRequest {
                request_id,
                tenant_id: TenantId::SINGLE,
                session_id: child_session_id,
                tool_use_id: ToolUseId::new(),
                tool_name: "FileWrite".to_owned(),
                subject: PermissionSubject::ToolInvocation {
                    tool: "FileWrite".to_owned(),
                    input: serde_json::json!({ "path": "README.md" }),
                },
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                confirmation_expected: None,
                created_at: harness_contracts::now(),
            },
            permission_context(child_session_id),
        )
        .await;

    assert_eq!(decision, Decision::AllowOnce);
    let parent_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionForwarded(forwarded)
                if forwarded.subagent_id == subagent_id
                    && forwarded.original_request_id == request_id
                    && forwarded.team_id == Some(team_id)
                    && forwarded.team_member_profile_id.as_deref() == Some("reviewer")
        )
    }));
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionResolved(resolved)
                if resolved.subagent_id == subagent_id
                    && resolved.original_request_id == request_id
                    && resolved.team_id == Some(team_id)
                    && resolved.team_member_profile_id.as_deref() == Some("reviewer")
        )
    }));
}

#[tokio::test]
async fn bridge_denies_without_parent_broker_when_permission_audit_append_fails() {
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let subagent_id = SubagentId::new();
    let broker = Arc::new(CountingBroker::default());
    let bridge = permission_bridge(
        broker.clone(),
        Arc::new(FailingAppendStore),
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        subagent_id,
    )
    .with_child_context(child_session_id, RunId::new(), CorrelationId::new());

    let decision = bridge
        .decide(
            PermissionRequest {
                request_id: RequestId::new(),
                tenant_id: TenantId::SINGLE,
                session_id: child_session_id,
                tool_use_id: ToolUseId::new(),
                tool_name: "FileWrite".to_owned(),
                subject: PermissionSubject::ToolInvocation {
                    tool: "FileWrite".to_owned(),
                    input: serde_json::json!({ "path": "README.md" }),
                },
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                confirmation_expected: None,
                created_at: harness_contracts::now(),
            },
            permission_context(child_session_id),
        )
        .await;

    assert_eq!(decision, Decision::DenyOnce);
    assert_eq!(broker.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn runner_cap_adapter_applies_team_member_parent_attribution() {
    struct CapturingRunner {
        parent: Arc<tokio::sync::Mutex<Option<ParentContext>>>,
    }

    #[async_trait]
    impl SubagentRunner for CapturingRunner {
        async fn spawn(
            &self,
            spec: SubagentSpec,
            _input: harness_contracts::TurnInput,
            parent_ctx: ParentContext,
        ) -> Result<harness_subagent::SubagentHandle, SubagentError> {
            *self.parent.lock().await = Some(parent_ctx.clone());
            Ok(harness_subagent::SubagentHandle::ready(
                harness_subagent::SubagentAnnouncement {
                    subagent_id: SubagentId::new(),
                    parent_session_id: parent_ctx.parent_session_id,
                    status: SubagentStatus::Completed,
                    summary: spec.task,
                    result: None,
                    usage: UsageSnapshot::default(),
                    transcript_ref: None,
                    context_report: None,
                },
            ))
        }
    }

    let team_id = TeamId::new();
    let parent = Arc::new(tokio::sync::Mutex::new(None));
    let adapter = SubagentRunnerCapAdapter::from_runner_with_team_attribution(
        Arc::new(CapturingRunner {
            parent: Arc::clone(&parent),
        }),
        team_id,
        "reviewer",
    );
    let parent_context = harness_contracts::SubagentParentContext {
        tenant_id: TenantId::SINGLE,
        parent_session_id: SessionId::new(),
        parent_run_id: RunId::new(),
        depth: 0,
        sibling_count: 0,
        trigger_tool_use_id: None,
        correlation_id: CorrelationId::new(),
    };

    adapter
        .spawn(
            serde_json::to_value(SubagentSpec::minimal("worker", "inspect")).unwrap(),
            parent_context,
        )
        .await
        .expect("subagent spawns")
        .wait()
        .await
        .expect("announcement");

    let captured = parent.lock().await.clone().expect("parent captured");
    assert_eq!(captured.team_id, Some(team_id));
    assert_eq!(captured.team_member_profile_id.as_deref(), Some("reviewer"));
}

#[tokio::test]
async fn bridge_forwards_parent_hard_policy_probe() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let bridge = permission_bridge(
        Arc::new(HardPolicyBroker),
        store,
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        SubagentId::new(),
    );
    let request = PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: child_session_id,
        tool_use_id: ToolUseId::new(),
        tool_name: "FileWrite".to_owned(),
        subject: PermissionSubject::ToolInvocation {
            tool: "FileWrite".to_owned(),
            input: serde_json::json!({ "path": "README.md" }),
        },
        severity: Severity::High,
        scope_hint: DecisionScope::Any,
        confirmation_expected: None,
        created_at: harness_contracts::now(),
    };

    assert!(
        bridge
            .hard_policy_denies(&request, &permission_context(child_session_id))
            .await
    );
}

#[tokio::test]
async fn bridge_decide_denies_parent_hard_policy_before_parent_allow() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let child_run_id = RunId::new();
    let subagent_id = SubagentId::new();
    let correlation_id = CorrelationId::new();
    let bridge = permission_bridge(
        Arc::new(HardPolicyBroker),
        store.clone(),
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        subagent_id,
    )
    .with_child_context(child_session_id, child_run_id, correlation_id);
    let request_id = RequestId::new();

    let decision = bridge
        .decide(
            PermissionRequest {
                request_id,
                tenant_id: TenantId::SINGLE,
                session_id: child_session_id,
                tool_use_id: ToolUseId::new(),
                tool_name: "FileWrite".to_owned(),
                subject: PermissionSubject::ToolInvocation {
                    tool: "FileWrite".to_owned(),
                    input: serde_json::json!({ "path": "README.md" }),
                },
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                confirmation_expected: None,
                created_at: harness_contracts::now(),
            },
            permission_context(child_session_id),
        )
        .await;

    assert_eq!(decision, Decision::DenyOnce);

    let parent_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionResolved(resolved)
                if resolved.parent_session_id == parent_session_id
                    && resolved.subagent_id == subagent_id
                    && resolved.original_request_id == request_id
                    && resolved.decision == Decision::DenyOnce
        )
    }));

    let child_events: Vec<_> = store
        .read(TenantId::SINGLE, child_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(child_events.iter().any(|event| {
        matches!(
            event,
            Event::PermissionResolved(resolved)
                if resolved.request_id == request_id
                    && resolved.decision == Decision::DenyOnce
        )
    }));
}

#[tokio::test]
async fn bridge_marks_child_permission_request_auto_resolved_for_bypass_mode() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let child_run_id = RunId::new();
    let correlation_id = CorrelationId::new();
    let bridge = permission_bridge(
        Arc::new(AllowBroker),
        store.clone(),
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        SubagentId::new(),
    )
    .with_child_context(child_session_id, child_run_id, correlation_id);
    let request_id = RequestId::new();
    let mut ctx = permission_context(child_session_id);
    ctx.permission_mode = PermissionMode::BypassPermissions;
    ctx.interactivity = InteractivityLevel::NoInteractive;

    let decision = bridge
        .decide(
            PermissionRequest {
                request_id,
                tenant_id: TenantId::SINGLE,
                session_id: child_session_id,
                tool_use_id: ToolUseId::new(),
                tool_name: "FileWrite".to_owned(),
                subject: PermissionSubject::ToolInvocation {
                    tool: "FileWrite".to_owned(),
                    input: serde_json::json!({ "path": "README.md" }),
                },
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                confirmation_expected: None,
                created_at: harness_contracts::now(),
            },
            ctx,
        )
        .await;

    assert_eq!(decision, Decision::AllowOnce);
    let child_events: Vec<_> = store
        .read(TenantId::SINGLE, child_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(child_events.iter().any(|event| {
        matches!(
            event,
            Event::PermissionRequested(requested)
                if requested.request_id == request_id && requested.auto_resolved
        )
    }));
}

#[tokio::test]
async fn bridge_preserves_decision_scope_matrix() {
    let cases = vec![
        (Decision::AllowOnce, DecisionScope::Any),
        (
            Decision::AllowSession,
            DecisionScope::ToolName("FileRead".to_owned()),
        ),
        (
            Decision::AllowPermanent,
            DecisionScope::Category("filesystem".to_owned()),
        ),
        (
            Decision::DenyOnce,
            DecisionScope::PathPrefix(PathBuf::from("/tmp/work")),
        ),
        (
            Decision::DenyPermanent,
            DecisionScope::GlobPattern("**/*.pem".to_owned()),
        ),
        (
            Decision::Escalate,
            DecisionScope::ExactArgs(serde_json::json!({ "path": "README.md" })),
        ),
        (
            Decision::AllowOnce,
            DecisionScope::ExactCommand {
                command: "cargo test".to_owned(),
                cwd: Some(PathBuf::from("/tmp/work")),
            },
        ),
        (
            Decision::DenyOnce,
            DecisionScope::ExecuteCodeScript {
                script_hash: [7; 32],
            },
        ),
    ];

    for (decision, scope) in cases {
        let store: Arc<InMemoryEventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let parent_session_id = SessionId::new();
        let parent_run_id = RunId::new();
        let child_session_id = SessionId::new();
        let child_run_id = RunId::new();
        let subagent_id = SubagentId::new();
        let correlation_id = CorrelationId::new();
        let bridge = permission_bridge(
            Arc::new(FixedBroker {
                decision: decision.clone(),
            }),
            store.clone(),
            TenantId::SINGLE,
            parent_session_id,
            parent_run_id,
            subagent_id,
        )
        .with_child_context(child_session_id, child_run_id, correlation_id);
        let request_id = RequestId::new();

        let actual = bridge
            .decide(
                PermissionRequest {
                    request_id,
                    tenant_id: TenantId::SINGLE,
                    session_id: child_session_id,
                    tool_use_id: ToolUseId::new(),
                    tool_name: "FileRead".to_owned(),
                    subject: PermissionSubject::ToolInvocation {
                        tool: "FileRead".to_owned(),
                        input: serde_json::json!({ "path": "README.md" }),
                    },
                    severity: Severity::Medium,
                    scope_hint: scope.clone(),
                    confirmation_expected: None,
                    created_at: harness_contracts::now(),
                },
                permission_context(child_session_id),
            )
            .await;
        assert_eq!(actual, decision);

        let parent_envelopes: Vec<_> = store
            .read_envelopes(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect()
            .await;
        assert!(parent_envelopes.iter().any(|envelope| {
            envelope.correlation_id == correlation_id
                && matches!(
                    &envelope.payload,
                    Event::SubagentPermissionResolved(resolved)
                        if resolved.original_request_id == request_id
                            && resolved.decision == decision
                            && matches!(
                                resolved.decided_by,
                                DecidedBy::ParentForwarded {
                                    parent_session_id: forwarded_parent,
                                    ..
                                } if forwarded_parent == parent_session_id
                            )
                )
        }));

        let child_envelopes: Vec<_> = store
            .read_envelopes(TenantId::SINGLE, child_session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect()
            .await;
        assert!(child_envelopes.iter().any(|envelope| {
            envelope.correlation_id == correlation_id
                && matches!(
                    &envelope.payload,
                    Event::PermissionResolved(resolved)
                        if resolved.request_id == request_id
                            && resolved.decision == decision
                            && resolved.scope == scope
                            && matches!(
                                resolved.decided_by,
                                DecidedBy::ParentForwarded {
                                    parent_session_id: forwarded_parent,
                                    ..
                                } if forwarded_parent == parent_session_id
                            )
                )
        }));
    }
}

fn permission_bridge(
    parent_broker: Arc<dyn PermissionBroker>,
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    parent_session_id: SessionId,
    parent_run_id: RunId,
    subagent_id: SubagentId,
) -> SubagentPermissionBridge {
    let parent_authority = Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::clone(&parent_broker))
            .with_interactive_broker(Arc::clone(&parent_broker))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .expect("test permission authority should build"),
    );
    SubagentPermissionBridge::with_parent_authority(
        parent_authority,
        event_store,
        tenant_id,
        parent_session_id,
        parent_run_id,
        subagent_id,
    )
}

#[tokio::test]
async fn admin_lists_status_and_cancels_running_subagents() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(WaitingChild::default());
    let runner = Arc::new(DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    ));
    let parent = ParentContext::for_test(0);
    let spawn = {
        let runner = runner.clone();
        tokio::spawn(async move {
            runner
                .spawn(
                    SubagentSpec::minimal("reviewer", "inspect"),
                    test_input("inspect"),
                    parent,
                )
                .await
        })
    };

    child.started.notified().await;
    let running = runner.list().await;
    assert_eq!(running.len(), 1);
    let subagent_id = running[0].subagent_id;
    assert!(runner.status(subagent_id).await.is_some());

    runner
        .cancel(subagent_id)
        .await
        .expect("admin cancel should reach running child");
    let result = spawn.await.unwrap();
    assert!(matches!(result, Err(SubagentError::Cancelled)));
}

#[tokio::test]
async fn admin_pause_blocks_spawning_and_interrupt_is_audited() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(WaitingChild::default());
    let runner = Arc::new(DefaultSubagentRunner::new(
        child.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    ));
    let parent = ParentContext::for_test(0);

    runner
        .pause_spawning(true, "ops".to_owned(), Some("drain".to_owned()))
        .await
        .unwrap();
    assert!(runner.is_spawning_paused().await);
    let paused_error = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .expect_err("pause should reject new subagents");
    assert_eq!(paused_error, SubagentError::SpawningPaused);

    runner
        .pause_spawning(false, "ops".to_owned(), None)
        .await
        .unwrap();
    let spawn = {
        let runner = runner.clone();
        let parent = parent.clone();
        tokio::spawn(async move {
            runner
                .spawn(
                    SubagentSpec::minimal("reviewer", "inspect"),
                    test_input("inspect"),
                    parent,
                )
                .await
        })
    };
    child.started.notified().await;
    let running = runner.list_active().await;
    assert_eq!(running.len(), 1);

    runner
        .interrupt(running[0].subagent_id, "ops".to_owned())
        .await
        .unwrap();
    runner
        .interrupt(SubagentId::new(), "ops".to_owned())
        .await
        .expect("unknown interrupt should be idempotent");
    let result = spawn.await.unwrap();
    assert!(matches!(result, Err(SubagentError::Cancelled)));

    let events: Vec<_> = store
        .read(
            TenantId::SINGLE,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    SubagentTerminationReason::AdminInterrupted { admin_id }
                        if admin_id == "ops"
                )
        )
    }));
    assert!(store
        .query_after(TenantId::SINGLE, None, 64)
        .await
        .unwrap()
        .into_iter()
        .any(|envelope| {
            matches!(
                envelope.payload,
                Event::SubagentSpawnPaused(paused)
                    if paused.paused && paused.by == "ops"
            )
        }));
}

struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct FixedBroker {
    decision: Decision,
}

#[async_trait]
impl PermissionBroker for FixedBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.decision.clone()
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

#[derive(Default)]
struct CountingBroker {
    calls: AtomicUsize,
}

#[async_trait]
impl PermissionBroker for CountingBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct FailingAppendStore;

#[async_trait]
impl EventStore for FailingAppendStore {
    async fn append(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        Err(JournalError::Message("append failed".to_owned()))
    }

    async fn append_with_metadata(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _metadata: AppendMetadata,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        Err(JournalError::Message("append failed".to_owned()))
    }

    async fn read_envelopes(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn page_session_envelopes(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _after_event_id: Option<harness_contracts::EventId>,
        _limit: usize,
    ) -> Result<EventEnvelopePage, JournalError> {
        Ok(EventEnvelopePage {
            envelopes: Vec::new(),
            next_event_id: None,
        })
    }

    async fn query_after(
        &self,
        _tenant: TenantId,
        _after: Option<harness_contracts::EventId>,
        _limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Ok(Vec::new())
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(None)
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Ok(false)
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Ok(PruneReport {
            events_removed: 0,
            snapshots_removed: 0,
            bytes_freed: 0,
        })
    }
}

struct HardPolicyBroker;

#[async_trait]
impl PermissionBroker for HardPolicyBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        true
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

fn permission_context(session_id: SessionId) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id,
        tenant_id: TenantId::SINGLE,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::DenyAll,
        hook_overrides: Vec::new(),
    }
}

#[derive(Default)]
struct WaitingChild {
    started: Notify,
}

#[async_trait]
impl ChildSessionRunner for WaitingChild {
    async fn run_child(&self, request: ChildRunRequest) -> Result<ChildRunOutcome, SubagentError> {
        self.started.notify_waiters();
        request.cancellation.cancelled().await;
        Ok(ChildRunOutcome {
            status: SubagentStatus::Cancelled,
            summary: "cancelled".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

fn test_input(text: &str) -> harness_contracts::TurnInput {
    harness_contracts::TurnInput {
        message: harness_contracts::Message {
            id: harness_contracts::MessageId::new(),
            role: harness_contracts::MessageRole::User,
            parts: vec![harness_contracts::MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::Value::Null,
    }
}
