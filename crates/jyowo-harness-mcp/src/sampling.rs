use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use harness_contracts::{
    now, Event, ManifestOriginRef, McpSamplingRequestedEvent, McpServerId, McpServerSource,
    PermissionActorSource, PermissionMode, RequestId, RunId, SamplingBudgetDimension,
    SamplingDenyReason, SamplingOutcome, SessionId, TrustLevel,
};
use serde_json::{json, Value};

use crate::{
    authorize_mcp_sampling, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpAuthorizationContext,
    McpError, McpEventSink, McpMetric, McpMetricOutcome, McpMetricsSink, McpServerSpec,
    McpTimeouts, NoopMcpMetricsSink, TransportChoice,
};

pub const MCP_SAMPLING_DENIED_CODE: i32 = -32601;
pub const MCP_SAMPLING_BUDGET_EXCEEDED_CODE: i32 = -32029;
pub const MCP_SAMPLING_UPSTREAM_ERROR_CODE: i32 = -32030;
const JSONRPC_INVALID_PARAMS: i32 = -32602;

#[derive(Debug, Clone, PartialEq)]
pub struct SamplingPolicy {
    pub allow: SamplingAllow,
    pub allowed_models: ModelAllowlist,
    pub per_request: SamplingBudget,
    pub aggregate: AggregateBudget,
    pub rate_limit: SamplingRateLimit,
    pub log_level: SamplingLogLevel,
    pub cache: SamplingCachePolicy,
}

impl SamplingPolicy {
    pub fn denied() -> Self {
        Self {
            allow: SamplingAllow::Denied,
            allowed_models: ModelAllowlist::default(),
            per_request: SamplingBudget::default(),
            aggregate: AggregateBudget::default(),
            rate_limit: SamplingRateLimit::default(),
            log_level: SamplingLogLevel::Summary,
            cache: SamplingCachePolicy::default(),
        }
    }

    pub fn allow_auto() -> Self {
        Self {
            allow: SamplingAllow::AllowAuto,
            ..Self::denied()
        }
    }

    pub fn allow_with_approval() -> Self {
        Self {
            allow: SamplingAllow::AllowWithApproval,
            ..Self::denied()
        }
    }

    pub fn is_denied(&self) -> bool {
        self.allow == SamplingAllow::Denied
    }

    pub fn evaluate(
        &self,
        request: SamplingRequest,
        usage: SamplingUsageSnapshot,
        timeouts: McpTimeouts,
        event_sink: Arc<dyn McpEventSink>,
    ) -> SamplingDecision {
        let effective_timeout = self.effective_timeout(&request, timeouts);
        let prompt_cache_namespace = self.cache.namespace(&request);

        match self.effective_allow(&request) {
            EffectiveSamplingAllow::Denied(reason) => {
                return self.reject(
                    request,
                    event_sink,
                    prompt_cache_namespace,
                    SamplingOutcome::Denied { reason },
                    MCP_SAMPLING_DENIED_CODE,
                    "sampling/createMessage denied",
                );
            }
            EffectiveSamplingAllow::RequiresApproval => {
                return SamplingDecision::RequiresApproval {
                    request,
                    effective_timeout,
                    prompt_cache_namespace,
                };
            }
            EffectiveSamplingAllow::Allowed => {}
        }

        if !self.allowed_models.allows(request.model_id.as_deref()) {
            return self.reject(
                request,
                event_sink,
                prompt_cache_namespace,
                SamplingOutcome::Denied {
                    reason: SamplingDenyReason::ModelNotAllowed,
                },
                MCP_SAMPLING_DENIED_CODE,
                "sampling model is not allowed",
            );
        }

        if let Some(dimension) = self.per_request.exceeded_by(&request) {
            return self.reject(
                request,
                event_sink,
                prompt_cache_namespace,
                SamplingOutcome::BudgetExceeded { dimension },
                MCP_SAMPLING_BUDGET_EXCEEDED_CODE,
                "sampling per-request budget exceeded",
            );
        }

        if let Some(dimension) = self.aggregate.exceeded_by(&request, &usage) {
            return self.reject(
                request,
                event_sink,
                prompt_cache_namespace,
                SamplingOutcome::BudgetExceeded { dimension },
                MCP_SAMPLING_BUDGET_EXCEEDED_CODE,
                "sampling aggregate budget exceeded",
            );
        }

        if self.rate_limit.exceeded_by(&usage) {
            return self.reject(
                request,
                event_sink,
                prompt_cache_namespace,
                SamplingOutcome::RateLimited,
                MCP_SAMPLING_BUDGET_EXCEEDED_CODE,
                "sampling rate limit exceeded",
            );
        }

        SamplingDecision::Allowed {
            request,
            effective_timeout,
            prompt_cache_namespace,
        }
    }

    fn effective_timeout(&self, request: &SamplingRequest, timeouts: McpTimeouts) -> Duration {
        let requested = request
            .requested_timeout
            .unwrap_or(self.per_request.timeout);
        requested
            .min(self.per_request.timeout)
            .min(timeouts.sampling)
    }

    fn effective_allow(&self, request: &SamplingRequest) -> EffectiveSamplingAllow {
        match self.allow {
            SamplingAllow::Denied => {
                EffectiveSamplingAllow::Denied(SamplingDenyReason::PolicyDenied)
            }
            SamplingAllow::AllowWithApproval
                if matches!(
                    request.permission_mode,
                    PermissionMode::BypassPermissions | PermissionMode::DontAsk
                ) =>
            {
                EffectiveSamplingAllow::Denied(SamplingDenyReason::PermissionModeBlocked)
            }
            SamplingAllow::AllowWithApproval => EffectiveSamplingAllow::RequiresApproval,
            SamplingAllow::AllowAuto if request.server_trust == TrustLevel::UserControlled => {
                EffectiveSamplingAllow::Denied(SamplingDenyReason::InlineUserSourceRefused)
            }
            SamplingAllow::AllowAuto if request.permission_mode == PermissionMode::Plan => {
                EffectiveSamplingAllow::RequiresApproval
            }
            SamplingAllow::AllowAuto => EffectiveSamplingAllow::Allowed,
        }
    }

    fn reject(
        &self,
        request: SamplingRequest,
        event_sink: Arc<dyn McpEventSink>,
        prompt_cache_namespace: String,
        outcome: SamplingOutcome,
        code: i32,
        message: &'static str,
    ) -> SamplingDecision {
        emit_sampling_event(
            &request,
            &prompt_cache_namespace,
            outcome.clone(),
            event_sink,
        );
        SamplingDecision::Rejected {
            error: JsonRpcError {
                code,
                message: message.to_owned(),
                data: Some(json!({ "server_id": request.server_id.0 })),
            },
            outcome,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingAllow {
    Denied,
    AllowWithApproval,
    AllowAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplingBudget {
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_tool_rounds: u8,
    pub timeout: Duration,
}

impl Default for SamplingBudget {
    fn default() -> Self {
        Self {
            max_input_tokens: 8_192,
            max_output_tokens: 4_096,
            max_tool_rounds: 0,
            timeout: Duration::from_secs(30),
        }
    }
}

impl SamplingBudget {
    fn exceeded_by(&self, request: &SamplingRequest) -> Option<SamplingBudgetDimension> {
        if request.input_tokens > self.max_input_tokens {
            return Some(SamplingBudgetDimension::PerRequestInputTokens);
        }
        if request.max_output_tokens > self.max_output_tokens {
            return Some(SamplingBudgetDimension::PerRequestOutputTokens);
        }
        if request.tool_rounds > self.max_tool_rounds {
            return Some(SamplingBudgetDimension::PerRequestToolRounds);
        }
        if request
            .requested_timeout
            .is_some_and(|timeout| timeout > self.timeout)
        {
            return Some(SamplingBudgetDimension::PerRequestTimeout);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AggregateBudget {
    pub per_server_session_input_tokens: u64,
    pub per_server_session_output_tokens: u64,
    pub per_session_input_tokens: u64,
    pub per_session_output_tokens: u64,
    pub lock_after_exceeded: bool,
}

impl Default for AggregateBudget {
    fn default() -> Self {
        Self {
            per_server_session_input_tokens: 64_000,
            per_server_session_output_tokens: 32_000,
            per_session_input_tokens: 256_000,
            per_session_output_tokens: 128_000,
            lock_after_exceeded: true,
        }
    }
}

impl AggregateBudget {
    fn exceeded_by(
        &self,
        request: &SamplingRequest,
        usage: &SamplingUsageSnapshot,
    ) -> Option<SamplingBudgetDimension> {
        if usage
            .per_server_session_input_tokens
            .saturating_add(request.input_tokens)
            > self.per_server_session_input_tokens
        {
            return Some(SamplingBudgetDimension::PerServerSessionInput);
        }
        if usage
            .per_server_session_output_tokens
            .saturating_add(request.max_output_tokens)
            > self.per_server_session_output_tokens
        {
            return Some(SamplingBudgetDimension::PerServerSessionOutput);
        }
        if usage
            .per_session_input_tokens
            .saturating_add(request.input_tokens)
            > self.per_session_input_tokens
        {
            return Some(SamplingBudgetDimension::PerSessionInput);
        }
        if usage
            .per_session_output_tokens
            .saturating_add(request.max_output_tokens)
            > self.per_session_output_tokens
        {
            return Some(SamplingBudgetDimension::PerSessionOutput);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplingRateLimit {
    pub per_server_rps: f32,
    pub per_session_rps: f32,
    pub burst: u32,
}

impl Default for SamplingRateLimit {
    fn default() -> Self {
        Self {
            per_server_rps: 1.0,
            per_session_rps: 4.0,
            burst: 4,
        }
    }
}

impl SamplingRateLimit {
    fn exceeded_by(&self, usage: &SamplingUsageSnapshot) -> bool {
        usage.current_per_server_rps >= self.per_server_rps
            || usage.current_per_session_rps >= self.per_session_rps
            || usage.burst_used >= self.burst
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingLogLevel {
    None,
    Summary,
    FullPrompt,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SamplingCachePolicy {
    IsolatedNamespace { ttl: Duration },
    SharedWithMainSession { namespace: String },
}

impl Default for SamplingCachePolicy {
    fn default() -> Self {
        Self::IsolatedNamespace {
            ttl: Duration::from_secs(300),
        }
    }
}

impl SamplingCachePolicy {
    pub fn namespace(&self, request: &SamplingRequest) -> String {
        match self {
            Self::IsolatedNamespace { .. } => {
                format!(
                    "mcp::sampling::{}::{}",
                    request.server_id.0, request.session_id
                )
            }
            Self::SharedWithMainSession { namespace } => namespace.clone(),
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ModelAllowlist {
    #[default]
    InheritSession,
    Restricted(BTreeSet<String>),
}

impl ModelAllowlist {
    pub fn restricted(models: impl IntoIterator<Item = String>) -> Self {
        Self::Restricted(models.into_iter().collect())
    }

    pub fn allows(&self, model_id: Option<&str>) -> bool {
        match self {
            Self::InheritSession => true,
            Self::Restricted(models) => model_id.is_some_and(|model_id| models.contains(model_id)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SamplingRequest {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub server_id: McpServerId,
    pub request_id: RequestId,
    pub model_id: Option<String>,
    pub input_tokens: u64,
    pub max_output_tokens: u64,
    pub tool_rounds: u8,
    pub requested_timeout: Option<Duration>,
    pub permission_mode: PermissionMode,
    pub server_trust: TrustLevel,
    pub prompt_cache_namespace: Option<String>,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SamplingResponse {
    pub model_id: String,
    pub content: Value,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplingUsageSnapshot {
    pub per_server_session_input_tokens: u64,
    pub per_server_session_output_tokens: u64,
    pub per_session_input_tokens: u64,
    pub per_session_output_tokens: u64,
    pub current_per_server_rps: f32,
    pub current_per_session_rps: f32,
    pub burst_used: u32,
}

impl Default for SamplingUsageSnapshot {
    fn default() -> Self {
        Self {
            per_server_session_input_tokens: 0,
            per_server_session_output_tokens: 0,
            per_session_input_tokens: 0,
            per_session_output_tokens: 0,
            current_per_server_rps: 0.0,
            current_per_session_rps: 0.0,
            burst_used: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SamplingDecision {
    Allowed {
        request: SamplingRequest,
        effective_timeout: Duration,
        prompt_cache_namespace: String,
    },
    RequiresApproval {
        request: SamplingRequest,
        effective_timeout: Duration,
        prompt_cache_namespace: String,
    },
    Rejected {
        error: JsonRpcError,
        outcome: SamplingOutcome,
    },
}

#[async_trait]
pub trait SamplingProvider: Send + Sync + 'static {
    async fn create_message(&self, request: SamplingRequest) -> Result<SamplingResponse, McpError>;
}

#[derive(Clone)]
pub struct SamplingJsonRpcHandler {
    policy: SamplingPolicy,
    usage: SamplingUsageSnapshot,
    timeouts: McpTimeouts,
    event_sink: Arc<dyn McpEventSink>,
    session_id: SessionId,
    run_id: Option<RunId>,
    server_id: McpServerId,
    permission_mode: PermissionMode,
    permission_actor_source: PermissionActorSource,
    server_trust: TrustLevel,
    manifest_origin: ManifestOriginRef,
    metrics_sink: Arc<dyn McpMetricsSink>,
    provider: Option<Arc<dyn SamplingProvider>>,
    authorization_context: Option<McpAuthorizationContext>,
}

impl SamplingJsonRpcHandler {
    pub fn new(policy: SamplingPolicy, event_sink: Arc<dyn McpEventSink>) -> Self {
        Self {
            policy,
            usage: SamplingUsageSnapshot::default(),
            timeouts: McpTimeouts::default(),
            event_sink,
            session_id: SessionId::default(),
            run_id: None,
            server_id: McpServerId("unknown".to_owned()),
            permission_mode: PermissionMode::Default,
            permission_actor_source: PermissionActorSource::ParentRun,
            server_trust: TrustLevel::UserControlled,
            manifest_origin: ManifestOriginRef::File {
                path: "mcp-sampling-handler".to_owned(),
            },
            metrics_sink: Arc::new(NoopMcpMetricsSink),
            provider: None,
            authorization_context: None,
        }
    }

    #[must_use]
    pub fn with_usage(mut self, usage: SamplingUsageSnapshot) -> Self {
        self.usage = usage;
        self
    }

    #[must_use]
    pub fn with_timeouts(mut self, timeouts: McpTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    #[must_use]
    pub fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = session_id;
        self
    }

    #[must_use]
    pub fn with_run_id(mut self, run_id: Option<RunId>) -> Self {
        self.run_id = run_id;
        self
    }

    #[must_use]
    pub fn with_server_id(mut self, server_id: McpServerId) -> Self {
        self.server_id = server_id;
        self
    }

    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: PermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self
    }

    #[must_use]
    pub fn with_permission_actor_source(
        mut self,
        permission_actor_source: PermissionActorSource,
    ) -> Self {
        if let PermissionActorSource::McpServer { origin, .. } = &permission_actor_source {
            self.manifest_origin = origin.clone();
        }
        self.permission_actor_source = permission_actor_source;
        self
    }

    #[must_use]
    pub fn with_server_trust(mut self, server_trust: TrustLevel) -> Self {
        self.server_trust = server_trust;
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        self.metrics_sink = metrics_sink;
        self
    }

    #[must_use]
    pub fn with_provider(mut self, provider: Arc<dyn SamplingProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    #[must_use]
    pub fn with_authorization_context(mut self, context: McpAuthorizationContext) -> Self {
        self.authorization_context = Some(context);
        self
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        if request.method != "sampling/createMessage" {
            return JsonRpcResponse::failure(
                request.id,
                JsonRpcError {
                    code: MCP_SAMPLING_DENIED_CODE,
                    message: format!("method not found: {}", request.method),
                    data: None,
                },
            );
        }

        let sampling_request = match self.request_from_params(request.params.as_ref()) {
            Ok(request) => request,
            Err(error) => return JsonRpcResponse::failure(request.id, error),
        };

        match self.policy.evaluate(
            sampling_request,
            self.usage,
            self.timeouts,
            Arc::clone(&self.event_sink),
        ) {
            SamplingDecision::Rejected { error, .. } => {
                self.record_sampling(McpMetricOutcome::Denied);
                JsonRpcResponse::failure(request.id, error)
            }
            SamplingDecision::RequiresApproval {
                request: sampling_request,
                effective_timeout,
                prompt_cache_namespace,
            } => {
                if sampling_request.run_id.is_none() {
                    return self.missing_run_context_response(
                        request.id,
                        &sampling_request,
                        &prompt_cache_namespace,
                    );
                }
                let Some(authorization_context) = &self.authorization_context else {
                    self.record_sampling(McpMetricOutcome::Deferred);
                    return JsonRpcResponse::failure(
                        request.id,
                        JsonRpcError {
                            code: MCP_SAMPLING_DENIED_CODE,
                            message: "sampling authorization context is not configured".to_owned(),
                            data: Some(json!({ "server_id": self.server_id.0 })),
                        },
                    );
                };

                match self
                    .request_sampling_approval(
                        authorization_context,
                        &sampling_request,
                        &prompt_cache_namespace,
                    )
                    .await
                {
                    SamplingApproval::Allowed => {
                        return self
                            .invoke_provider(
                                request.id,
                                sampling_request,
                                effective_timeout,
                                prompt_cache_namespace,
                            )
                            .await;
                    }
                    SamplingApproval::Denied => {
                        self.record_sampling(McpMetricOutcome::Denied);
                        emit_sampling_event(
                            &sampling_request,
                            &prompt_cache_namespace,
                            SamplingOutcome::Denied {
                                reason: SamplingDenyReason::ApprovalDenied,
                            },
                            Arc::clone(&self.event_sink),
                        );
                        JsonRpcResponse::failure(
                            request.id,
                            JsonRpcError {
                                code: MCP_SAMPLING_DENIED_CODE,
                                message: "sampling approval denied".to_owned(),
                                data: Some(json!({ "server_id": self.server_id.0 })),
                            },
                        )
                    }
                }
            }
            SamplingDecision::Allowed {
                request: sampling_request,
                effective_timeout,
                prompt_cache_namespace,
            } => {
                if sampling_request.run_id.is_none() {
                    return self.missing_run_context_response(
                        request.id,
                        &sampling_request,
                        &prompt_cache_namespace,
                    );
                }
                let Some(authorization_context) = &self.authorization_context else {
                    self.record_sampling(McpMetricOutcome::Denied);
                    return JsonRpcResponse::failure(
                        request.id,
                        JsonRpcError {
                            code: MCP_SAMPLING_DENIED_CODE,
                            message: "sampling authorization context is not configured".to_owned(),
                            data: Some(json!({ "server_id": self.server_id.0 })),
                        },
                    );
                };
                match self
                    .request_sampling_approval(
                        authorization_context,
                        &sampling_request,
                        &prompt_cache_namespace,
                    )
                    .await
                {
                    SamplingApproval::Allowed => {
                        self.invoke_provider(
                            request.id,
                            sampling_request,
                            effective_timeout,
                            prompt_cache_namespace,
                        )
                        .await
                    }
                    SamplingApproval::Denied => {
                        self.record_sampling(McpMetricOutcome::Denied);
                        emit_sampling_event(
                            &sampling_request,
                            &prompt_cache_namespace,
                            SamplingOutcome::Denied {
                                reason: SamplingDenyReason::ApprovalDenied,
                            },
                            Arc::clone(&self.event_sink),
                        );
                        JsonRpcResponse::failure(
                            request.id,
                            JsonRpcError {
                                code: MCP_SAMPLING_DENIED_CODE,
                                message: "sampling approval denied".to_owned(),
                                data: Some(json!({ "server_id": self.server_id.0 })),
                            },
                        )
                    }
                }
            }
        }
    }

    fn missing_run_context_response(
        &self,
        jsonrpc_id: Value,
        request: &SamplingRequest,
        prompt_cache_namespace: &str,
    ) -> JsonRpcResponse {
        self.record_sampling(McpMetricOutcome::Denied);
        emit_sampling_event(
            request,
            prompt_cache_namespace,
            SamplingOutcome::Denied {
                reason: SamplingDenyReason::PolicyDenied,
            },
            Arc::clone(&self.event_sink),
        );
        JsonRpcResponse::failure(
            jsonrpc_id,
            JsonRpcError {
                code: MCP_SAMPLING_DENIED_CODE,
                message: "sampling requires an authoritative run context".to_owned(),
                data: Some(json!({ "server_id": self.server_id.0 })),
            },
        )
    }

    async fn invoke_provider(
        &self,
        jsonrpc_id: Value,
        mut sampling_request: SamplingRequest,
        effective_timeout: Duration,
        prompt_cache_namespace: String,
    ) -> JsonRpcResponse {
        if !self
            .policy
            .allowed_models
            .allows(sampling_request.model_id.as_deref())
        {
            self.record_sampling(McpMetricOutcome::Denied);
            emit_sampling_event(
                &sampling_request,
                &prompt_cache_namespace,
                SamplingOutcome::Denied {
                    reason: SamplingDenyReason::ModelNotAllowed,
                },
                Arc::clone(&self.event_sink),
            );
            return JsonRpcResponse::failure(
                jsonrpc_id,
                JsonRpcError {
                    code: MCP_SAMPLING_DENIED_CODE,
                    message: "sampling model is not allowed".to_owned(),
                    data: Some(json!({ "server_id": sampling_request.server_id.0 })),
                },
            );
        }

        let Some(provider) = &self.provider else {
            self.record_sampling(McpMetricOutcome::Deferred);
            return JsonRpcResponse::failure(
                jsonrpc_id,
                JsonRpcError {
                    code: MCP_SAMPLING_DENIED_CODE,
                    message: "sampling model invocation is deferred beyond P0".to_owned(),
                    data: Some(json!({ "server_id": self.server_id.0 })),
                },
            );
        };

        sampling_request.prompt_cache_namespace = Some(prompt_cache_namespace.clone());
        let started = Instant::now();
        match tokio::time::timeout(
            effective_timeout,
            provider.create_message(sampling_request.clone()),
        )
        .await
        {
            Ok(Ok(response)) => {
                self.record_sampling(McpMetricOutcome::Success);
                self.record_sampling_tokens(
                    &sampling_request.server_id,
                    response.input_tokens,
                    response.output_tokens,
                );
                emit_completed_sampling_event(
                    &sampling_request,
                    &prompt_cache_namespace,
                    &response,
                    started.elapsed(),
                    Arc::clone(&self.event_sink),
                );
                JsonRpcResponse::success(
                    jsonrpc_id,
                    json!({
                        "model": response.model_id,
                        "role": "assistant",
                        "content": response.content,
                        "stopReason": "endTurn"
                    }),
                )
            }
            Ok(Err(error)) => {
                self.record_sampling(McpMetricOutcome::Error);
                let message = error.to_string();
                emit_sampling_event(
                    &sampling_request,
                    &prompt_cache_namespace,
                    SamplingOutcome::UpstreamError {
                        code: MCP_SAMPLING_UPSTREAM_ERROR_CODE,
                        message: message.clone(),
                    },
                    Arc::clone(&self.event_sink),
                );
                JsonRpcResponse::failure(
                    jsonrpc_id,
                    JsonRpcError {
                        code: MCP_SAMPLING_UPSTREAM_ERROR_CODE,
                        message,
                        data: Some(json!({ "server_id": sampling_request.server_id.0 })),
                    },
                )
            }
            Err(_) => {
                self.record_sampling(McpMetricOutcome::Cancelled);
                emit_sampling_event(
                    &sampling_request,
                    &prompt_cache_namespace,
                    SamplingOutcome::Cancelled,
                    Arc::clone(&self.event_sink),
                );
                JsonRpcResponse::failure(
                    jsonrpc_id,
                    JsonRpcError {
                        code: MCP_SAMPLING_UPSTREAM_ERROR_CODE,
                        message: "sampling provider timed out".to_owned(),
                        data: Some(json!({ "server_id": sampling_request.server_id.0 })),
                    },
                )
            }
        }
    }

    async fn request_sampling_approval(
        &self,
        authorization_context: &McpAuthorizationContext,
        request: &SamplingRequest,
        prompt_cache_namespace: &str,
    ) -> SamplingApproval {
        let spec = McpServerSpec::new(
            request.server_id.clone(),
            request.server_id.0.clone(),
            TransportChoice::InProcess,
            McpServerSource::Dynamic {
                registered_by: "sampling".to_owned(),
            },
        )
        .with_manifest_origin(self.manifest_origin.clone());
        match authorize_mcp_sampling(
            authorization_context,
            &spec,
            request.request_id,
            request.model_id.as_deref(),
            prompt_cache_namespace,
        )
        .await
        {
            Ok(()) => SamplingApproval::Allowed,
            Err(_) => SamplingApproval::Denied,
        }
    }

    fn record_sampling(&self, outcome: McpMetricOutcome) {
        self.metrics_sink
            .record(McpMetric::SamplingRequested { outcome });
    }

    fn record_sampling_tokens(
        &self,
        server_id: &McpServerId,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        self.metrics_sink.record(McpMetric::SamplingInputTokens {
            server_id: server_id.clone(),
            amount: input_tokens,
        });
        self.metrics_sink.record(McpMetric::SamplingOutputTokens {
            server_id: server_id.clone(),
            amount: output_tokens,
        });
    }

    fn request_from_params(&self, params: Option<&Value>) -> Result<SamplingRequest, JsonRpcError> {
        let params =
            params.ok_or_else(|| invalid_params("sampling/createMessage missing params"))?;
        Ok(SamplingRequest {
            session_id: self.session_id,
            run_id: self.run_id,
            server_id: self.server_id.clone(),
            request_id: parse_optional_id(params.get("request_id"))?.unwrap_or_else(RequestId::new),
            model_id: params
                .get("model")
                .or_else(|| params.get("model_id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            input_tokens: params
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            max_output_tokens: params
                .get("max_tokens")
                .or_else(|| params.get("max_output_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            tool_rounds: params
                .get("tool_rounds")
                .and_then(Value::as_u64)
                .map(|value| value.min(u64::from(u8::MAX)) as u8)
                .unwrap_or_default(),
            requested_timeout: params
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .map(Duration::from_millis),
            permission_mode: self.permission_mode,
            server_trust: self.server_trust,
            prompt_cache_namespace: None,
            params: params.clone(),
        })
    }
}

enum SamplingApproval {
    Allowed,
    Denied,
}

enum EffectiveSamplingAllow {
    Allowed,
    RequiresApproval,
    Denied(SamplingDenyReason),
}

fn emit_sampling_event(
    request: &SamplingRequest,
    prompt_cache_namespace: &str,
    outcome: SamplingOutcome,
    event_sink: Arc<dyn McpEventSink>,
) {
    event_sink.emit(Event::McpSamplingRequested(McpSamplingRequestedEvent {
        session_id: request.session_id,
        run_id: request.run_id,
        server_id: request.server_id.clone(),
        request_id: request.request_id,
        model_id: match outcome {
            SamplingOutcome::Completed => request.model_id.clone(),
            _ => None,
        },
        input_tokens: request.input_tokens,
        output_tokens: request.max_output_tokens,
        latency_ms: 0,
        outcome,
        prompt_cache_namespace: prompt_cache_namespace.to_owned(),
        at: now(),
    }));
}

fn emit_completed_sampling_event(
    request: &SamplingRequest,
    prompt_cache_namespace: &str,
    response: &SamplingResponse,
    latency: Duration,
    event_sink: Arc<dyn McpEventSink>,
) {
    event_sink.emit(Event::McpSamplingRequested(McpSamplingRequestedEvent {
        session_id: request.session_id,
        run_id: request.run_id,
        server_id: request.server_id.clone(),
        request_id: request.request_id,
        model_id: Some(response.model_id.clone()),
        input_tokens: response.input_tokens,
        output_tokens: response.output_tokens,
        latency_ms: latency.as_millis().try_into().unwrap_or(u64::MAX),
        outcome: SamplingOutcome::Completed,
        prompt_cache_namespace: prompt_cache_namespace.to_owned(),
        at: now(),
    }));
}

fn parse_optional_id<T>(value: Option<&Value>) -> Result<Option<T>, JsonRpcError>
where
    T: serde::de::DeserializeOwned,
{
    value
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| invalid_params(format!("invalid id: {error}")))
}

fn invalid_params(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: JSONRPC_INVALID_PARAMS,
        message: message.into(),
        data: None,
    }
}
