use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::SandboxError;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, ActionResource, Decision, DecisionScope, Event, FallbackPolicy,
    InteractivityLevel, NetworkAccess, PermissionActorSource, PermissionMode, PermissionSubject,
    ResourceLimits, RuleSource, RunId, SandboxMode, SandboxPolicy, SandboxPreflightStatus,
    SandboxScope, SessionId, Severity, TenantId, ToolActionPlan, ToolUseId, WorkspaceAccess,
};
use harness_execution::{
    AuthorizationContext, AuthorizationEventSink, AuthorizationService, ExecutionError,
    TicketLedger,
};
use harness_permission::{
    NoopDecisionPersistence, PermissionAuthority, PermissionRule, RuleAction, RuleEngineBroker,
};
use harness_sandbox::{
    ExecContext, ExecSpec, ProcessHandle, SandboxBackend, SandboxBaseConfig, SandboxCapabilities,
    SessionSnapshotFile, SnapshotSpec,
};
use parking_lot::Mutex;

#[tokio::test]
async fn authorization_service_denies_hard_policy_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Policy, RuleAction::Deny).await,
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
async fn authorization_service_emits_permission_then_preflight_events_without_journal_dependency() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Session, RuleAction::Allow).await,
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

async fn real_authority(source: RuleSource, action: RuleAction) -> Arc<PermissionAuthority> {
    let broker = RuleEngineBroker::builder()
        .with_tenant(TenantId::SINGLE)
        .with_rules(vec![PermissionRule {
            id: "test-rule".to_owned(),
            priority: 10,
            scope: DecisionScope::Any,
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
