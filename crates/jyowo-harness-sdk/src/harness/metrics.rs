use super::*;

impl Harness {
    pub(super) fn model_metrics_sink(&self) -> Option<Arc<dyn ModelMetricsSink>> {
        self.inner.observer.as_ref().map(|observer| {
            Arc::new(SdkModelMetricsSink {
                observer: Arc::clone(observer),
            }) as Arc<dyn ModelMetricsSink>
        })
    }

    #[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
    pub(super) fn memory_metrics_sink(&self) -> Option<Arc<dyn harness_memory::MemoryMetricsSink>> {
        self.inner.observer.as_ref().map(|observer| {
            Arc::new(SdkMemoryMetricsSink {
                observer: Arc::clone(observer),
            }) as Arc<dyn harness_memory::MemoryMetricsSink>
        })
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
struct SdkMemoryMetricsSink {
    observer: Arc<Observer>,
}

struct SdkModelMetricsSink {
    observer: Arc<Observer>,
}

impl ModelMetricsSink for SdkModelMetricsSink {
    fn record_credential_pool_cooldown(&self, model_id: &str) {
        self.observer
            .model_metrics
            .record_credential_pool_cooldown(model_id);
    }

    fn record_aux_queue_wait(&self, model_id: &str, duration: Duration) {
        self.observer
            .model_metrics
            .record_aux_queue_wait(model_id, duration);
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
impl harness_memory::MemoryMetricsSink for SdkMemoryMetricsSink {
    fn record(&self, metric: harness_memory::MemoryMetric) {
        let (name, attrs) = self.attributes(metric);
        let mut span = self.observer.start_span(name, attrs);
        span.set_status(SpanStatus::Ok);
        span.end();
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
impl SdkMemoryMetricsSink {
    fn attributes(&self, metric: harness_memory::MemoryMetric) -> (&'static str, SpanAttributes) {
        match metric {
            harness_memory::MemoryMetric::Recall {
                provider_id,
                outcome,
                duration_ms,
                returned_count,
            } => {
                let mut attrs = SpanAttributes::new()
                    .with(
                        "outcome",
                        AttributeValue::String(memory_recall_outcome(outcome).to_owned()),
                    )
                    .with(
                        "duration_ms",
                        AttributeValue::Int(u64_to_i64(duration_ms.into())),
                    )
                    .with(
                        "returned_count",
                        AttributeValue::Int(u64_to_i64(returned_count.into())),
                    );
                if let Some(provider_id) = provider_id {
                    attrs = attrs.with("provider_id", AttributeValue::String(provider_id));
                }
                ("memory.recall", attrs)
            }
            harness_memory::MemoryMetric::RecallDegraded {
                provider_id,
                reason,
            } => {
                let mut attrs = SpanAttributes::new().with(
                    "reason",
                    AttributeValue::String(self.redact_reason(&reason)),
                );
                if let Some(provider_id) = provider_id {
                    attrs = attrs.with("provider_id", AttributeValue::String(provider_id));
                }
                ("memory.recall.degraded", attrs)
            }
            harness_memory::MemoryMetric::RecallHitRateSample { provider_id, hit } => {
                let mut attrs = SpanAttributes::new().with("hit", AttributeValue::Bool(hit));
                if let Some(provider_id) = provider_id {
                    attrs = attrs.with("provider_id", AttributeValue::String(provider_id));
                }
                ("memory.recall.hit_rate", attrs)
            }
            harness_memory::MemoryMetric::ThreatDetected { category, action } => (
                "memory.threat.detected",
                SpanAttributes::new()
                    .with(
                        "category",
                        AttributeValue::String(threat_category(category).to_owned()),
                    )
                    .with(
                        "action",
                        AttributeValue::String(threat_action(action).to_owned()),
                    ),
            ),
            harness_memory::MemoryMetric::MemdirWrite {
                file,
                action,
                bytes_written,
            } => (
                "memory.memdir.write",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with(
                        "action",
                        AttributeValue::String(memory_write_action(&action).to_owned()),
                    )
                    .with(
                        "bytes_written",
                        AttributeValue::Int(u64_to_i64(bytes_written)),
                    ),
            ),
            harness_memory::MemoryMetric::MemdirBytes { file, bytes } => (
                "memory.memdir.bytes",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with("bytes", AttributeValue::Int(u64_to_i64(bytes))),
            ),
            harness_memory::MemoryMetric::MemdirOverflow {
                file,
                current_chars,
                threshold,
            } => (
                "memory.memdir.overflow",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with(
                        "current_chars",
                        AttributeValue::Int(u64_to_i64(current_chars)),
                    )
                    .with("threshold", AttributeValue::Int(u64_to_i64(threshold))),
            ),
            harness_memory::MemoryMetric::MemdirLockWait { file, waited_ms } => (
                "memory.memdir.lock_wait",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with(
                        "waited_ms",
                        AttributeValue::Int(u64_to_i64(waited_ms.into())),
                    ),
            ),
            harness_memory::MemoryMetric::MemdirLockFailed { file, retries } => (
                "memory.memdir.lock_failed",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with("retries", AttributeValue::Int(u64_to_i64(retries.into()))),
            ),
            #[cfg(feature = "memory-consolidation")]
            harness_memory::MemoryMetric::ConsolidationRan {
                hook_id,
                promoted,
                demoted,
            } => (
                "memory.consolidation.ran",
                SpanAttributes::new()
                    .with("hook_id", AttributeValue::String(hook_id))
                    .with("promoted", AttributeValue::Int(u64_to_i64(promoted.into())))
                    .with("demoted", AttributeValue::Int(u64_to_i64(demoted.into()))),
            ),
            harness_memory::MemoryMetric::ExternalProviderConfigured { configured } => (
                "memory.external.configured",
                SpanAttributes::new().with("configured", AttributeValue::Bool(configured)),
            ),
            harness_memory::MemoryMetric::Upsert { kind, visibility } => (
                "memory.upsert",
                SpanAttributes::new()
                    .with(
                        "kind",
                        AttributeValue::String(memory_kind(&kind).to_owned()),
                    )
                    .with(
                        "visibility",
                        AttributeValue::String(memory_visibility(&visibility).to_owned()),
                    ),
            ),
        }
    }

    fn redact_reason(&self, reason: &str) -> String {
        let redacted = self.observer.redactor.redact(
            reason,
            &RedactRules {
                scope: RedactScope::TraceOnly,
                replacement: "[REDACTED]".to_owned(),
                pattern_set: RedactPatternSet::Default,
            },
        );
        truncate_chars(&redacted, 160)
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_recall_outcome(outcome: harness_memory::MemoryRecallMetricOutcome) -> &'static str {
    match outcome {
        harness_memory::MemoryRecallMetricOutcome::Recalled => "recalled",
        harness_memory::MemoryRecallMetricOutcome::Empty => "empty",
        harness_memory::MemoryRecallMetricOutcome::Skipped => "skipped",
        harness_memory::MemoryRecallMetricOutcome::Degraded => "degraded",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memdir_file(file: MemdirFileTag) -> &'static str {
    match file {
        MemdirFileTag::Memory => "memory",
        MemdirFileTag::User => "user",
        MemdirFileTag::Dreams => "dreams",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_write_action(action: &harness_contracts::MemoryWriteAction) -> &'static str {
    match action {
        harness_contracts::MemoryWriteAction::AppendSection { .. } => "append_section",
        harness_contracts::MemoryWriteAction::ReplaceSection { .. } => "replace_section",
        harness_contracts::MemoryWriteAction::DeleteSection { .. } => "delete_section",
        harness_contracts::MemoryWriteAction::Upsert => "upsert",
        harness_contracts::MemoryWriteAction::Forget => "forget",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_kind(kind: &harness_contracts::MemoryKind) -> &'static str {
    match kind {
        harness_contracts::MemoryKind::UserPreference => "user_preference",
        harness_contracts::MemoryKind::Feedback => "feedback",
        harness_contracts::MemoryKind::ProjectFact => "project_fact",
        harness_contracts::MemoryKind::Reference => "reference",
        harness_contracts::MemoryKind::AgentSelfNote => "agent_self_note",
        harness_contracts::MemoryKind::Custom(_) => "custom",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_visibility(visibility: &harness_contracts::MemoryVisibility) -> &'static str {
    match visibility {
        harness_contracts::MemoryVisibility::Private { .. } => "private",
        harness_contracts::MemoryVisibility::User { .. } => "user",
        harness_contracts::MemoryVisibility::Team { .. } => "team",
        harness_contracts::MemoryVisibility::Tenant => "tenant",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn threat_category(category: harness_contracts::ThreatCategory) -> &'static str {
    match category {
        harness_contracts::ThreatCategory::PromptInjection => "prompt_injection",
        harness_contracts::ThreatCategory::Exfiltration => "exfiltration",
        harness_contracts::ThreatCategory::Backdoor => "backdoor",
        harness_contracts::ThreatCategory::Credential => "credential",
        harness_contracts::ThreatCategory::Malicious => "malicious",
        harness_contracts::ThreatCategory::SpecialToken => "special_token",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn threat_action(action: harness_contracts::ThreatAction) -> &'static str {
    match action {
        harness_contracts::ThreatAction::Warn => "warn",
        harness_contracts::ThreatAction::Redact => "redact",
        harness_contracts::ThreatAction::Block => "block",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(super) struct SdkMcpEventSink {
    pub(super) event_store: Arc<dyn EventStore>,
    pub(super) tenant_id: TenantId,
    pub(super) session_id: harness_contracts::SessionId,
}

impl McpEventSink for SdkMcpEventSink {
    fn emit(&self, event: Event) {
        let event_store = Arc::clone(&self.event_store);
        let tenant_id = self.tenant_id;
        let session_id = self.session_id;
        std::thread::spawn(move || {
            futures::executor::block_on(async move {
                let _ = event_store.append(tenant_id, session_id, &[event]).await;
            });
        });
    }
}

pub(super) struct SdkMcpMetricsSink {
    pub(super) observer: Arc<Observer>,
}

impl McpMetricsSink for SdkMcpMetricsSink {
    fn record(&self, metric: McpMetric) {
        let (name, attrs) = mcp_metric_attributes(metric);
        let mut span = self.observer.start_span(name, attrs);
        span.set_status(SpanStatus::Ok);
        span.end();
    }
}

fn mcp_metric_attributes(metric: McpMetric) -> (&'static str, SpanAttributes) {
    match metric {
        McpMetric::OAuthRefresh { outcome } => (
            "mcp.oauth.refresh",
            SpanAttributes::new().with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ConnectionTotal {
            server_id,
            transport,
            outcome,
        } => (
            "mcp.connection.total",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("transport", string_attr_value(transport))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ConnectionState { server_id, state } => (
            "mcp.connection.state",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("state", string_attr_value(mcp_state_label(state))),
        ),
        McpMetric::ReconnectAttempt {
            server_id,
            attempt,
            outcome,
        } => (
            "mcp.reconnect.attempt",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("attempt", AttributeValue::Int(i64::from(attempt)))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ToolInvocation { server_id, outcome } => (
            "mcp.tool.invocation",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ToolFilterSkipped { server_id, reason } => (
            "mcp.tool_filter.skipped",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("reason", string_attr_value(reason)),
        ),
        McpMetric::ListChanged {
            server_id,
            disposition,
        } => (
            "mcp.list.changed",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("disposition", string_attr_value(format!("{disposition:?}"))),
        ),
        McpMetric::ResourceUpdated { server_id, kind } => (
            "mcp.resource.updated",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("kind", string_attr_value(format!("{kind:?}"))),
        ),
        McpMetric::SamplingRequested { outcome } => (
            "mcp.sampling.requested",
            SpanAttributes::new().with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::SamplingInputTokens { server_id, amount } => (
            "mcp.sampling.input_tokens",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with(
                    "amount",
                    AttributeValue::Int(amount.try_into().unwrap_or(i64::MAX)),
                ),
        ),
        McpMetric::SamplingOutputTokens { server_id, amount } => (
            "mcp.sampling.output_tokens",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with(
                    "amount",
                    AttributeValue::Int(amount.try_into().unwrap_or(i64::MAX)),
                ),
        ),
        McpMetric::ServerRequest { method, outcome } => (
            "mcp.server.request",
            SpanAttributes::new()
                .with("method", string_attr_value(method))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ServerThrottled { capability } => (
            "mcp.server.throttled",
            SpanAttributes::new().with("capability", string_attr_value(capability)),
        ),
        McpMetric::ServerTenantIsolationRejected => (
            "mcp.server.tenant_isolation.rejected",
            SpanAttributes::new(),
        ),
    }
}

fn string_attr_value(value: impl Into<String>) -> AttributeValue {
    AttributeValue::String(value.into())
}

fn mcp_state_label(state: McpMetricConnectionState) -> &'static str {
    match state {
        McpMetricConnectionState::Connecting => "connecting",
        McpMetricConnectionState::Ready => "ready",
        McpMetricConnectionState::Reconnecting => "reconnecting",
        McpMetricConnectionState::Failed => "failed",
        McpMetricConnectionState::Closed => "closed",
    }
}
