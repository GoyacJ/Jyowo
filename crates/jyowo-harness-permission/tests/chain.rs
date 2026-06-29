use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    DecidedBy, Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionMode,
    PermissionSubject, RequestId, RuleSource, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    policy_scope_matches_request, ChainedBroker, DecisionHistoryQuery, FallbackTerminator,
    PermissionBroker, PermissionContext, PermissionRequest, PermissionRule, PersistedDecision,
    PriorDecision, RuleAction, RuleSnapshot,
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

#[tokio::test]
async fn fallback_terminator_bypass_allows_after_escalation() {
    let broker = ChainedBroker::builder()
        .push(Arc::new(StaticBroker(Decision::Escalate)))
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::AskUser,
            Arc::new(EmptyHistory),
        )))
        .build()
        .unwrap();
    let mut ctx = permission_context();
    ctx.permission_mode = PermissionMode::BypassPermissions;
    ctx.interactivity = InteractivityLevel::NoInteractive;

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::AllowOnce
    );
}

#[tokio::test]
async fn fallback_terminator_policy_deny_wins_before_bypass_permission_mode() {
    let broker = ChainedBroker::builder()
        .push(Arc::new(StaticBroker(Decision::Escalate)))
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::AskUser,
            Arc::new(EmptyHistory),
        )))
        .build()
        .unwrap();
    let mut ctx = permission_context();
    ctx.permission_mode = PermissionMode::BypassPermissions;
    ctx.rule_snapshot = Arc::new(RuleSnapshot {
        rules: vec![PermissionRule {
            id: "policy-deny-shell".to_owned(),
            priority: 0,
            scope: DecisionScope::ToolName("shell".to_owned()),
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }],
        generation: 1,
        built_at: Utc::now(),
    });

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn fallback_terminator_any_policy_deny_wins_before_bypass_permission_mode() {
    let broker = ChainedBroker::builder()
        .push(Arc::new(StaticBroker(Decision::Escalate)))
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::AskUser,
            Arc::new(EmptyHistory),
        )))
        .build()
        .unwrap();
    let mut ctx = permission_context();
    ctx.permission_mode = PermissionMode::BypassPermissions;
    ctx.rule_snapshot = Arc::new(RuleSnapshot {
        rules: vec![PermissionRule {
            id: "policy-deny-any".to_owned(),
            priority: 0,
            scope: DecisionScope::Any,
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }],
        generation: 1,
        built_at: Utc::now(),
    });

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn fallback_terminator_path_prefix_policy_deny_wins_before_bypass_permission_mode() {
    let broker = ChainedBroker::builder()
        .push(Arc::new(StaticBroker(Decision::Escalate)))
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::AskUser,
            Arc::new(EmptyHistory),
        )))
        .build()
        .unwrap();
    let mut ctx = permission_context();
    ctx.permission_mode = PermissionMode::BypassPermissions;
    ctx.rule_snapshot = Arc::new(RuleSnapshot {
        rules: vec![PermissionRule {
            id: "policy-deny-workspace-path".to_owned(),
            priority: 0,
            scope: DecisionScope::PathPrefix(PathBuf::from("/tmp/workspace")),
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }],
        generation: 1,
        built_at: Utc::now(),
    });
    let mut request = permission_request();
    request.scope_hint = DecisionScope::PathPrefix(PathBuf::from("/tmp/workspace/src/main.rs"));

    assert_eq!(broker.decide(request, ctx).await, Decision::DenyOnce);
}

#[test]
fn policy_scope_matching_covers_wide_policy_scopes_without_cross_variant_guessing() {
    assert!(policy_scope_matches_request(
        &DecisionScope::ToolName("shell".to_owned()),
        &DecisionScope::ToolName("shell".to_owned()),
    ));
    assert!(policy_scope_matches_request(
        &DecisionScope::Category("filesystem".to_owned()),
        &DecisionScope::Category("filesystem".to_owned()),
    ));
    assert!(policy_scope_matches_request(
        &DecisionScope::PathPrefix(PathBuf::from("/tmp/workspace")),
        &DecisionScope::PathPrefix(PathBuf::from("/tmp/workspace/src/main.rs")),
    ));
    assert!(!policy_scope_matches_request(
        &DecisionScope::PathPrefix(PathBuf::from("/tmp/workspace")),
        &DecisionScope::PathPrefix(PathBuf::from("/tmp/other/src/main.rs")),
    ));
    assert!(!policy_scope_matches_request(
        &DecisionScope::Category("filesystem".to_owned()),
        &DecisionScope::ToolName("FileWrite".to_owned()),
    ));
}

#[tokio::test]
async fn chained_broker_policy_deny_wins_before_prior_allow() {
    let broker = ChainedBroker::builder()
        .push(Arc::new(StaticBroker(Decision::AllowOnce)))
        .terminator(Arc::new(FallbackTerminator::new(
            FallbackPolicy::AskUser,
            Arc::new(EmptyHistory),
        )))
        .build()
        .unwrap();
    let mut ctx = permission_context();
    ctx.rule_snapshot = Arc::new(RuleSnapshot {
        rules: vec![PermissionRule {
            id: "policy-deny-shell".to_owned(),
            priority: 0,
            scope: DecisionScope::ToolName("shell".to_owned()),
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }],
        generation: 1,
        built_at: Utc::now(),
    });

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::DenyOnce
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
