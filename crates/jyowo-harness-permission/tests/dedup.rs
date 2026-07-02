use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError, PermissionMode,
    PermissionSubject, RequestId, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    DedupGate, DedupGateConfig, PermissionBroker, PermissionContext, PermissionRequest,
    PersistedDecision,
};
use serde_json::json;

#[tokio::test]
async fn dedup_gate_reuses_safe_prior_decision_inside_window() {
    let inner = Arc::new(CountingBroker::new(Decision::AllowOnce));
    let gate = DedupGate::with_config(
        inner.clone(),
        DedupGateConfig {
            window: Duration::from_secs(60),
        },
    );

    let first = tool_request();
    let second = PermissionRequest {
        request_id: RequestId::new(),
        tool_use_id: ToolUseId::new(),
        ..first.clone()
    };

    assert_eq!(
        gate.decide(first.clone(), permission_context()).await,
        Decision::AllowOnce
    );
    assert_eq!(
        gate.decide(second.clone(), permission_context()).await,
        Decision::AllowOnce
    );

    assert_eq!(inner.calls(), 1);
    let hit = gate
        .lookup(&second)
        .expect("second request should be cached");
    assert_eq!(hit.original_request_id, first.request_id);
}

#[tokio::test]
async fn dedup_gate_checks_hard_policy_before_reusing_prior_allow() {
    let inner = Arc::new(HardPolicyAfterFirstAllowBroker::default());
    let gate = DedupGate::new(inner.clone());

    let first = tool_request();
    let second = PermissionRequest {
        request_id: RequestId::new(),
        tool_use_id: ToolUseId::new(),
        ..first.clone()
    };

    assert_eq!(
        gate.decide(first, permission_context()).await,
        Decision::AllowOnce
    );
    assert_eq!(
        gate.decide(second, permission_context()).await,
        Decision::DenyOnce
    );
    assert_eq!(inner.calls(), 1);
}

#[tokio::test]
async fn dedup_gate_checks_hard_policy_before_first_decide() {
    let inner = Arc::new(AlwaysHardPolicyBroker::default());
    let gate = DedupGate::new(inner.clone());
    let request = tool_request();

    assert_eq!(
        gate.decide(request.clone(), permission_context()).await,
        Decision::DenyOnce
    );
    assert_eq!(inner.calls(), 0);
    assert!(
        gate.lookup(&request).is_none(),
        "hard policy denies should not be cached as reusable decisions"
    );
}

#[tokio::test]
async fn dedup_gate_does_not_reuse_dangerous_allow() {
    let inner = Arc::new(CountingBroker::new(Decision::AllowOnce));
    let gate = DedupGate::new(inner.clone());

    let first = command_request(Severity::High);
    let second = PermissionRequest {
        request_id: RequestId::new(),
        tool_use_id: ToolUseId::new(),
        ..first.clone()
    };

    assert_eq!(
        gate.decide(first, permission_context()).await,
        Decision::AllowOnce
    );
    assert_eq!(
        gate.decide(second, permission_context()).await,
        Decision::AllowOnce
    );

    assert_eq!(inner.calls(), 2);
}

#[tokio::test]
async fn dedup_gate_reuses_dangerous_deny() {
    let inner = Arc::new(CountingBroker::new(Decision::DenyOnce));
    let gate = DedupGate::new(inner.clone());

    let first = command_request(Severity::Critical);
    let second = PermissionRequest {
        request_id: RequestId::new(),
        tool_use_id: ToolUseId::new(),
        ..first.clone()
    };

    assert_eq!(
        gate.decide(first, permission_context()).await,
        Decision::DenyOnce
    );
    assert_eq!(
        gate.decide(second, permission_context()).await,
        Decision::DenyOnce
    );

    assert_eq!(inner.calls(), 1);
}

struct CountingBroker {
    decision: Decision,
    calls: AtomicUsize,
}

impl CountingBroker {
    fn new(decision: Decision) -> Self {
        Self {
            decision,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PermissionBroker for CountingBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.decision.clone()
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Default)]
struct HardPolicyAfterFirstAllowBroker {
    calls: AtomicUsize,
}

impl HardPolicyAfterFirstAllowBroker {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PermissionBroker for HardPolicyAfterFirstAllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Decision::AllowOnce
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        self.calls.load(Ordering::SeqCst) > 0
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Default)]
struct AlwaysHardPolicyBroker {
    calls: AtomicUsize,
}

impl AlwaysHardPolicyBroker {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PermissionBroker for AlwaysHardPolicyBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Decision::AllowOnce
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        true
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

fn tool_request() -> PermissionRequest {
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "read_blob".to_owned(),
        subject: PermissionSubject::ToolInvocation {
            tool: "read_blob".to_owned(),
            input: json!({ "path": "README.md" }),
        },
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("read_blob".to_owned()),
        created_at: Utc::now(),
    }
}

fn command_request(severity: Severity) -> PermissionRequest {
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject: PermissionSubject::CommandExec {
            command: "rm -rf tmp".to_owned(),
            argv: vec!["rm".to_owned(), "-rf".to_owned(), "tmp".to_owned()],
            cwd: None,
            fingerprint: None,
        },
        severity,
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
        hook_overrides: Vec::new(),
    }
}
