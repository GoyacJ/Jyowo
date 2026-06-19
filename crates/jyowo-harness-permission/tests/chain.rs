use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    DecidedBy, Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionMode,
    PermissionSubject, RequestId, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    ChainedBroker, DecisionHistoryQuery, FallbackTerminator, PermissionBroker, PermissionContext,
    PermissionRequest, PersistedDecision, PriorDecision, RuleSnapshot,
};

#[tokio::test]
async fn chained_broker_tries_next_on_escalate() {
    let broker = ChainedBroker::builder()
        .push(Arc::new(StaticBroker(Decision::Escalate)))
        .push(Arc::new(StaticBroker(Decision::AllowOnce)))
        .build()
        .unwrap();

    assert_eq!(
        broker
            .decide(permission_request(), permission_context())
            .await,
        Decision::AllowOnce
    );
}

#[tokio::test]
async fn fallback_terminator_fail_closes_no_interactive_ask_user() {
    let broker = ChainedBroker::builder()
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::AskUser,
            Arc::new(EmptyHistory),
        )))
        .build()
        .unwrap();
    let mut ctx = permission_context();
    ctx.interactivity = InteractivityLevel::NoInteractive;

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn fallback_terminator_reads_closest_history() {
    let broker = ChainedBroker::builder()
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::ClosestMatchingRule,
            Arc::new(AllowHistory),
        )))
        .build()
        .unwrap();

    assert_eq!(
        broker
            .decide(permission_request(), permission_context())
            .await,
        Decision::AllowSession
    );
}

struct StaticBroker(Decision);

#[async_trait]
impl PermissionBroker for StaticBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.0.clone()
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct EmptyHistory;

#[async_trait]
impl DecisionHistoryQuery for EmptyHistory {
    async fn find_closest(&self, _scope: &DecisionScope) -> Option<PriorDecision> {
        None
    }
}

struct AllowHistory;

#[async_trait]
impl DecisionHistoryQuery for AllowHistory {
    async fn find_closest(&self, scope: &DecisionScope) -> Option<PriorDecision> {
        Some(PriorDecision {
            scope: scope.clone(),
            decision: Decision::AllowSession,
            decided_at: Utc::now(),
            decided_by: DecidedBy::Rule {
                rule_id: "prior".to_owned(),
            },
        })
    }
}

fn permission_request() -> PermissionRequest {
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject: PermissionSubject::CommandExec {
            command: "pwd".to_owned(),
            argv: vec!["pwd".to_owned()],
            cwd: None,
            fingerprint: None,
        },
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("shell".to_owned()),
        created_at: Utc::now(),
    }
}

fn permission_context() -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        rule_snapshot: Arc::new(RuleSnapshot {
            rules: Vec::new(),
            generation: 0,
            built_at: Utc::now(),
        }),
        hook_overrides: Vec::new(),
    }
}
