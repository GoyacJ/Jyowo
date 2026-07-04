#![cfg(feature = "stream")]

use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError, PermissionMode,
    PermissionOptionId, PermissionSubject, RequestId, SessionId, Severity, TenantId, TimeoutPolicy,
    ToolUseId,
};
use harness_permission::{
    default_permission_decision_options, CancelReason, PermissionBroker, PermissionContext,
    PermissionRequest, ResolverHandle, StreamBasedBroker, StreamBrokerConfig,
};

#[test]
fn stream_broker_can_be_constructed_without_tokio_runtime() {
    let (_broker, _receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: Some(Duration::from_secs(1)),
        max_pending: 16,
    });
}

#[tokio::test]
async fn stream_broker_sends_request_and_returns_resolved_decision() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;
    let ctx = permission_context(None);

    let decide = tokio::spawn(async move { broker.decide(request, ctx).await });
    let emitted = receiver.recv().await.unwrap();
    assert_eq!(emitted.request_id, request_id);

    let option_id = pending_option_id_for_decision(&resolver, request_id, Decision::AllowOnce);
    resolver
        .resolve_option(request_id, option_id, Decision::AllowOnce, None)
        .await
        .unwrap();

    assert_eq!(decide.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn stream_broker_preserves_confirmation_expected_on_pending_request() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let mut request = permission_request();
    request.confirmation_expected = Some("DELETE".to_owned());
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();

    let pending = resolver.pending_permission_requests();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request.request_id, request_id);
    assert_eq!(pending[0].confirmation_expected.as_deref(), Some("DELETE"));

    let option_id = pending_option_id_for_decision(&resolver, request_id, Decision::DenyOnce);
    resolver
        .resolve_option(request_id, option_id, Decision::DenyOnce, None)
        .await
        .unwrap();
    assert_eq!(decide.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn stream_broker_resolves_backend_authored_approve_option() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();
    let pending = resolver.pending_permission_requests();
    let approve = pending[0]
        .decision_options
        .iter()
        .find(|option| matches!(option.decision, Decision::AllowOnce))
        .unwrap();

    let resolved = resolver
        .resolve_option(request_id, approve.option_id, Decision::AllowOnce, None)
        .await
        .unwrap();

    assert_eq!(resolved, Decision::AllowOnce);
    assert_eq!(decide.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn stream_broker_resolves_backend_authored_deny_option() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();
    let pending = resolver.pending_permission_requests();
    let deny = pending[0]
        .decision_options
        .iter()
        .find(|option| matches!(option.decision, Decision::DenyOnce))
        .unwrap();

    let resolved = resolver
        .resolve_option(request_id, deny.option_id, Decision::DenyOnce, None)
        .await
        .unwrap();

    assert_eq!(resolved, Decision::DenyOnce);
    assert_eq!(decide.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn stream_broker_rejects_invalid_option_without_removing_pending_request() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();

    let error = resolver
        .resolve_option(
            request_id,
            harness_contracts::PermissionOptionId::new(),
            Decision::AllowOnce,
            None,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, PermissionError::Message(_)));
    assert_eq!(resolver.pending_permission_requests().len(), 1);

    let deny = resolver.pending_permission_requests()[0].decision_options[1].clone();
    resolver
        .resolve_option(request_id, deny.option_id, Decision::DenyOnce, None)
        .await
        .unwrap();
    assert_eq!(decide.await.unwrap(), Decision::DenyOnce);
}

#[test]
fn default_permission_options_do_not_derive_option_id_from_request_id() {
    let request = permission_request();
    let options = default_permission_decision_options(&request);

    assert_eq!(options.len(), 2);
    assert_ne!(
        options[0].option_id,
        derived_permission_option_id_for(request.request_id, 1)
    );
    assert_ne!(
        options[1].option_id,
        derived_permission_option_id_for(request.request_id, 2)
    );
    assert!(options
        .iter()
        .all(|option| option.scope == request.scope_hint));
    assert!(options.iter().all(|option| option.fingerprint.is_some()));
}

#[tokio::test]
async fn stream_broker_rejects_scope_mismatch_without_removing_pending_request() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;
    let tenant_id = request.tenant_id;
    let session_id = request.session_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();
    let approve = resolver.pending_permission_requests()[0].decision_options[0].clone();

    let error = resolver
        .resolve_option_for(
            request_id,
            TenantId::new(),
            session_id,
            approve.option_id,
            Decision::AllowOnce,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, PermissionError::Message(_)));
    assert_eq!(resolver.pending_permission_requests().len(), 1);

    let error = resolver
        .resolve_option_for(
            request_id,
            tenant_id,
            SessionId::new(),
            approve.option_id,
            Decision::AllowOnce,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, PermissionError::Message(_)));
    assert_eq!(resolver.pending_permission_requests().len(), 1);

    resolver
        .resolve_option_for(
            request_id,
            tenant_id,
            session_id,
            approve.option_id,
            Decision::AllowOnce,
            None,
        )
        .await
        .unwrap();
    assert_eq!(decide.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn stream_broker_rejects_submitted_decision_kind_conflict_without_removing_pending_request() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();
    let approve = resolver.pending_permission_requests()[0].decision_options[0].clone();

    let error = resolver
        .resolve_option(request_id, approve.option_id, Decision::DenyOnce, None)
        .await
        .unwrap_err();

    assert!(matches!(error, PermissionError::Message(_)));
    assert_eq!(resolver.pending_permission_requests().len(), 1);

    resolver
        .resolve_option(request_id, approve.option_id, Decision::AllowOnce, None)
        .await
        .unwrap();
    assert_eq!(decide.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn stream_broker_rejects_missing_confirmation_without_removing_pending_request() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let mut request = permission_request();
    request.confirmation_expected = Some("DELETE".to_owned());
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });
    receiver.recv().await.unwrap();
    let approve = resolver.pending_permission_requests()[0].decision_options[0].clone();

    let error = resolver
        .resolve_option(request_id, approve.option_id, Decision::AllowOnce, None)
        .await
        .unwrap_err();

    assert!(matches!(error, PermissionError::Message(_)));
    assert_eq!(resolver.pending_permission_requests().len(), 1);

    resolver
        .resolve_option(
            request_id,
            approve.option_id,
            Decision::AllowOnce,
            Some("DELETE"),
        )
        .await
        .unwrap();
    assert_eq!(decide.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn stream_broker_rejects_unknown_resolution() {
    let (_broker, _receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });

    let err = resolver
        .resolve_option(
            RequestId::new(),
            harness_contracts::PermissionOptionId::new(),
            Decision::AllowOnce,
            None,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, PermissionError::Message(_)));
}

#[tokio::test]
async fn stream_broker_keeps_high_and_critical_requests_pending_until_explicit_decision() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let high_request = permission_request_with_severity(Severity::High);
    let critical_request = permission_request_with_severity(Severity::Critical);
    let high_request_id = high_request.request_id;
    let critical_request_id = critical_request.request_id;
    let high_decide =
        tokio::spawn(async move { broker.decide(high_request, permission_context(None)).await });
    let emitted_high = receiver.recv().await.unwrap();
    assert_eq!(emitted_high.request_id, high_request_id);
    assert_eq!(emitted_high.severity, Severity::High);

    let pending = resolver.pending_requests();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, high_request_id);

    let option_id = pending_option_id_for_decision(&resolver, high_request_id, Decision::DenyOnce);
    resolver
        .resolve_option(high_request_id, option_id, Decision::DenyOnce, None)
        .await
        .unwrap();
    assert_eq!(high_decide.await.unwrap(), Decision::DenyOnce);

    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let critical_decide = tokio::spawn(async move {
        broker
            .decide(critical_request, permission_context(None))
            .await
    });
    let emitted_critical = receiver.recv().await.unwrap();
    assert_eq!(emitted_critical.request_id, critical_request_id);
    assert_eq!(emitted_critical.severity, Severity::Critical);

    let option_id =
        pending_option_id_for_decision(&resolver, critical_request_id, Decision::AllowOnce);
    resolver
        .resolve_option(critical_request_id, option_id, Decision::AllowOnce, None)
        .await
        .unwrap();
    assert_eq!(critical_decide.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn stream_broker_uses_context_timeout_default() {
    let (broker, _receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let ctx = permission_context(Some(TimeoutPolicy {
        deadline_ms: 10,
        default_on_timeout: Decision::DenyPermanent,
        heartbeat_interval_ms: None,
    }));

    assert_eq!(broker.decide(request, ctx).await, Decision::DenyPermanent);
}

#[tokio::test]
async fn stream_broker_emits_heartbeat_and_sweeps_timed_out_pending() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_millis(80)),
        heartbeat_interval: Some(Duration::from_millis(20)),
        max_pending: 16,
    });
    let mut heartbeats = broker.subscribe_heartbeats();
    let request = permission_request();
    let request_id = request.request_id;

    let decide =
        tokio::spawn(async move { broker.decide(request, permission_context(None)).await });

    receiver.recv().await.unwrap();
    let heartbeat = tokio::time::timeout(Duration::from_secs(1), heartbeats.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heartbeat.request_id, request_id);
    assert_eq!(decide.await.unwrap(), Decision::DenyOnce);
    assert!(matches!(
        resolver
            .resolve_option(
                request_id,
                harness_contracts::PermissionOptionId::new(),
                Decision::AllowOnce,
                None,
            )
            .await,
        Err(PermissionError::Message(_))
    ));
}

#[tokio::test]
async fn stream_broker_denies_when_pending_queue_is_full() {
    let (broker, _receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 0,
    });

    assert_eq!(
        broker
            .decide(permission_request(), permission_context(None))
            .await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn no_interactive_fails_safe_without_pending_request() {
    let (broker, mut receiver, _resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let mut ctx = permission_context(None);
    ctx.interactivity = InteractivityLevel::NoInteractive;

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::DenyOnce
    );
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn bypass_permission_mode_allows_without_pending_request() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let mut ctx = permission_context(None);
    ctx.permission_mode = PermissionMode::BypassPermissions;

    assert_eq!(
        broker.decide(permission_request(), ctx).await,
        Decision::AllowOnce
    );
    assert!(resolver.pending_requests().is_empty());
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn stream_broker_cancel_cleans_pending_and_unblocks_decide() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let request = permission_request();
    let request_id = request.request_id;
    let ctx = permission_context(None);

    let decide = tokio::spawn(async move { broker.decide(request, ctx).await });
    receiver.recv().await.unwrap();

    resolver
        .cancel(request_id, CancelReason::SessionEnded)
        .await
        .unwrap();

    assert_eq!(decide.await.unwrap(), Decision::DenyOnce);
    assert!(matches!(
        resolver
            .resolve_option(
                request_id,
                harness_contracts::PermissionOptionId::new(),
                Decision::AllowOnce,
                None,
            )
            .await,
        Err(PermissionError::Message(_))
    ));
}

fn pending_option_id_for_decision(
    resolver: &ResolverHandle,
    request_id: RequestId,
    decision: Decision,
) -> harness_contracts::PermissionOptionId {
    resolver
        .pending_permission_requests()
        .into_iter()
        .find(|pending| pending.request.request_id == request_id)
        .expect("pending request should exist")
        .decision_options
        .into_iter()
        .find(|option| option.decision == decision)
        .expect("pending option should exist")
        .option_id
}

trait ResolverHandleTestExt {
    async fn resolve_option(
        &self,
        request_id: RequestId,
        option_id: PermissionOptionId,
        submitted_decision: Decision,
        confirmation_text: Option<&str>,
    ) -> Result<Decision, PermissionError>;
}

impl ResolverHandleTestExt for ResolverHandle {
    async fn resolve_option(
        &self,
        request_id: RequestId,
        option_id: PermissionOptionId,
        submitted_decision: Decision,
        confirmation_text: Option<&str>,
    ) -> Result<Decision, PermissionError> {
        let pending = self
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.request_id == request_id);
        let (tenant_id, session_id) = pending
            .map(|pending| (pending.request.tenant_id, pending.request.session_id))
            .unwrap_or_else(|| (TenantId::new(), SessionId::new()));
        self.resolve_option_for(
            request_id,
            tenant_id,
            session_id,
            option_id,
            submitted_decision,
            confirmation_text,
        )
        .await
    }
}

fn derived_permission_option_id_for(
    request_id: RequestId,
    discriminator: u8,
) -> PermissionOptionId {
    let mut bytes = request_id.as_bytes();
    bytes[15] ^= discriminator;
    PermissionOptionId::from_u128(u128::from_be_bytes(bytes))
}

fn permission_request() -> PermissionRequest {
    permission_request_with_severity(Severity::Low)
}

fn permission_request_with_severity(severity: Severity) -> PermissionRequest {
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
        severity,
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
