#![cfg(feature = "testing")]

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    Decision, DecisionId, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError,
    PermissionMode, PermissionSubject, RequestId, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    DecisionPersistence, PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
    TestBroker,
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
async fn test_broker_replays_scripted_decisions_in_order() {
    let broker = TestBroker::new(vec![Decision::AllowOnce, Decision::DenyPermanent]);

    assert_eq!(
        broker
            .decide(permission_request("first"), permission_context())
            .await,
        Decision::AllowOnce
    );
    assert_eq!(
        broker
            .decide(permission_request("second"), permission_context())
            .await,
        Decision::DenyPermanent
    );
}

#[tokio::test]
async fn test_broker_fails_closed_when_script_is_exhausted() {
    let broker = TestBroker::default();

    assert_eq!(
        broker
            .decide(permission_request("exhausted"), permission_context())
            .await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn test_broker_records_request_and_context() {
    let broker = TestBroker::new(vec![Decision::AllowSession]);
    let request = permission_request("recorded");
    let ctx = permission_context();
    let expected_request_id = request.request_id;
    let expected_session_id = ctx.session_id;

    broker.decide(request, ctx).await;

    let calls = broker.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].request.request_id, expected_request_id);
    assert_eq!(calls[0].ctx.session_id, expected_session_id);
}

#[tokio::test]
async fn test_broker_persist_delegates_to_persistence() {
    let persistence = Arc::new(RecordingPersistence::default());
    let broker = TestBroker::default().with_persistence(persistence.clone());
    let decision_id = DecisionId::new();
    let scope = DecisionScope::ToolName("shell".to_owned());
    let decision = PersistedDecision {
        decision_id,
        decision: Decision::AllowSession,
        scope: scope.clone(),
        source: harness_contracts::RuleSource::Session,
        session_id: None,
        fingerprint: None,
    };

    broker.persist(decision.clone()).await.unwrap();

    assert_eq!(persistence.calls.lock().as_slice(), &[decision]);
}

#[tokio::test]
async fn test_broker_can_be_used_as_dyn_permission_broker() {
    let broker: Box<dyn PermissionBroker> = Box::new(TestBroker::new(vec![Decision::AllowOnce]));

    assert_eq!(
        broker
            .decide(permission_request("dyn"), permission_context())
            .await,
        Decision::AllowOnce
    );
}

fn permission_request(command: &str) -> PermissionRequest {
    let tenant_id = TenantId::SHARED;
    let session_id = SessionId::new();
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id,
        session_id,
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject: PermissionSubject::CommandExec {
            command: command.to_owned(),
            argv: vec![command.to_owned()],
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
        tenant_id: TenantId::SHARED,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}
