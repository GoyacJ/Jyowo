#![cfg(feature = "stream")]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError, PermissionMode,
    PermissionSubject, RequestId, RuleSource, SessionId, Severity, TenantId, TimeoutPolicy,
    ToolUseId,
};
use harness_permission::{
    CancelReason, PermissionBroker, PermissionContext, PermissionRequest, PermissionRule,
    RuleAction, RuleSnapshot, StreamBasedBroker, StreamBrokerConfig,
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

    resolver
        .resolve(request_id, Decision::AllowSession)
        .await
        .unwrap();

    assert_eq!(decide.await.unwrap(), Decision::AllowSession);
}

#[tokio::test]
async fn stream_broker_rejects_unknown_resolution() {
    let (_broker, _receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });

    let err = resolver
        .resolve(RequestId::new(), Decision::AllowOnce)
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

    resolver
        .resolve(high_request_id, Decision::DenyOnce)
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

    resolver
        .resolve(critical_request_id, Decision::AllowOnce)
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
        resolver.resolve(request_id, Decision::AllowOnce).await,
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
async fn policy_deny_wins_before_bypass_permission_mode() {
    let (broker, mut receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let mut ctx = permission_context(None);
    ctx.permission_mode = PermissionMode::BypassPermissions;
    ctx.rule_snapshot = Arc::new(RuleSnapshot {
        rules: vec![PermissionRule {
            id: "policy-deny-shell".to_owned(),
            priority: 1,
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
        resolver.resolve(request_id, Decision::AllowOnce).await,
        Err(PermissionError::Message(_))
    ));
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
        rule_snapshot: Arc::new(RuleSnapshot {
            rules: Vec::new(),
            generation: 0,
            built_at: Utc::now(),
        }),
        hook_overrides: Vec::new(),
    }
}
