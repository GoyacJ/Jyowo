#![cfg(feature = "interactive")]

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use futures::FutureExt;
use harness_contracts::{
    Decision, DecisionId, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError,
    PermissionMode, PermissionSubject, RequestId, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    DecisionPersistence, DirectBroker, PermissionBroker, PermissionContext, PermissionRequest,
    PersistedDecision,
};
use parking_lot::Mutex;

#[derive(Default)]
struct RecordingPersistence {
    calls: Mutex<Vec<PersistedDecision>>,
}

#[async_trait]
impl DecisionPersistence for RecordingPersistence {
    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.calls.lock().push(decision);
        Ok(())
    }
}

#[tokio::test]
async fn direct_broker_callback_receives_request_and_context() {
    let request = permission_request();
    let ctx = permission_context();
    let expected_request_id = request.request_id;
    let expected_session_id = ctx.session_id;

    let broker = DirectBroker::new(move |request: PermissionRequest, ctx: PermissionContext| {
        async move {
            assert_eq!(request.request_id, expected_request_id);
            assert_eq!(ctx.session_id, expected_session_id);
            Decision::AllowOnce
        }
        .boxed()
    });

    let broker: Box<dyn PermissionBroker> = Box::new(broker);
    assert_eq!(broker.decide(request, ctx).await, Decision::AllowOnce);
}

#[tokio::test]
async fn direct_broker_persist_delegates_to_persistence() {
    let persistence = Arc::new(RecordingPersistence::default());
    let broker = DirectBroker::new(|_request, _ctx| async { Decision::DenyOnce }.boxed())
        .with_persistence(persistence.clone());
    let decision_id = DecisionId::new();
    let scope = DecisionScope::ToolName("shell".to_owned());
    let decision = PersistedDecision {
        decision_id,
        decision: Decision::DenyOnce,
        scope: scope.clone(),
        source: harness_contracts::RuleSource::Session,
        session_id: None,
        fingerprint: None,
    };

    broker.persist(decision.clone()).await.unwrap();

    assert_eq!(persistence.calls.lock().as_slice(), &[decision]);
}

fn permission_request() -> PermissionRequest {
    let tenant_id = TenantId::SHARED;
    let session_id = SessionId::new();
    PermissionRequest {
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
        action_plan_hash: harness_contracts::ActionPlanHash::default(),
        decision_options: Vec::new(),
        confirmation_expected: None,
        created_at: Utc::now(),
    }
}

fn permission_context() -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SHARED,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}
