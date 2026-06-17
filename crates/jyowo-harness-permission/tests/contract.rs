#![cfg(all(feature = "interactive", feature = "stream", feature = "mock"))]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use chrono::Utc;
use futures::FutureExt;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionMode, PermissionSubject,
    RequestId, SessionId, Severity, TenantId, TimeoutPolicy, ToolUseId,
};
use harness_permission::{
    DirectBroker, MockBroker, PermissionBroker, PermissionContext, PermissionRequest, RuleSnapshot,
    StreamBasedBroker, StreamBrokerConfig,
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
async fn contract_mock_broker() {
    mock_fail_closed_default().await;
    mock_permission_context_required().await;
    mock_no_state_across_calls().await;
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
            Decision::DenyPermanent,
        )
        .await,
        Decision::DenyPermanent
    );
}

async fn mock_fail_closed_default() {
    let broker = MockBroker::default();

    assert_eq!(
        broker
            .decide(permission_request("mock-deny"), permission_context(None))
            .await,
        Decision::DenyOnce
    );
}

async fn mock_permission_context_required() {
    let broker = MockBroker::new(vec![Decision::AllowOnce]);
    let ctx = permission_context(None);
    let expected_session_id = ctx.session_id;

    assert_eq!(
        broker.decide(permission_request("mock-ctx"), ctx).await,
        Decision::AllowOnce
    );

    let calls = broker.calls();
    assert_eq!(calls[0].ctx.session_id, expected_session_id);
}

async fn mock_no_state_across_calls() {
    let broker = MockBroker::new(vec![Decision::AllowOnce, Decision::DenyPermanent]);

    assert_eq!(
        broker
            .decide(permission_request("mock-first"), permission_context(None))
            .await,
        Decision::AllowOnce
    );
    assert_eq!(
        broker
            .decide(permission_request("mock-second"), permission_context(None))
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
        resolver.resolve(request_id, decision).await.unwrap();
    });
    resolved.0
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

fn permission_context(timeout_policy: Option<TimeoutPolicy>) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SHARED,
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
