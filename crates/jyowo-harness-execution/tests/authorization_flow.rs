use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::SandboxError;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, ActionResource, Decision, DecisionScope, Event, FallbackPolicy,
    HostRule, InteractivityLevel, NetworkAccess, PermissionActorSource, PermissionConfirmation,
    PermissionMode, PermissionSubject, ResourceLimits, RuleSource, RunId, SandboxMode,
    SandboxPolicy, SandboxPreflightStatus, SandboxScope, SessionId, Severity, TenantId,
    ToolActionPlan, ToolUseId, WorkspaceAccess,
};
use harness_execution::{
    AuthorizationContext, AuthorizationEventSink, AuthorizationService, ExecutionError,
    TicketLedger,
};
use harness_permission::{
    DangerousPatternLibrary, NoopDecisionPersistence, PermissionAuthority, PermissionBroker,
    PermissionContext, PermissionRequest, PermissionRule, PersistedDecision, RuleAction,
    RuleEngineBroker, StreamBasedBroker, StreamBrokerConfig,
};
use harness_sandbox::{
    ExecContext, ExecSpec, LocalSandbox, ProcessHandle, SandboxBackend, SandboxBaseConfig,
    SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
};
use parking_lot::Mutex;
use std::time::Duration;

#[tokio::test]
async fn authorization_service_denies_hard_policy_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Policy, RuleAction::Deny, DecisionScope::Any).await,
        Arc::new(TestSandbox::default()),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), action_plan("dangerous", DecisionScope::Any))
        .await
        .unwrap_err();

    assert!(matches!(error, ExecutionError::PermissionDenied { .. }));
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::DenyOnce
    ));
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn authorization_service_denies_hard_policy_under_bypass_mode_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let ledger = Arc::new(TicketLedger::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Policy, RuleAction::Deny, DecisionScope::Any).await,
        Arc::new(TestSandbox::default()),
        sink.clone(),
        ledger.clone(),
    );
    let mut context = context();
    context.permission_mode = PermissionMode::BypassPermissions;
    let plan = action_plan("dangerous", DecisionScope::Any);

    let error = service
        .authorize_plan(context, plan.clone())
        .await
        .unwrap_err();

    assert!(matches!(error, ExecutionError::PermissionDenied { .. }));
    assert!(matches!(
        ledger.consume(
            harness_contracts::AuthorizationTicketId::new(),
            &harness_execution::AuthorizationTicketClaims {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                tool_use_id: plan.tool_use_id,
                tool_name: plan.tool_name,
                action_plan_hash: plan.plan_hash,
            },
            Utc::now(),
        ),
        Err(ExecutionError::TicketUnknown { .. })
    ));
    let events = sink.events();
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::DenyOnce
    ));
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn authorization_service_denies_dangerous_command_under_bypass_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let ledger = Arc::new(TicketLedger::default());
    let service = AuthorizationService::new(
        dangerous_command_authority().await,
        Arc::new(TestSandbox::default()),
        sink.clone(),
        ledger.clone(),
    );
    let mut context = context();
    context.permission_mode = PermissionMode::BypassPermissions;
    let plan = dangerous_command_plan("rm -rf /");

    let error = service
        .authorize_plan(context, plan.clone())
        .await
        .unwrap_err();

    assert!(matches!(error, ExecutionError::PermissionDenied { .. }));
    assert!(matches!(
        ledger.consume(
            harness_contracts::AuthorizationTicketId::new(),
            &harness_execution::AuthorizationTicketClaims {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                tool_use_id: plan.tool_use_id,
                tool_name: plan.tool_name,
                action_plan_hash: plan.plan_hash,
            },
            Utc::now(),
        ),
        Err(ExecutionError::TicketUnknown { .. })
    ));
    let events = sink.events();
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::DenyOnce
    ));
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn authorization_service_emits_permission_then_preflight_events_without_journal_dependency() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Session, RuleAction::Allow, DecisionScope::Any).await,
        Arc::new(TestSandbox::default()),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let outcome = service
        .authorize_plan(context(), action_plan("safe", DecisionScope::Any))
        .await
        .unwrap();

    assert_eq!(outcome.decision, Decision::AllowOnce);
    assert_eq!(
        outcome.ticket.claims.action_plan_hash,
        outcome.action_plan_hash
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightPassed(preflight)
            if preflight.status == SandboxPreflightStatus::Passed
                && preflight.backend_id == "test-sandbox"
    ));
}

#[tokio::test]
async fn authorization_service_uses_sandbox_authority_for_exec_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let workspace = tempfile::tempdir().unwrap();
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("Bash".to_owned()),
        )
        .await,
        Arc::new(LocalSandbox::new(workspace.path())),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), command_plan("printf blocked"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "local"
                && failed.reason.contains("no-network")
    ));
}

#[tokio::test]
async fn authorization_service_uses_sandbox_authority_for_network_only_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("mcp_transport".to_owned()),
        )
        .await,
        Arc::new(NetworkCapablePreflightSandbox),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), network_only_plan())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "network-capable"
                && matches!(failed.policy.network, NetworkAccess::AllowList(_))
                && failed.policy_hash != Default::default()
                && failed.reason.contains("fine-grained network policy")
    ));
}

#[tokio::test]
async fn authorization_service_mints_ticket_after_sandbox_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("Bash".to_owned()),
        )
        .await,
        Arc::new(SlowPassingPreflightSandbox),
        sink.clone(),
        Arc::new(TicketLedger::new(Duration::from_millis(5))),
    );

    let operation = service
        .authorize_operation(context(), command_plan("printf authorized"))
        .await
        .unwrap();

    assert_eq!(operation.sandbox_backend_id, "slow-preflight");
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightPassed(passed)
            if passed.status == SandboxPreflightStatus::Passed
                && passed.backend_id == "slow-preflight"
    ));
}

#[tokio::test]
async fn authorization_service_declared_network_resource_requires_effective_network_policy() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("custom_network_tool".to_owned()),
        )
        .await,
        Arc::new(NetworkCapablePreflightSandbox),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), declared_network_resource_plan("network-capable"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "network-capable"
                && failed.reason.contains("fine-grained network policy")
    ));
}

#[tokio::test]
async fn authorization_service_preflights_declared_network_resource_even_without_network_policy() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("custom_network_tool".to_owned()),
        )
        .await,
        Arc::new(RejectingPreflightSandbox {
            backend_id: "network-resource-preflight",
            reason: "declared network resource preflight".to_owned(),
        }),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(
            context(),
            declared_network_resource_plan("network-resource-preflight"),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "network-resource-preflight"
                && matches!(failed.policy.network, NetworkAccess::AllowList(_))
                && failed.policy_hash != Default::default()
                && failed.reason.contains("declared network resource preflight")
    ));
}

#[tokio::test]
async fn authorization_service_carries_type_to_confirm_into_pending_permission() {
    let sink = Arc::new(RecordingSink::default());
    let (stream_broker, _receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let service = AuthorizationService::new(
        interactive_authority(Arc::new(stream_broker)).await,
        Arc::new(TestSandbox::default()),
        sink,
        Arc::new(TicketLedger::default()),
    );
    let mut context = context();
    context.interactivity = InteractivityLevel::FullyInteractive;
    let mut plan = action_plan(
        "write_file",
        DecisionScope::ToolName("write_file".to_owned()),
    );
    plan.review.confirmation = PermissionConfirmation::TypeToConfirm {
        expected: "DELETE".to_owned(),
    };
    let request_id = plan.tool_use_id;

    let authorize = tokio::spawn(async move { service.authorize_plan(context, plan).await });

    let pending = wait_for_pending_confirmation(&resolver, request_id).await;
    assert_eq!(pending.as_deref(), Some("DELETE"));

    let pending = resolver
        .pending_permission_requests()
        .into_iter()
        .find(|pending| pending.request.tool_use_id == request_id)
        .expect("permission should still be pending");
    let request_id = pending.request.request_id;
    let tenant_id = pending.request.tenant_id;
    let session_id = pending.request.session_id;
    let option_id = pending
        .decision_options
        .into_iter()
        .find(|option| option.decision == Decision::DenyOnce)
        .expect("deny option should exist")
        .option_id;
    resolver
        .resolve_option_for(
            request_id,
            tenant_id,
            session_id,
            option_id,
            Decision::DenyOnce,
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        authorize.await.unwrap(),
        Err(ExecutionError::PermissionDenied { .. })
    ));
}

async fn real_authority(
    source: RuleSource,
    action: RuleAction,
    scope: DecisionScope,
) -> Arc<PermissionAuthority> {
    let broker = RuleEngineBroker::builder()
        .with_tenant(TenantId::SINGLE)
        .with_rules(vec![PermissionRule {
            id: "test-rule".to_owned(),
            priority: 10,
            scope,
            action,
            source,
        }])
        .with_fallback(FallbackPolicy::AskUser)
        .build()
        .await
        .unwrap();

    Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::new(broker))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .unwrap(),
    )
}

async fn interactive_authority(
    interactive_broker: Arc<dyn PermissionBroker>,
) -> Arc<PermissionAuthority> {
    Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::new(EscalatingPolicyBroker))
            .with_interactive_broker(interactive_broker)
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .unwrap(),
    )
}

async fn dangerous_command_authority() -> Arc<PermissionAuthority> {
    let broker = RuleEngineBroker::builder()
        .with_tenant(TenantId::SINGLE)
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![PermissionRule {
            id: "allow-shell".to_owned(),
            priority: 10,
            scope: DecisionScope::ToolName("Bash".to_owned()),
            action: RuleAction::Allow,
            source: RuleSource::Session,
        }])
        .with_fallback(FallbackPolicy::AskUser)
        .build()
        .await
        .unwrap();

    Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::new(broker))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .unwrap(),
    )
}

async fn wait_for_pending_confirmation(
    resolver: &harness_permission::ResolverHandle,
    tool_use_id: ToolUseId,
) -> Option<String> {
    for _ in 0..50 {
        if let Some(pending) = resolver
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.tool_use_id == tool_use_id)
        {
            return pending.confirmation_expected;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    None
}

fn context() -> AuthorizationContext {
    AuthorizationContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        fallback_policy: FallbackPolicy::AskUser,
        workspace_root: PathBuf::from("/workspace"),
    }
}

fn action_plan(tool_name: &str, scope: DecisionScope) -> ToolActionPlan {
    ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: tool_name.to_owned(),
        actor_source: PermissionActorSource::ParentRun,
        subject: PermissionSubject::ToolInvocation {
            tool: tool_name.to_owned(),
            input: serde_json::json!({}),
        },
        scope,
        severity: Severity::Medium,
        resources: vec![ActionResource::Sandbox {
            backend_id: "test-sandbox".to_owned(),
            policy_hash: Default::default(),
        }],
        sandbox_policy: SandboxPolicy {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: NetworkAccess::None,
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: None,
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::None,
        network_access: NetworkAccess::None,
        review: Default::default(),
        plan_hash: ActionPlanHash::from_bytes([2; 32]),
        created_at: Utc::now(),
    }
}

fn dangerous_command_plan(command: &str) -> ToolActionPlan {
    let mut plan = action_plan("Bash", DecisionScope::ToolName("Bash".to_owned()));
    plan.subject = PermissionSubject::DangerousCommand {
        command: command.to_owned(),
        pattern_id: "unix-rm-rf-root".to_owned(),
        severity: Severity::Critical,
    };
    plan.severity = Severity::Critical;
    plan
}

fn command_plan(command: &str) -> ToolActionPlan {
    let mut plan = action_plan("Bash", DecisionScope::ToolName("Bash".to_owned()));
    plan.subject = PermissionSubject::CommandExec {
        command: command.to_owned(),
        argv: Vec::new(),
        cwd: None,
        fingerprint: None,
    };
    plan.resources = vec![ActionResource::Command {
        command: command.to_owned(),
        argv: Vec::new(),
        cwd: None,
        fingerprint: harness_contracts::ExecFingerprint([0; 32]),
    }];
    plan
}

fn network_only_plan() -> ToolActionPlan {
    let mut plan = action_plan(
        "mcp_transport",
        DecisionScope::ToolName("mcp_transport".to_owned()),
    );
    plan.resources = vec![
        ActionResource::Network {
            host: "api.example.test".to_owned(),
            port: Some(443),
        },
        ActionResource::Sandbox {
            backend_id: "network-capable".to_owned(),
            policy_hash: Default::default(),
        },
    ];
    let network_access = NetworkAccess::AllowList(vec![HostRule {
        pattern: "api.example.test".to_owned(),
        ports: Some(vec![443]),
    }]);
    plan.sandbox_policy.network = network_access.clone();
    plan.network_access = network_access;
    plan
}

fn declared_network_resource_plan(backend_id: &str) -> ToolActionPlan {
    let mut plan = action_plan(
        "custom_network_tool",
        DecisionScope::ToolName("custom_network_tool".to_owned()),
    );
    plan.resources = vec![
        ActionResource::Network {
            host: "api.example.test".to_owned(),
            port: Some(443),
        },
        ActionResource::Sandbox {
            backend_id: backend_id.to_owned(),
            policy_hash: Default::default(),
        },
    ];
    plan
}

struct SlowPassingPreflightSandbox;

#[async_trait]
impl SandboxBackend for SlowPassingPreflightSandbox {
    fn backend_id(&self) -> &str {
        "slow-preflight"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    fn preflight_execute(&self, _spec: &ExecSpec) -> Result<(), SandboxError> {
        std::thread::sleep(Duration::from_millis(30));
        Ok(())
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox does not execute".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct EscalatingPolicyBroker;

#[async_trait]
impl PermissionBroker for EscalatingPolicyBroker {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::Escalate
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        false
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

#[async_trait]
impl AuthorizationEventSink for RecordingSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        assert_eq!(tenant_id, TenantId::SINGLE);
        assert!(!session_id.to_string().is_empty());
        self.events.lock().extend(events);
        Ok(())
    }
}

#[derive(Default)]
struct TestSandbox;

#[async_trait]
impl SandboxBackend for TestSandbox {
    fn backend_id(&self) -> &str {
        "test-sandbox"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            max_concurrent_execs: 1,
            snapshot_kinds: BTreeSet::new(),
            ..SandboxCapabilities::default()
        }
    }

    fn base_config(&self) -> SandboxBaseConfig {
        SandboxBaseConfig::default()
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox does not execute".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct NetworkCapablePreflightSandbox;

#[async_trait]
impl SandboxBackend for NetworkCapablePreflightSandbox {
    fn backend_id(&self) -> &str {
        "network-capable"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_network: true,
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox does not execute".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct RejectingPreflightSandbox {
    backend_id: &'static str,
    reason: String,
}

#[async_trait]
impl SandboxBackend for RejectingPreflightSandbox {
    fn backend_id(&self) -> &str {
        self.backend_id
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_network: true,
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    fn preflight_execute(&self, _spec: &ExecSpec) -> Result<(), SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: self.reason.clone(),
        })
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox does not execute".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}
