#![cfg(all(feature = "interactive", feature = "stream", feature = "testing"))]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::FutureExt;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError, PermissionMode,
    PermissionSubject, RequestId, RuleSource, SessionId, Severity, TenantId, TimeoutPolicy,
    ToolUseId,
};
use harness_permission::{
    DecisionHistory, DecisionLookup, DecisionPersistence, DirectBroker, NoopDecisionPersistence,
    PermissionAuthority, PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
    StreamBasedBroker, StreamBrokerConfig, TestBroker,
};

#[tokio::test]
async fn contract_direct_broker() {
    direct_fail_closed_default().await;
    direct_permission_context_required().await;
    direct_no_state_across_calls().await;
}

#[tokio::test]
async fn contract_stream_broker() {
    stream_fail_closed_default().await;
    stream_permission_context_required().await;
    stream_no_state_across_calls().await;
}

#[tokio::test]
async fn contract_test_broker() {
    test_fail_closed_default().await;
    test_permission_context_required().await;
    test_no_state_across_calls().await;
}

#[tokio::test]
async fn authority_policy_deny_does_not_depend_on_call_site_snapshot() {
    let authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(PolicyDenyBroker))
        .with_decision_store(Arc::new(IntegrityStore))
        .build()
        .unwrap();
    let mut request = permission_request("authority-deny");
    let ctx = permission_context(None);
    request.session_id = ctx.session_id;
    request.tenant_id = ctx.tenant_id;

    assert_eq!(authority.decide(request, ctx).await, Decision::DenyOnce);
}

#[tokio::test]
async fn authority_bypass_cannot_override_policy_deny() {
    let authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(PolicyDenyBroker))
        .with_decision_store(Arc::new(IntegrityStore))
        .build()
        .unwrap();
    let mut request = permission_request("authority-bypass-deny");
    let mut ctx = permission_context(None);
    ctx.permission_mode = PermissionMode::BypassPermissions;
    request.session_id = ctx.session_id;
    request.tenant_id = ctx.tenant_id;

    assert_eq!(authority.decide(request, ctx).await, Decision::DenyOnce);
}

#[tokio::test]
async fn authority_reuses_own_user_scoped_persisted_decisions_through_history() {
    let store = Arc::new(RecordingIntegrityStore::default());
    let first_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_interactive_broker(Arc::new(TestBroker::new(vec![Decision::AllowSession])))
        .with_decision_store(store.clone())
        .build()
        .unwrap();
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::FullyInteractive;
    let mut first_request = permission_request("authority-user-persisted");
    first_request.session_id = ctx.session_id;
    first_request.tenant_id = ctx.tenant_id;

    assert_eq!(
        first_authority.decide(first_request, ctx.clone()).await,
        Decision::AllowSession
    );

    let second_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_decision_store(store)
        .build()
        .unwrap();
    ctx.interactivity = InteractivityLevel::NoInteractive;
    let mut second_request = permission_request("authority-user-persisted");
    second_request.session_id = ctx.session_id;
    second_request.tenant_id = ctx.tenant_id;

    let outcome = second_authority
        .decide_with_audit(second_request, ctx)
        .await;
    assert_eq!(outcome.decision, Decision::AllowOnce);
    assert!(matches!(
        outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::PersistedDecision { .. }
    ));
}

#[tokio::test]
async fn authority_does_not_persist_durable_deny_as_reusable_allow() {
    let store = Arc::new(RecordingIntegrityStore::default());
    let first_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_interactive_broker(Arc::new(TestBroker::new(vec![Decision::DenyPermanent])))
        .with_decision_store(store.clone())
        .build()
        .unwrap();
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::FullyInteractive;
    let mut first_request = permission_request("authority-deny-not-persisted-as-allow");
    first_request.session_id = ctx.session_id;
    first_request.tenant_id = ctx.tenant_id;

    assert_eq!(
        first_authority.decide(first_request, ctx.clone()).await,
        Decision::DenyPermanent
    );

    let second_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_decision_store(store)
        .build()
        .unwrap();
    ctx.interactivity = InteractivityLevel::NoInteractive;
    let mut second_request = permission_request("authority-deny-not-persisted-as-allow");
    second_request.session_id = ctx.session_id;
    second_request.tenant_id = ctx.tenant_id;

    let outcome = second_authority
        .decide_with_audit(second_request, ctx)
        .await;
    assert_eq!(outcome.decision, Decision::DenyPermanent);
    assert!(matches!(
        outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::PersistedDecision { .. }
    ));
}

#[test]
fn authority_rejects_stream_broker_as_policy_anchor() {
    let (stream, _receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig::default());
    let error = match PermissionAuthority::builder()
        .with_policy_broker(Arc::new(stream))
        .with_decision_store(Arc::new(IntegrityStore))
        .build()
    {
        Ok(_) => panic!("stream broker should not anchor production authority"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("hard policy"));
}

#[test]
fn noop_persistence_does_not_satisfy_authority_integrity() {
    let noop = NoopDecisionPersistence;
    assert!(!noop.supports_integrity());
    let error = match PermissionAuthority::builder()
        .with_policy_broker(Arc::new(PolicyDenyBroker))
        .with_decision_store(Arc::new(noop))
        .build()
    {
        Ok(_) => panic!("noop persistence should not satisfy authority integrity"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("integrity"));
}

#[tokio::test]
async fn authority_transient_store_does_not_reuse_or_persist_durable_decisions() {
    let store = Arc::new(TransientRecordingStore::default());
    let first_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_interactive_broker(Arc::new(TestBroker::new(vec![Decision::AllowPermanent])))
        .with_transient_decision_store(store.clone())
        .build()
        .unwrap();
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::FullyInteractive;
    let mut first_request = permission_request("authority-transient");
    first_request.session_id = ctx.session_id;
    first_request.tenant_id = ctx.tenant_id;

    assert_eq!(
        first_authority.decide(first_request, ctx.clone()).await,
        Decision::AllowPermanent
    );
    assert_eq!(store.persist_count.load(Ordering::SeqCst), 0);
    assert_eq!(store.lookup_count.load(Ordering::SeqCst), 0);

    let second_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_transient_decision_store(store.clone())
        .build()
        .unwrap();
    ctx.interactivity = InteractivityLevel::NoInteractive;
    let mut second_request = permission_request("authority-transient");
    second_request.session_id = ctx.session_id;
    second_request.tenant_id = ctx.tenant_id;

    let outcome = second_authority
        .decide_with_audit(second_request, ctx)
        .await;
    assert_eq!(outcome.decision, Decision::DenyOnce);
    assert!(matches!(
        outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::NoInteractive
    ));
    assert_eq!(store.persist_count.load(Ordering::SeqCst), 0);
    assert_eq!(store.lookup_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn authority_does_not_dedup_allow_when_durable_persistence_fails() {
    let authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_interactive_broker(Arc::new(TestBroker::new(vec![Decision::AllowPermanent])))
        .with_decision_store(Arc::new(FailingIntegrityStore))
        .build()
        .unwrap();
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::FullyInteractive;
    let mut first_request = permission_request("authority-persist-fail-no-dedup");
    first_request.session_id = ctx.session_id;
    first_request.tenant_id = ctx.tenant_id;

    let first_outcome = authority
        .decide_with_audit(first_request, ctx.clone())
        .await;
    assert_eq!(first_outcome.decision, Decision::DenyOnce);
    assert_eq!(
        first_outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::PersistenceFailed
    );

    ctx.interactivity = InteractivityLevel::NoInteractive;
    let mut second_request = permission_request("authority-persist-fail-no-dedup");
    second_request.session_id = ctx.session_id;
    second_request.tenant_id = ctx.tenant_id;
    let second_outcome = authority.decide_with_audit(second_request, ctx).await;
    assert_eq!(second_outcome.decision, Decision::DenyOnce);
    assert_eq!(
        second_outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::NoInteractive
    );
}

#[cfg(feature = "rule-engine")]
#[tokio::test]
async fn authority_reuses_history_before_ask_user_nointeractive_fallback() {
    let store = Arc::new(RecordingIntegrityStore::default());
    let first_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(EscalatePolicyBroker))
        .with_interactive_broker(Arc::new(TestBroker::new(vec![Decision::AllowPermanent])))
        .with_decision_store(store.clone())
        .build()
        .unwrap();
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::FullyInteractive;
    let mut first_request = permission_request("authority-ask-user-history");
    first_request.session_id = ctx.session_id;
    first_request.tenant_id = ctx.tenant_id;

    assert_eq!(
        first_authority.decide(first_request, ctx.clone()).await,
        Decision::AllowPermanent
    );

    let second_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(
            harness_permission::RuleEngineBroker::builder()
                .build()
                .await
                .unwrap(),
        ))
        .with_decision_store(store)
        .build()
        .unwrap();
    ctx.interactivity = InteractivityLevel::NoInteractive;
    let mut second_request = permission_request("authority-ask-user-history");
    second_request.session_id = ctx.session_id;
    second_request.tenant_id = ctx.tenant_id;

    let outcome = second_authority
        .decide_with_audit(second_request, ctx)
        .await;
    assert_eq!(outcome.decision, Decision::AllowOnce);
    assert!(matches!(
        outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::PersistedDecision { .. }
    ));
}

#[cfg(feature = "rule-engine")]
#[tokio::test]
async fn authority_bypass_applies_after_ask_user_rule_fallback() {
    let authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::new(
            harness_permission::RuleEngineBroker::builder()
                .build()
                .await
                .unwrap(),
        ))
        .with_decision_store(Arc::new(IntegrityStore))
        .build()
        .unwrap();
    let mut request = permission_request("authority-bypass-ask-user-fallback");
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::NoInteractive;
    ctx.permission_mode = PermissionMode::BypassPermissions;
    request.session_id = ctx.session_id;
    request.tenant_id = ctx.tenant_id;

    let outcome = authority.decide_with_audit(request, ctx).await;
    assert_eq!(outcome.decision, Decision::AllowOnce);
    assert_eq!(
        outcome.decided_by,
        harness_permission::PermissionAuthorityDecisionSource::PermissionMode
    );
}

async fn direct_fail_closed_default() {
    let broker = DirectBroker::new(|_request, _ctx| async { Decision::DenyOnce }.boxed());

    assert_eq!(
        broker
            .decide(permission_request("direct-deny"), permission_context(None))
            .await,
        Decision::DenyOnce
    );
}

async fn direct_permission_context_required() {
    let ctx = permission_context(None);
    let expected_session_id = ctx.session_id;
    let broker = DirectBroker::new(move |_request, ctx: PermissionContext| {
        async move {
            assert_eq!(ctx.session_id, expected_session_id);
            Decision::AllowOnce
        }
        .boxed()
    });

    assert_eq!(
        broker.decide(permission_request("direct-ctx"), ctx).await,
        Decision::AllowOnce
    );
}

async fn direct_no_state_across_calls() {
    let count = Arc::new(AtomicUsize::new(0));
    let broker = DirectBroker::new({
        let count = count.clone();
        move |_request, _ctx| {
            let next = count.fetch_add(1, Ordering::SeqCst);
            async move {
                match next {
                    0 => Decision::AllowOnce,
                    _ => Decision::DenyPermanent,
                }
            }
            .boxed()
        }
    });

    assert_eq!(
        broker
            .decide(permission_request("direct-first"), permission_context(None))
            .await,
        Decision::AllowOnce
    );
    assert_eq!(
        broker
            .decide(
                permission_request("direct-second"),
                permission_context(None)
            )
            .await,
        Decision::DenyPermanent
    );
}

async fn stream_fail_closed_default() {
    let (broker, _receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 0,
    });

    assert_eq!(
        broker
            .decide(permission_request("stream-deny"), permission_context(None))
            .await,
        Decision::DenyOnce
    );
}

async fn stream_permission_context_required() {
    let (broker, _receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let ctx = permission_context(Some(TimeoutPolicy {
        deadline_ms: 1,
        default_on_timeout: Decision::DenyPermanent,
        heartbeat_interval_ms: None,
    }));

    assert_eq!(
        broker
            .decide(permission_request("stream-timeout"), ctx)
            .await,
        Decision::DenyPermanent
    );
}

async fn stream_no_state_across_calls() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });

    assert_eq!(
        resolve_stream_request(
            &broker,
            &mut receiver,
            &resolver,
            permission_request("stream-first"),
            Decision::AllowOnce,
        )
        .await,
        Decision::AllowOnce
    );
    assert_eq!(
        resolve_stream_request(
            &broker,
            &mut receiver,
            &resolver,
            permission_request("stream-second"),
            Decision::DenyOnce,
        )
        .await,
        Decision::DenyOnce
    );
}

async fn test_fail_closed_default() {
    let broker = TestBroker::default();

    assert_eq!(
        broker
            .decide(permission_request("test-deny"), permission_context(None))
            .await,
        Decision::DenyOnce
    );
}

async fn test_permission_context_required() {
    let broker = TestBroker::new(vec![Decision::AllowOnce]);
    let ctx = permission_context(None);
    let expected_session_id = ctx.session_id;

    assert_eq!(
        broker.decide(permission_request("test-ctx"), ctx).await,
        Decision::AllowOnce
    );

    let calls = broker.calls();
    assert_eq!(calls[0].ctx.session_id, expected_session_id);
}

async fn test_no_state_across_calls() {
    let broker = TestBroker::new(vec![Decision::AllowOnce, Decision::DenyPermanent]);

    assert_eq!(
        broker
            .decide(permission_request("test-first"), permission_context(None))
            .await,
        Decision::AllowOnce
    );
    assert_eq!(
        broker
            .decide(permission_request("test-second"), permission_context(None))
            .await,
        Decision::DenyPermanent
    );
}

async fn resolve_stream_request(
    broker: &StreamBasedBroker,
    receiver: &mut tokio::sync::mpsc::Receiver<PermissionRequest>,
    resolver: &harness_permission::ResolverHandle,
    request: PermissionRequest,
    decision: Decision,
) -> Decision {
    let request_id = request.request_id;
    let resolved = tokio::join!(broker.decide(request, permission_context(None)), async {
        receiver.recv().await.unwrap();
        let option_id = resolver
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.request_id == request_id)
            .expect("pending request should exist")
            .decision_options
            .into_iter()
            .find(|option| option.decision == decision)
            .expect("pending option should exist")
            .option_id;
        let pending = resolver
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.request_id == request_id)
            .expect("pending request should exist");
        resolver
            .resolve_option_for(
                request_id,
                pending.request.tenant_id,
                pending.request.session_id,
                option_id,
                decision,
                None,
            )
            .await
            .unwrap();
    });
    resolved.0
}

struct PolicyDenyBroker;

#[async_trait]
impl PermissionBroker for PolicyDenyBroker {
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
        true
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct EscalatePolicyBroker;

#[async_trait]
impl PermissionBroker for EscalatePolicyBroker {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::Escalate
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Debug)]
struct IntegrityStore;

#[async_trait]
impl DecisionPersistence for IntegrityStore {
    fn supports_integrity(&self) -> bool {
        true
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Debug)]
struct FailingIntegrityStore;

#[async_trait]
impl DecisionPersistence for FailingIntegrityStore {
    fn supports_integrity(&self) -> bool {
        true
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Err(PermissionError::Message(
            "test persistence failure".to_owned(),
        ))
    }
}

#[async_trait]
impl DecisionHistory for FailingIntegrityStore {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(None)
    }
}

#[async_trait]
impl DecisionHistory for IntegrityStore {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(None)
    }
}

#[derive(Debug, Default)]
struct RecordingIntegrityStore {
    decisions: parking_lot::Mutex<Vec<PersistedDecision>>,
}

#[async_trait]
impl DecisionPersistence for RecordingIntegrityStore {
    fn supports_integrity(&self) -> bool {
        true
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.decisions.lock().push(decision);
        Ok(())
    }
}

#[derive(Debug, Default)]
struct TransientRecordingStore {
    persist_count: AtomicUsize,
    lookup_count: AtomicUsize,
}

#[async_trait]
impl DecisionPersistence for TransientRecordingStore {
    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persist_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl DecisionHistory for TransientRecordingStore {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        self.lookup_count.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}

#[async_trait]
impl DecisionHistory for RecordingIntegrityStore {
    async fn find_scoped_decision(
        &self,
        lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(self
            .decisions
            .lock()
            .iter()
            .find(|decision| {
                decision.source == RuleSource::User
                    && decision.source == lookup.decision_source
                    && harness_permission::policy_scope_matches_request(
                        &decision.scope,
                        &lookup.requested_scope,
                    )
                    && decision
                        .fingerprint
                        .is_some_and(|fingerprint| fingerprint == lookup.fingerprint)
            })
            .cloned())
    }
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
        action_plan_hash: harness_contracts::ActionPlanHash::default(),
        decision_options: Vec::new(),
        confirmation_expected: None,
        created_at: Utc::now(),
    }
}

fn permission_context(timeout_policy: Option<TimeoutPolicy>) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SHARED,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}
