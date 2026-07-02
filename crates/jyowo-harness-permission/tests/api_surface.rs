use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError, PermissionMode,
    PermissionSubject, RequestId, RuleSource, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    OverrideDecision, PermissionBroker, PermissionContext, PermissionRequest, PermissionRule,
    RuleAction, RuleProvider, RuleSnapshot, RulesUpdated,
};

struct ContextCheckingBroker;

#[async_trait]
impl PermissionBroker for ContextCheckingBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        assert_eq!(request.session_id, ctx.session_id);
        assert_eq!(request.tenant_id, ctx.tenant_id);
        assert_eq!(ctx.permission_mode, PermissionMode::DontAsk);
        Decision::DenyOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct EmptyRuleProvider;

#[async_trait]
impl RuleProvider for EmptyRuleProvider {
    fn provider_id(&self) -> &'static str {
        "empty"
    }

    fn source(&self) -> RuleSource {
        RuleSource::Workspace
    }

    async fn resolve_rules(
        &self,
        _tenant: TenantId,
    ) -> Result<Vec<PermissionRule>, PermissionError> {
        Ok(Vec::new())
    }

    fn watch(&self) -> Option<BoxStream<'static, RulesUpdated>> {
        None
    }
}

#[tokio::test]
async fn permission_broker_decide_requires_context() {
    let tenant_id = TenantId::SHARED;
    let session_id = SessionId::new();
    let request = PermissionRequest {
        request_id: RequestId::new(),
        tenant_id,
        session_id,
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
    };
    let ctx = PermissionContext {
        permission_mode: PermissionMode::DontAsk,
        previous_mode: Some(PermissionMode::Default),
        session_id,
        tenant_id,
        run_id: None,
        interactivity: InteractivityLevel::NoInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::DenyAll,
        hook_overrides: Vec::<OverrideDecision>::new(),
    };

    let broker: Box<dyn PermissionBroker> = Box::new(ContextCheckingBroker);
    assert_eq!(broker.decide(request, ctx).await, Decision::DenyOnce);
}

#[test]
fn permission_rule_and_provider_surface_use_contract_types() {
    let rule = PermissionRule {
        id: "deny-shell".to_owned(),
        priority: 100,
        scope: DecisionScope::ToolName("shell".to_owned()),
        action: RuleAction::Deny,
        source: RuleSource::Policy,
    };
    let snapshot = RuleSnapshot {
        rules: vec![rule],
        generation: 1,
        built_at: Utc::now(),
    };

    assert_eq!(snapshot.rules[0].source, RuleSource::Policy);

    let provider = EmptyRuleProvider;
    let stream: Option<BoxStream<'static, RulesUpdated>> = provider.watch();
    assert!(stream.is_none());
}
