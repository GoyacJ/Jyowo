use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    BudgetMetric, DecisionScope, DeferPolicy, Event, ManifestOriginRef, McpOrigin, McpServerId,
    McpServerSource, NetworkAccess, OverflowAction, PermissionSubject, ProviderRestriction,
    ResultBudget, SemverString, ToolActionPlan, ToolDescriptor, ToolDescriptorMetadata, ToolError,
    ToolExecutionChannel, ToolGroup, ToolIntegrationSource, ToolOrigin, ToolProperties, ToolResult,
    ToolResultPart, ToolUseHeartbeatEvent, TrustLevel, WorkspaceAccess,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, PermissionCheck, Tool, ToolContext,
    ToolEvent, ToolProgress, ToolStream, ValidationError,
};
use serde_json::Value;

use crate::{
    McpConnection, McpContent, McpError, McpMetric, McpMetricOutcome, McpMetricsSink,
    McpToolAnnotations, McpToolCallEvent, McpToolDescriptor, McpToolResult, NoopMcpMetricsSink,
};

#[derive(Clone)]
pub struct McpToolWrapper {
    descriptor: ToolDescriptor,
    upstream_name: String,
    connection: Arc<dyn McpConnection>,
    server_id: McpServerId,
    origin: ManifestOriginRef,
    metrics_sink: Arc<dyn McpMetricsSink>,
    cancel_ack_timeout: Duration,
}

impl McpToolWrapper {
    pub fn new(
        server_id: McpServerId,
        server_source: McpServerSource,
        origin: ManifestOriginRef,
        server_trust: TrustLevel,
        mcp_tool: McpToolDescriptor,
        connection: Arc<dyn McpConnection>,
        defer_policy: DeferPolicy,
        canonical_name: String,
    ) -> Self {
        Self::new_with_metrics(
            server_id,
            server_source,
            origin,
            server_trust,
            mcp_tool,
            connection,
            defer_policy,
            canonical_name,
            Arc::new(NoopMcpMetricsSink),
        )
    }

    pub fn new_with_metrics(
        server_id: McpServerId,
        server_source: McpServerSource,
        origin: ManifestOriginRef,
        server_trust: TrustLevel,
        mcp_tool: McpToolDescriptor,
        connection: Arc<dyn McpConnection>,
        defer_policy: DeferPolicy,
        canonical_name: String,
        metrics_sink: Arc<dyn McpMetricsSink>,
    ) -> Self {
        Self::new_with_metrics_and_cancel_ack_timeout(
            server_id,
            server_source,
            origin,
            server_trust,
            mcp_tool,
            connection,
            defer_policy,
            canonical_name,
            metrics_sink,
            crate::McpTimeouts::default().cancel_ack,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_metrics_and_cancel_ack_timeout(
        server_id: McpServerId,
        server_source: McpServerSource,
        origin: ManifestOriginRef,
        server_trust: TrustLevel,
        mcp_tool: McpToolDescriptor,
        connection: Arc<dyn McpConnection>,
        defer_policy: DeferPolicy,
        canonical_name: String,
        metrics_sink: Arc<dyn McpMetricsSink>,
        cancel_ack_timeout: Duration,
    ) -> Self {
        let upstream_name = mcp_tool.name.clone();
        let description = mcp_tool
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP tool {upstream_name}"));
        let properties = tool_properties_from_annotations(
            mcp_tool.annotations.as_ref(),
            server_trust,
            defer_policy,
        );
        let mut server_meta = mcp_tool.meta;
        if let Some(open_world_hint) = mcp_tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.open_world_hint)
        {
            server_meta.insert("openWorldHint".to_owned(), Value::Bool(open_world_hint));
        }
        let descriptor = ToolDescriptor {
            name: canonical_name,
            display_name: upstream_name.clone(),
            description: description.clone(),
            category: "mcp".to_owned(),
            group: ToolGroup::Network,
            version: SemverString::from("0.1.0"),
            input_schema: mcp_tool.input_schema,
            output_schema: mcp_tool.output_schema,
            dynamic_schema: false,
            properties,
            trust_level: server_trust,
            required_capabilities: Vec::new(),
            budget: ResultBudget {
                metric: BudgetMetric::Chars,
                limit: 64_000,
                on_overflow: OverflowAction::Truncate,
                preview_head_chars: 4_000,
                preview_tail_chars: 1_000,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Mcp(McpOrigin {
                server_id: server_id.clone(),
                upstream_name: upstream_name.clone(),
                server_meta,
                server_source,
                server_trust,
            }),
            search_hint: Some(description),
            service_binding: None,
            metadata: ToolDescriptorMetadata {
                integration_source: ToolIntegrationSource::Mcp,
                ..Default::default()
            },
        };

        Self {
            descriptor,
            upstream_name,
            connection,
            server_id,
            origin,
            metrics_sink,
            cancel_ack_timeout,
        }
    }

    pub fn upstream_name(&self) -> &str {
        &self.upstream_name
    }
}

fn tool_properties_from_annotations(
    annotations: Option<&McpToolAnnotations>,
    server_trust: TrustLevel,
    defer_policy: DeferPolicy,
) -> ToolProperties {
    let Some(annotations) = annotations else {
        return fail_closed_tool_properties(defer_policy);
    };
    if server_trust != TrustLevel::AdminTrusted {
        return fail_closed_tool_properties(defer_policy);
    }

    let destructive = annotations.destructive_hint.unwrap_or(true);
    let read_only = annotations.read_only_hint.unwrap_or(false) && !destructive;
    let concurrency_safe = read_only && !destructive && annotations.idempotent_hint == Some(true);

    ToolProperties {
        is_concurrency_safe: concurrency_safe,
        is_read_only: read_only,
        is_destructive: destructive,
        long_running: None,
        defer_policy,
    }
}

fn fail_closed_tool_properties(defer_policy: DeferPolicy) -> ToolProperties {
    ToolProperties {
        is_concurrency_safe: false,
        is_read_only: false,
        is_destructive: true,
        long_running: None,
        defer_policy,
    }
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if !input.is_object() {
            return Err(ValidationError::from(
                "mcp tool input must be a JSON object",
            ));
        }
        validate_input_schema(&self.descriptor.input_schema, input)
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::McpToolCall {
                    server: self.server_id.0.clone(),
                    tool: self.upstream_name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            vec![crate::mcp_tool_resource(
                &self.server_id,
                &self.origin,
                &self.upstream_name,
            )],
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let mut upstream = self
            .connection
            .call_tool_events(&self.upstream_name, input)
            .await
            .map_err(|error| {
                self.record_invocation(McpMetricOutcome::Error);
                to_tool_error(error)
            })?;
        let tool_use_id = ctx.tool_use_id;
        let run_id = ctx.run_id;
        let metrics_sink = Arc::clone(&self.metrics_sink);
        let server_id = self.server_id.clone();
        let connection = Arc::clone(&self.connection);
        let request_id = ctx.tool_use_id.to_string();
        let interrupt = ctx.interrupt.clone();
        let cancel_ack_timeout = self.cancel_ack_timeout;

        Ok(Box::pin(async_stream::stream! {
            loop {
                let event = tokio::select! {
                    event = upstream.next() => event,
                    () = tokio::time::sleep(Duration::from_millis(10)), if interrupt.is_interrupted() => {
                        let _ = connection
                            .cancel_tool_call(&request_id, Some("harness interrupted tool call".to_owned()))
                            .await;
                        match tokio::time::timeout(cancel_ack_timeout, upstream.next()).await {
                            Ok(event) => event,
                            Err(_) => {
                                let _ = connection
                                    .mark_unhealthy("mcp tool cancellation acknowledgement timed out".to_owned())
                                    .await;
                                record_invocation(&metrics_sink, &server_id, McpMetricOutcome::Cancelled);
                                yield ToolEvent::Error(ToolError::Interrupted);
                                break;
                            },
                        }
                    },
                };
                let Some(event) = event else {
                    break;
                };
                match event {
                    McpToolCallEvent::Progress {
                        progress,
                        total,
                        message,
                        ..
                    } => {
                        let message = message.unwrap_or_else(|| "mcp tool running".to_owned());
                        let fraction = progress_fraction(progress, total);
                        let progress_event = ToolProgress {
                            message: message.clone(),
                            fraction,
                            at: chrono::Utc::now(),
                        };
                        yield ToolEvent::Progress(progress_event.clone());
                        yield ToolEvent::Journal(Event::ToolUseHeartbeat(ToolUseHeartbeatEvent {
                            tool_use_id,
                            run_id,
                            message,
                            fraction,
                            silent_for_ms: 0,
                            at: progress_event.at,
                        }));
                    },
                    McpToolCallEvent::Cancelled { .. } => {
                        record_invocation(&metrics_sink, &server_id, McpMetricOutcome::Cancelled);
                        yield ToolEvent::Error(ToolError::Interrupted);
                        break;
                    },
                    McpToolCallEvent::Final(result) => {
                        if result.is_error {
                            record_invocation(&metrics_sink, &server_id, McpMetricOutcome::Error);
                            yield ToolEvent::Error(ToolError::Message(result_error_message(&result)));
                        } else {
                            record_invocation(&metrics_sink, &server_id, McpMetricOutcome::Success);
                            yield ToolEvent::Final(into_tool_result(result));
                        }
                        break;
                    },
                    McpToolCallEvent::Error(error) => {
                        record_invocation(&metrics_sink, &server_id, McpMetricOutcome::Error);
                        yield ToolEvent::Error(to_tool_error(error));
                        break;
                    },
                }
            }
        }))
    }
}

impl McpToolWrapper {
    fn record_invocation(&self, outcome: McpMetricOutcome) {
        record_invocation(&self.metrics_sink, &self.server_id, outcome);
    }
}

fn record_invocation(
    metrics_sink: &Arc<dyn McpMetricsSink>,
    server_id: &McpServerId,
    outcome: McpMetricOutcome,
) {
    metrics_sink.record(McpMetric::ToolInvocation {
        server_id: server_id.clone(),
        outcome,
    });
}

fn to_tool_error(error: McpError) -> ToolError {
    ToolError::Message(error.to_string())
}

fn into_tool_result(result: McpToolResult) -> ToolResult {
    let mut content = result.content;
    if content.len() == 1 {
        return match content.remove(0) {
            McpContent::Text { text } => ToolResult::Text(text),
            McpContent::Json { value } => ToolResult::Structured(value),
        };
    }

    ToolResult::Mixed(
        content
            .into_iter()
            .map(|part| match part {
                McpContent::Text { text } => ToolResultPart::Text { text },
                McpContent::Json { value } => ToolResultPart::Structured {
                    value,
                    schema_ref: None,
                },
            })
            .collect(),
    )
}

fn result_error_message(result: &McpToolResult) -> String {
    result
        .content
        .iter()
        .find_map(|content| match content {
            McpContent::Text { text } => Some(text.clone()),
            McpContent::Json { .. } => None,
        })
        .unwrap_or_else(|| "mcp tool returned an error".to_owned())
}

fn progress_fraction(progress: Option<f64>, total: Option<f64>) -> Option<f32> {
    let progress = progress?;
    let total = total?;
    if !progress.is_finite() || !total.is_finite() || total <= 0.0 {
        return None;
    }
    Some((progress / total).clamp(0.0, 1.0) as f32)
}

fn validate_input_schema(schema: &Value, input: &Value) -> Result<(), ValidationError> {
    let validator = jsonschema::validator_for(schema).map_err(|error| {
        ValidationError::from(format!("failed to compile mcp tool input schema: {error}"))
    })?;
    if validator.is_valid(input) {
        return Ok(());
    }
    let details = validator.iter_errors(input).next().map_or_else(
        || "mcp tool input does not match input schema".to_owned(),
        |error| error.to_string(),
    );
    Err(ValidationError::from(format!(
        "mcp tool input schema validation failed: {details}"
    )))
}
