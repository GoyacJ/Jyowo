use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    AgentId, Decision, Event, McpServerId, PermissionActorSource, PermissionMode,
    PermissionSubject, RequestId, RunId, SamplingBudgetDimension, SamplingDenyReason,
    SamplingOutcome, SessionId, TeamId, TrustLevel,
};
use harness_mcp::{
    AggregateBudget, JsonRpcRequest, McpEventSink, McpMetric, McpMetricOutcome, McpMetricsSink,
    McpTimeouts, ModelAllowlist, SamplingAllow, SamplingBudget, SamplingCachePolicy,
    SamplingDecision, SamplingJsonRpcHandler, SamplingPolicy, SamplingProvider, SamplingRateLimit,
    SamplingRequest, SamplingResponse, SamplingUsageSnapshot, MCP_SAMPLING_BUDGET_EXCEEDED_CODE,
    MCP_SAMPLING_DENIED_CODE,
};
use harness_tool::{PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision};
use parking_lot::Mutex;
use serde_json::json;

#[test]
fn denied_policy_rejects_and_emits_event() {
    let sink = Arc::new(CollectingSink::default());
    let decision = SamplingPolicy::denied().evaluate(
        sample_request(),
        SamplingUsageSnapshot::default(),
        McpTimeouts::default(),
        sink.clone(),
    );

    assert!(matches!(
        decision,
        SamplingDecision::Rejected { ref error, .. }
            if error.code == MCP_SAMPLING_DENIED_CODE
    ));
    assert!(matches!(
        sink.events().first(),
        Some(Event::McpSamplingRequested(event))
            if matches!(
                event.outcome,
                SamplingOutcome::Denied {
                    reason: SamplingDenyReason::PolicyDenied
                }
            )
    ));
}

#[test]
fn allow_auto_fails_closed_for_user_controlled_server() {
    let mut request = sample_request();
    request.server_trust = TrustLevel::UserControlled;
    let sink = Arc::new(CollectingSink::default());

    let decision = SamplingPolicy::allow_auto().evaluate(
        request,
        SamplingUsageSnapshot::default(),
        McpTimeouts::default(),
        sink.clone(),
    );

    assert!(matches!(
        decision,
        SamplingDecision::Rejected { ref error, .. }
            if error.code == MCP_SAMPLING_DENIED_CODE
    ));
    assert!(matches!(
        sink.events().first(),
        Some(Event::McpSamplingRequested(event))
            if matches!(
                event.outcome,
                SamplingOutcome::Denied {
                    reason: SamplingDenyReason::InlineUserSourceRefused
                }
            )
    ));
}

#[test]
fn permission_modes_downgrade_sampling_access() {
    let mut request = sample_request();
    request.permission_mode = PermissionMode::BypassPermissions;
    let denied = SamplingPolicy::allow_with_approval().evaluate(
        request,
        SamplingUsageSnapshot::default(),
        McpTimeouts::default(),
        Arc::new(CollectingSink::default()),
    );
    assert!(matches!(
        denied,
        SamplingDecision::Rejected { ref error, .. }
            if error.code == MCP_SAMPLING_DENIED_CODE
    ));

    let mut plan_request = sample_request();
    plan_request.permission_mode = PermissionMode::Plan;
    let approval = SamplingPolicy::allow_auto().evaluate(
        plan_request,
        SamplingUsageSnapshot::default(),
        McpTimeouts::default(),
        Arc::new(CollectingSink::default()),
    );
    assert!(matches!(
        approval,
        SamplingDecision::RequiresApproval { .. }
    ));
}

#[test]
fn per_request_budget_exceeded_returns_sampling_budget_error() {
    let policy = SamplingPolicy {
        allow: SamplingAllow::AllowAuto,
        per_request: SamplingBudget {
            max_input_tokens: 8,
            max_output_tokens: 4,
            max_tool_rounds: 0,
            timeout: Duration::from_secs(10),
        },
        ..SamplingPolicy::allow_auto()
    };
    let mut request = sample_request();
    request.input_tokens = 9;

    let decision = policy.evaluate(
        request,
        SamplingUsageSnapshot::default(),
        McpTimeouts::default(),
        Arc::new(CollectingSink::default()),
    );

    assert!(matches!(
        decision,
        SamplingDecision::Rejected {
            ref error,
            outcome: SamplingOutcome::BudgetExceeded {
                dimension: SamplingBudgetDimension::PerRequestInputTokens
            },
        } if error.code == MCP_SAMPLING_BUDGET_EXCEEDED_CODE
    ));
}

#[test]
fn aggregate_and_rate_limits_are_enforced() {
    let aggregate_policy = SamplingPolicy {
        allow: SamplingAllow::AllowAuto,
        aggregate: AggregateBudget {
            per_server_session_input_tokens: 10,
            per_server_session_output_tokens: 100,
            per_session_input_tokens: 100,
            per_session_output_tokens: 100,
            lock_after_exceeded: true,
        },
        ..SamplingPolicy::allow_auto()
    };
    let aggregate_decision = aggregate_policy.evaluate(
        sample_request(),
        SamplingUsageSnapshot {
            per_server_session_input_tokens: 9,
            ..SamplingUsageSnapshot::default()
        },
        McpTimeouts::default(),
        Arc::new(CollectingSink::default()),
    );
    assert!(matches!(
        aggregate_decision,
        SamplingDecision::Rejected {
            outcome: SamplingOutcome::BudgetExceeded {
                dimension: SamplingBudgetDimension::PerServerSessionInput
            },
            ..
        }
    ));

    let rate_policy = SamplingPolicy {
        allow: SamplingAllow::AllowAuto,
        rate_limit: SamplingRateLimit {
            per_server_rps: 1.0,
            per_session_rps: 10.0,
            burst: 10,
        },
        ..SamplingPolicy::allow_auto()
    };
    let rate_decision = rate_policy.evaluate(
        sample_request(),
        SamplingUsageSnapshot {
            current_per_server_rps: 1.0,
            ..SamplingUsageSnapshot::default()
        },
        McpTimeouts::default(),
        Arc::new(CollectingSink::default()),
    );
    assert!(matches!(
        rate_decision,
        SamplingDecision::Rejected {
            ref error,
            outcome: SamplingOutcome::RateLimited,
        } if error.code == MCP_SAMPLING_BUDGET_EXCEEDED_CODE
    ));
}

#[test]
fn accepted_decision_uses_isolated_cache_and_effective_timeout() {
    let policy = SamplingPolicy {
        allow: SamplingAllow::AllowAuto,
        cache: SamplingCachePolicy::IsolatedNamespace {
            ttl: Duration::from_secs(300),
        },
        per_request: SamplingBudget {
            timeout: Duration::from_secs(20),
            ..SamplingBudget::default()
        },
        ..SamplingPolicy::allow_auto()
    };
    let timeouts = McpTimeouts {
        sampling: Duration::from_secs(5),
        ..McpTimeouts::default()
    };

    let decision = policy.evaluate(
        sample_request(),
        SamplingUsageSnapshot::default(),
        timeouts,
        Arc::new(CollectingSink::default()),
    );

    assert!(matches!(
        decision,
        SamplingDecision::Allowed {
            effective_timeout,
            ref prompt_cache_namespace,
            ..
        } if effective_timeout == Duration::from_secs(5)
            && prompt_cache_namespace == "mcp::sampling::github::00000000000000000000000001"
    ));
}

#[test]
fn model_allowlist_rejects_unlisted_model() {
    let policy = SamplingPolicy {
        allow: SamplingAllow::AllowAuto,
        allowed_models: ModelAllowlist::restricted(["claude-3-5-sonnet".to_owned()]),
        ..SamplingPolicy::allow_auto()
    };
    let mut request = sample_request();
    request.model_id = Some("unlisted".to_owned());

    let decision = policy.evaluate(
        request,
        SamplingUsageSnapshot::default(),
        McpTimeouts::default(),
        Arc::new(CollectingSink::default()),
    );

    assert!(matches!(
        decision,
        SamplingDecision::Rejected {
            outcome: SamplingOutcome::Denied {
                reason: SamplingDenyReason::ModelNotAllowed
            },
            ..
        }
    ));
}

#[test]
fn sampling_provider_is_object_safe() {
    let provider: Arc<dyn SamplingProvider> = Arc::new(EchoSamplingProvider);
    assert_eq!(Arc::strong_count(&provider), 1);
}

#[tokio::test]
async fn jsonrpc_sampling_create_message_denies_by_default_and_emits_event() {
    let sink = Arc::new(CollectingSink::default());
    let metrics = Arc::new(CollectingMetrics::default());
    let handler = SamplingJsonRpcHandler::new(SamplingPolicy::denied(), sink.clone())
        .with_session_id(SessionId::from_u128(1))
        .with_run_id(Some(RunId::from_u128(2)))
        .with_server_id(McpServerId("github".to_owned()))
        .with_server_trust(TrustLevel::AdminTrusted)
        .with_metrics_sink(metrics.clone());

    let response = handler
        .handle_request(JsonRpcRequest::new(
            json!(3),
            "sampling/createMessage",
            Some(json!({
                "request_id": RequestId::from_u128(3),
                "model": "claude-3-5-sonnet",
                "input_tokens": 2,
                "max_tokens": 4,
                "messages": []
            })),
        ))
        .await;

    assert!(matches!(
        response.error,
        Some(ref error) if error.code == MCP_SAMPLING_DENIED_CODE
    ));
    assert!(matches!(
        sink.events().first(),
        Some(Event::McpSamplingRequested(event))
            if event.server_id == McpServerId("github".to_owned())
                && event.request_id == RequestId::from_u128(3)
                && matches!(
                    event.outcome,
                    SamplingOutcome::Denied {
                        reason: SamplingDenyReason::PolicyDenied
                    }
                )
    ));
    assert_eq!(metrics.sampling_outcomes(), vec![McpMetricOutcome::Denied]);
}

#[tokio::test]
async fn jsonrpc_sampling_create_message_invokes_provider_and_records_token_metrics() {
    let sink = Arc::new(CollectingSink::default());
    let metrics = Arc::new(CollectingMetrics::default());
    let handler = SamplingJsonRpcHandler::new(SamplingPolicy::allow_auto(), sink.clone())
        .with_session_id(SessionId::from_u128(1))
        .with_run_id(Some(RunId::from_u128(2)))
        .with_server_id(McpServerId("github".to_owned()))
        .with_server_trust(TrustLevel::AdminTrusted)
        .with_provider(Arc::new(EchoSamplingProvider))
        .with_metrics_sink(metrics.clone());

    let response = handler
        .handle_request(JsonRpcRequest::new(
            json!(3),
            "sampling/createMessage",
            Some(json!({
                "request_id": RequestId::from_u128(3),
                "model": "claude-3-5-sonnet",
                "input_tokens": 2,
                "max_tokens": 4,
                "messages": [{ "role": "user", "content": { "type": "text", "text": "hello" } }]
            })),
        ))
        .await;

    assert!(response.error.is_none());
    assert_eq!(
        response.result,
        Some(json!({
            "model": "test",
            "role": "assistant",
            "content": { "type": "text", "text": "ok" },
            "stopReason": "endTurn"
        }))
    );
    assert!(matches!(
        sink.events().first(),
        Some(Event::McpSamplingRequested(event))
            if event.server_id == McpServerId("github".to_owned())
                && event.model_id == Some("test".to_owned())
                && event.input_tokens == 1
                && event.output_tokens == 1
                && event.outcome == SamplingOutcome::Completed
    ));
    assert_eq!(metrics.sampling_outcomes(), vec![McpMetricOutcome::Success]);
    assert_eq!(metrics.sampling_token_sums(), (vec![1], vec![1]));
}

#[tokio::test]
async fn jsonrpc_sampling_create_message_rejects_auto_allow_without_authoritative_run_id() {
    let sink = Arc::new(CollectingSink::default());
    let provider = Arc::new(RecordingSamplingProvider::default());
    let broker = Arc::new(FixedPermissionBroker::new(Decision::AllowOnce));
    let handler = SamplingJsonRpcHandler::new(SamplingPolicy::allow_auto(), sink.clone())
        .with_session_id(SessionId::from_u128(1))
        .with_server_id(McpServerId("github".to_owned()))
        .with_server_trust(TrustLevel::AdminTrusted)
        .with_permission_broker(broker.clone())
        .with_provider(provider.clone());

    let response = handler
        .handle_request(JsonRpcRequest::new(
            json!(7),
            "sampling/createMessage",
            Some(json!({
                "request_id": RequestId::from_u128(7),
                "run_id": RunId::from_u128(99).to_string(),
                "model": "claude-3-5-sonnet",
                "input_tokens": 2,
                "max_tokens": 4,
                "messages": []
            })),
        ))
        .await;

    assert!(matches!(
        response.error,
        Some(ref error) if error.code == MCP_SAMPLING_DENIED_CODE
    ));
    assert!(broker.requests().is_empty());
    assert!(provider.last_request().is_none());
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
}

#[tokio::test]
async fn jsonrpc_sampling_create_message_waits_for_approval_before_provider_call() {
    let sink = Arc::new(CollectingSink::default());
    let provider = Arc::new(RecordingSamplingProvider::default());
    let broker = Arc::new(FixedPermissionBroker::new(Decision::AllowOnce));
    let actor_source = PermissionActorSource::TeamMember {
        team_id: TeamId::from_u128(3),
        agent_id: AgentId::from_u128(4),
        role: "researcher sk-abcdefghijklmnopqrstuvwxyz".to_owned(),
        parent_run_id: Some(RunId::from_u128(5)),
    };
    let expected_actor_source = PermissionActorSource::TeamMember {
        team_id: TeamId::from_u128(3),
        agent_id: AgentId::from_u128(4),
        role: "[REDACTED]".to_owned(),
        parent_run_id: Some(RunId::from_u128(5)),
    };
    let handler = SamplingJsonRpcHandler::new(SamplingPolicy::allow_with_approval(), sink.clone())
        .with_session_id(SessionId::from_u128(1))
        .with_run_id(Some(RunId::from_u128(2)))
        .with_server_id(McpServerId("github".to_owned()))
        .with_server_trust(TrustLevel::AdminTrusted)
        .with_permission_actor_source(actor_source.clone())
        .with_permission_broker(broker.clone())
        .with_provider(provider.clone());

    let response = handler
        .handle_request(JsonRpcRequest::new(
            json!(4),
            "sampling/createMessage",
            Some(json!({
                "request_id": RequestId::from_u128(4),
                "session_id": SessionId::from_u128(99).to_string(),
                "run_id": RunId::from_u128(98).to_string(),
                "server_id": "spoofed",
                "model": "claude-3-5-sonnet",
                "input_tokens": 2,
                "max_tokens": 4,
                "messages": []
            })),
        ))
        .await;

    assert!(response.error.is_none());
    assert_eq!(broker.requests().len(), 1);
    let broker_request = broker.requests().pop().expect("broker should see request");
    assert_eq!(broker_request.session_id, SessionId::from_u128(1));
    assert!(matches!(
        broker_request.subject,
        PermissionSubject::Custom { ref payload, .. }
            if payload["server_id"] == "github"
                && !payload.to_string().contains("spoofed")
    ));
    let broker_context = broker.contexts().pop().expect("broker should see context");
    assert_eq!(broker_context.session_id, SessionId::from_u128(1));
    assert_eq!(broker_context.run_id, Some(RunId::from_u128(2)));
    let request = provider.last_request().expect("provider was called");
    assert_eq!(request.session_id, SessionId::from_u128(1));
    assert_eq!(request.run_id, Some(RunId::from_u128(2)));
    assert_eq!(request.server_id, McpServerId("github".to_owned()));
    assert_eq!(
        request.prompt_cache_namespace.as_deref(),
        Some("mcp::sampling::github::00000000000000000000000001")
    );
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::PermissionRequested(permission)
                if permission.actor_source == expected_actor_source
                    && permission.run_id == RunId::from_u128(2)
                    && permission.session_id == SessionId::from_u128(1)
        )
    }));
}

#[tokio::test]
async fn jsonrpc_sampling_create_message_rejects_approval_without_authoritative_run_id() {
    let sink = Arc::new(CollectingSink::default());
    let provider = Arc::new(RecordingSamplingProvider::default());
    let broker = Arc::new(FixedPermissionBroker::new(Decision::AllowOnce));
    let handler = SamplingJsonRpcHandler::new(SamplingPolicy::allow_with_approval(), sink.clone())
        .with_session_id(SessionId::from_u128(1))
        .with_server_id(McpServerId("github".to_owned()))
        .with_server_trust(TrustLevel::AdminTrusted)
        .with_permission_broker(broker.clone())
        .with_provider(provider.clone());

    let response = handler
        .handle_request(JsonRpcRequest::new(
            json!(6),
            "sampling/createMessage",
            Some(json!({
                "request_id": RequestId::from_u128(6),
                "run_id": RunId::from_u128(99).to_string(),
                "model": "claude-3-5-sonnet",
                "input_tokens": 2,
                "max_tokens": 4,
                "messages": []
            })),
        ))
        .await;

    assert!(matches!(
        response.error,
        Some(ref error) if error.code == MCP_SAMPLING_DENIED_CODE
    ));
    assert!(broker.requests().is_empty());
    assert!(provider.last_request().is_none());
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
}

#[tokio::test]
async fn jsonrpc_sampling_create_message_does_not_call_provider_when_approval_denies() {
    let sink = Arc::new(CollectingSink::default());
    let provider = Arc::new(RecordingSamplingProvider::default());
    let handler = SamplingJsonRpcHandler::new(SamplingPolicy::allow_with_approval(), sink)
        .with_session_id(SessionId::from_u128(1))
        .with_run_id(Some(RunId::from_u128(2)))
        .with_server_id(McpServerId("github".to_owned()))
        .with_server_trust(TrustLevel::AdminTrusted)
        .with_permission_broker(Arc::new(FixedPermissionBroker::new(Decision::DenyOnce)))
        .with_provider(provider.clone());

    let response = handler
        .handle_request(JsonRpcRequest::new(
            json!(5),
            "sampling/createMessage",
            Some(json!({
                "request_id": RequestId::from_u128(5),
                "model": "claude-3-5-sonnet",
                "input_tokens": 2,
                "max_tokens": 4,
                "messages": []
            })),
        ))
        .await;

    assert!(matches!(
        response.error,
        Some(ref error) if error.code == MCP_SAMPLING_DENIED_CODE
    ));
    assert!(provider.last_request().is_none());
}

fn sample_request() -> SamplingRequest {
    SamplingRequest {
        session_id: SessionId::from_u128(1),
        run_id: Some(RunId::from_u128(2)),
        server_id: McpServerId("github".to_owned()),
        request_id: RequestId::from_u128(3),
        model_id: Some("claude-3-5-sonnet".to_owned()),
        input_tokens: 2,
        max_output_tokens: 4,
        tool_rounds: 0,
        requested_timeout: None,
        permission_mode: PermissionMode::Default,
        server_trust: TrustLevel::AdminTrusted,
        prompt_cache_namespace: None,
        params: json!({ "messages": [] }),
    }
}

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<Event>>,
}

impl CollectingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl McpEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

#[derive(Default)]
struct CollectingMetrics {
    metrics: Mutex<Vec<McpMetric>>,
}

impl CollectingMetrics {
    fn sampling_outcomes(&self) -> Vec<McpMetricOutcome> {
        self.metrics
            .lock()
            .iter()
            .filter_map(|metric| match metric {
                McpMetric::SamplingRequested { outcome } => Some(*outcome),
                _ => None,
            })
            .collect()
    }

    fn sampling_token_sums(&self) -> (Vec<u64>, Vec<u64>) {
        let metrics = self.metrics.lock();
        let input = metrics
            .iter()
            .filter_map(|metric| match metric {
                McpMetric::SamplingInputTokens { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        let output = metrics
            .iter()
            .filter_map(|metric| match metric {
                McpMetric::SamplingOutputTokens { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        (input, output)
    }
}

impl McpMetricsSink for CollectingMetrics {
    fn record(&self, metric: McpMetric) {
        self.metrics.lock().push(metric);
    }
}

struct EchoSamplingProvider;

#[async_trait]
impl SamplingProvider for EchoSamplingProvider {
    async fn create_message(
        &self,
        _request: SamplingRequest,
    ) -> Result<SamplingResponse, harness_mcp::McpError> {
        Ok(SamplingResponse {
            model_id: "test".to_owned(),
            content: json!({ "type": "text", "text": "ok" }),
            input_tokens: 1,
            output_tokens: 1,
        })
    }
}

#[derive(Default)]
struct RecordingSamplingProvider {
    last_request: Mutex<Option<SamplingRequest>>,
}

impl RecordingSamplingProvider {
    fn last_request(&self) -> Option<SamplingRequest> {
        self.last_request.lock().clone()
    }
}

#[async_trait]
impl SamplingProvider for RecordingSamplingProvider {
    async fn create_message(
        &self,
        request: SamplingRequest,
    ) -> Result<SamplingResponse, harness_mcp::McpError> {
        *self.last_request.lock() = Some(request);
        Ok(SamplingResponse {
            model_id: "test".to_owned(),
            content: json!({ "type": "text", "text": "ok" }),
            input_tokens: 1,
            output_tokens: 1,
        })
    }
}

struct FixedPermissionBroker {
    decision: Decision,
    requests: Mutex<Vec<PermissionRequest>>,
    contexts: Mutex<Vec<PermissionContext>>,
}

impl FixedPermissionBroker {
    fn new(decision: Decision) -> Self {
        Self {
            decision,
            requests: Mutex::new(Vec::new()),
            contexts: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<PermissionRequest> {
        self.requests.lock().clone()
    }

    fn contexts(&self) -> Vec<PermissionContext> {
        self.contexts.lock().clone()
    }
}

#[async_trait]
impl PermissionBroker for FixedPermissionBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.requests.lock().push(request);
        self.contexts.lock().push(ctx);
        self.decision.clone()
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}
