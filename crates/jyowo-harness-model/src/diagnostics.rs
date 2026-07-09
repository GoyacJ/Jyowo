//! Provider connectivity probe runner for model settings diagnostics.

use std::time::{Duration, Instant};

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    Message, MessageId, MessagePart, MessageRole, ModelError, ModelProtocol,
    ProviderProbeErrorKind, ProviderProbeSnapshot, ProviderProbeStatus, RequestId, TenantId,
    UsageSnapshot,
};
use tokio::time::timeout;

use crate::{ErrorClass, InferContext, ModelProvider, ModelRequest, ModelStreamEvent};

const PROBE_PROMPT: &str = "Respond with OK.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProbeInput {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderProbeOutcome {
    pub snapshot: ProviderProbeSnapshot,
    pub diagnostic_usage: Option<UsageSnapshot>,
}

pub struct ProviderProbeRunner;

impl ProviderProbeRunner {
    pub async fn run(
        provider: &dyn ModelProvider,
        input: ProviderProbeInput,
        protocol: ModelProtocol,
    ) -> ProviderProbeOutcome {
        let started = Instant::now();
        let checked_at = Utc::now();
        let timeout_duration = Duration::from_millis(input.timeout_ms);
        let deadline = started + timeout_duration;

        let request = ModelRequest {
            model_id: input.model_id.clone(),
            messages: vec![Message {
                id: MessageId::new(),
                role: MessageRole::User,
                parts: vec![MessagePart::Text(PROBE_PROMPT.to_owned())],
                created_at: checked_at,
            }],
            tools: None,
            system: None,
            temperature: Some(0.0),
            max_tokens: Some(8),
            stream: true,
            cache_breakpoints: Vec::new(),
            protocol,
            extra: serde_json::Value::Null,
            options: harness_contracts::ModelRequestOptions::default(),
            provider_context: crate::ProviderRequestContext::default(),
        };

        let mut ctx = InferContext::for_test();
        ctx.request_id = RequestId::new();
        ctx.tenant_id = TenantId::SINGLE;
        ctx.deadline = Some(deadline);
        ctx.suppress_usage_accounting = true;

        let probe_result = timeout(timeout_duration, async {
            let stream = provider.infer(request, ctx).await;
            match stream {
                Ok(mut stream) => {
                    let mut diagnostic_usage = UsageSnapshot::default();
                    while let Some(event) = stream.next().await {
                        match event {
                            ModelStreamEvent::MessageStart { usage, .. } => {
                                merge_usage(&mut diagnostic_usage, &usage);
                            }
                            ModelStreamEvent::MessageDelta { usage_delta, .. } => {
                                merge_usage(&mut diagnostic_usage, &usage_delta);
                            }
                            ModelStreamEvent::MessageStop => {
                                return Ok(Some(diagnostic_usage));
                            }
                            ModelStreamEvent::StreamError { class, .. } => {
                                return Err(classify_stream_error(class));
                            }
                            _ => {}
                        }
                    }
                    Err(classify_probe_failure(
                        ProviderProbeStatus::Failed,
                        ProviderProbeErrorKind::Provider,
                        "Connectivity probe ended before completion.",
                    ))
                }
                Err(error) => Err(map_model_error(error)),
            }
        })
        .await;

        let latency_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

        match probe_result {
            Ok(Ok(Some(usage))) => {
                let diagnostic_usage = usage_has_tokens(&usage).then_some(usage);
                ProviderProbeOutcome {
                    snapshot: ProviderProbeSnapshot {
                        config_id: input.config_id,
                        provider_id: input.provider_id,
                        model_id: input.model_id,
                        status: ProviderProbeStatus::Online,
                        timeout_ms: input.timeout_ms,
                        latency_ms: Some(latency_ms),
                        checked_at,
                        error_kind: None,
                        safe_message: None,
                    },
                    diagnostic_usage,
                }
            }
            Ok(Ok(None)) => ProviderProbeOutcome {
                snapshot: success_snapshot(&input, input.timeout_ms, Some(latency_ms), checked_at),
                diagnostic_usage: None,
            },
            Ok(Err(failure)) => ProviderProbeOutcome {
                snapshot: failure.snapshot(&input, input.timeout_ms, Some(latency_ms), checked_at),
                diagnostic_usage: None,
            },
            Err(_) => ProviderProbeOutcome {
                snapshot: failure_snapshot(
                    &input,
                    input.timeout_ms,
                    Some(latency_ms),
                    checked_at,
                    ProviderProbeStatus::Timeout,
                    ProviderProbeErrorKind::Timeout,
                    "Connectivity probe timed out.",
                ),
                diagnostic_usage: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeFailure {
    status: ProviderProbeStatus,
    error_kind: ProviderProbeErrorKind,
    safe_message: String,
}

impl ProbeFailure {
    fn snapshot(
        &self,
        input: &ProviderProbeInput,
        timeout_ms: u64,
        latency_ms: Option<u64>,
        checked_at: chrono::DateTime<Utc>,
    ) -> ProviderProbeSnapshot {
        failure_snapshot(
            input,
            timeout_ms,
            latency_ms,
            checked_at,
            self.status,
            self.error_kind,
            &self.safe_message,
        )
    }
}

fn classify_probe_failure(
    status: ProviderProbeStatus,
    error_kind: ProviderProbeErrorKind,
    safe_message: impl Into<String>,
) -> ProbeFailure {
    ProbeFailure {
        status,
        error_kind,
        safe_message: safe_message.into(),
    }
}

fn classify_stream_error(class: ErrorClass) -> ProbeFailure {
    match class {
        ErrorClass::AuthExpired => classify_probe_failure(
            ProviderProbeStatus::Unauthenticated,
            ProviderProbeErrorKind::Auth,
            "Provider authentication failed.",
        ),
        ErrorClass::RateLimited { .. } => classify_probe_failure(
            ProviderProbeStatus::RateLimited,
            ProviderProbeErrorKind::RateLimit,
            "Provider rate limit reached.",
        ),
        ErrorClass::Transient => classify_probe_failure(
            ProviderProbeStatus::Failed,
            ProviderProbeErrorKind::Network,
            "Connectivity probe failed.",
        ),
        ErrorClass::ContextOverflow => classify_probe_failure(
            ProviderProbeStatus::Failed,
            ProviderProbeErrorKind::Provider,
            "Connectivity probe failed.",
        ),
        ErrorClass::Fatal => classify_probe_failure(
            ProviderProbeStatus::Failed,
            ProviderProbeErrorKind::Provider,
            "Connectivity probe failed.",
        ),
    }
}

fn map_model_error(error: ModelError) -> ProbeFailure {
    match error {
        ModelError::DeadlineExceeded(_) => classify_probe_failure(
            ProviderProbeStatus::Timeout,
            ProviderProbeErrorKind::Timeout,
            "Connectivity probe timed out.",
        ),
        ModelError::AuthExpired(_) => classify_probe_failure(
            ProviderProbeStatus::Unauthenticated,
            ProviderProbeErrorKind::Auth,
            "Provider authentication failed.",
        ),
        ModelError::RateLimited(_) => classify_probe_failure(
            ProviderProbeStatus::RateLimited,
            ProviderProbeErrorKind::RateLimit,
            "Provider rate limit reached.",
        ),
        ModelError::InvalidRequest(_) => classify_probe_failure(
            ProviderProbeStatus::Failed,
            ProviderProbeErrorKind::InvalidConfig,
            "Provider configuration is invalid.",
        ),
        ModelError::Cancelled => classify_probe_failure(
            ProviderProbeStatus::Failed,
            ProviderProbeErrorKind::Unknown,
            "Connectivity probe was cancelled.",
        ),
        _ => classify_probe_failure(
            ProviderProbeStatus::Failed,
            ProviderProbeErrorKind::Unknown,
            "Connectivity probe failed.",
        ),
    }
}

fn failure_snapshot(
    input: &ProviderProbeInput,
    timeout_ms: u64,
    latency_ms: Option<u64>,
    checked_at: chrono::DateTime<Utc>,
    status: ProviderProbeStatus,
    error_kind: ProviderProbeErrorKind,
    safe_message: &str,
) -> ProviderProbeSnapshot {
    ProviderProbeSnapshot {
        config_id: input.config_id.clone(),
        provider_id: input.provider_id.clone(),
        model_id: input.model_id.clone(),
        status,
        timeout_ms,
        latency_ms,
        checked_at,
        error_kind: Some(error_kind),
        safe_message: Some(safe_message.to_owned()),
    }
}

fn success_snapshot(
    input: &ProviderProbeInput,
    timeout_ms: u64,
    latency_ms: Option<u64>,
    checked_at: chrono::DateTime<Utc>,
) -> ProviderProbeSnapshot {
    ProviderProbeSnapshot {
        config_id: input.config_id.clone(),
        provider_id: input.provider_id.clone(),
        model_id: input.model_id.clone(),
        status: ProviderProbeStatus::Online,
        timeout_ms,
        latency_ms,
        checked_at,
        error_kind: None,
        safe_message: None,
    }
}

fn merge_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
    total.tool_calls = total.tool_calls.saturating_add(delta.tool_calls);
}

fn usage_has_tokens(usage: &UsageSnapshot) -> bool {
    usage.input_tokens > 0
        || usage.output_tokens > 0
        || usage.cache_read_tokens > 0
        || usage.cache_write_tokens > 0
        || usage.cost_micros > 0
        || usage.tool_calls > 0
}
