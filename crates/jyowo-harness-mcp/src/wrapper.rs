use std::{
    collections::BTreeMap,
    io::{self, Write},
    sync::Arc,
    time::Duration,
};

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
use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
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
        let display_name = mcp_tool
            .title
            .clone()
            .or_else(|| {
                mcp_tool
                    .annotations
                    .as_ref()
                    .and_then(|annotations| annotations.title.clone())
            })
            .unwrap_or_else(|| upstream_name.clone());
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
        if let Some(icons) = mcp_tool.icons {
            server_meta.insert(
                "icons".to_owned(),
                serde_json::to_value(icons).expect("MCP icons serialize"),
            );
        }
        if let Some(execution) = mcp_tool.execution {
            server_meta.insert(
                "execution".to_owned(),
                serde_json::to_value(execution).expect("MCP tool execution serializes"),
            );
        }
        if let Some(open_world_hint) = mcp_tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.open_world_hint)
        {
            server_meta.insert("openWorldHint".to_owned(), Value::Bool(open_world_hint));
        }
        let descriptor = ToolDescriptor {
            name: canonical_name,
            display_name,
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
        let tool_use_id = ctx.tool_use_id;
        let client_request_id = tool_use_id.to_string();
        let mut upstream = self
            .connection
            .call_tool_events_for_request(&client_request_id, &self.upstream_name, input)
            .await
            .map_err(|error| {
                self.record_invocation(McpMetricOutcome::Error);
                to_tool_error(error)
            })?;
        let run_id = ctx.run_id;
        let metrics_sink = Arc::clone(&self.metrics_sink);
        let server_id = self.server_id.clone();
        let connection = Arc::clone(&self.connection);
        let request_id = client_request_id;
        let interrupt = ctx.interrupt.clone();
        let cancel_ack_timeout = self.cancel_ack_timeout;

        Ok(Box::pin(async_stream::stream! {
            loop {
                let event = if interrupt.is_interrupted() {
                    let cancel_and_ack = async {
                        let _ = connection
                            .cancel_tool_call(&request_id, Some("harness interrupted tool call".to_owned()))
                            .await;
                        loop {
                            let event = upstream.next().await;
                            if matches!(event, Some(McpToolCallEvent::Progress { .. })) {
                                continue;
                            }
                            break event;
                        }
                    };
                    match tokio::time::timeout(cancel_ack_timeout, cancel_and_ack).await {
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
                } else {
                    tokio::select! {
                        event = upstream.next() => event,
                        () = tokio::time::sleep(Duration::from_millis(10)) => continue,
                    }
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
    // Harness ToolResult has no non-model metadata channel. MCP `_meta` is
    // protocol-private, so it must not be projected into model-visible output.
    let mut parts = Vec::new();
    for content in result.content {
        match content {
            McpContent::Text {
                text,
                annotations,
                meta: _,
            } => {
                parts.push(ToolResultPart::Text { text: text.clone() });
                if annotations.is_some() {
                    parts.push(ToolResultPart::Structured {
                        value: serde_json::to_value(ModelVisibleContent(&McpContent::Text {
                            text,
                            annotations,
                            meta: BTreeMap::new(),
                        }))
                        .expect("MCP text content serializes"),
                        schema_ref: None,
                    });
                }
            }
            other => parts.push(ToolResultPart::Structured {
                value: serde_json::to_value(ModelVisibleContent(&other))
                    .expect("MCP content serializes"),
                schema_ref: None,
            }),
        }
    }
    if let Some(value) = result.structured_content {
        parts.push(ToolResultPart::Structured {
            value: Value::Object(value),
            schema_ref: None,
        });
    }
    match parts.as_slice() {
        [ToolResultPart::Text { text }] => ToolResult::Text(text.clone()),
        [ToolResultPart::Structured { value, .. }] => ToolResult::Structured(value.clone()),
        _ => ToolResult::Mixed(parts),
    }
}

fn result_error_message(result: &McpToolResult) -> String {
    const MAX_ERROR_MESSAGE_BYTES: usize = 16 * 1024;
    const MAX_SUMMARY_BYTES: usize = 2 * 1024;
    const DETAILS_PREFIX: &str = "\nMCP error details: ";

    let summary = result
        .content
        .iter()
        .find_map(|content| match content {
            McpContent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("mcp tool returned an error");
    let summary = truncate_utf8(summary, MAX_SUMMARY_BYTES, " [truncated]");
    let details_value = McpToolErrorDetails {
        content: ModelVisibleContents(&result.content),
        structured_content: result.structured_content.as_ref(),
    };
    let details_budget = MAX_ERROR_MESSAGE_BYTES
        .saturating_sub(summary.len())
        .saturating_sub(DETAILS_PREFIX.len());
    let (details, details_truncated) = json_prefix(&details_value, details_budget);

    if !details_truncated {
        return format!("{summary}{DETAILS_PREFIX}{details}");
    }

    // A JSON prefix can at most double when encoded as a JSON string because quotes and
    // backslashes are escaped. Split the remaining budget across the three protocol fields.
    let content_types = result
        .content
        .iter()
        .take(8)
        .map(|content| truncate_utf8(content_type_name(content), 32, "..."))
        .collect();
    let preview_budget = details_budget.saturating_sub(2_560) / 6;
    let (content_preview, _) = json_prefix(&ModelVisibleContents(&result.content), preview_budget);
    let structured_content_preview = result
        .structured_content
        .as_ref()
        .map(|content| json_prefix(content, preview_budget).0);
    let truncated = serde_json::to_string(&TruncatedMcpToolErrorDetails {
        truncated: true,
        content_types,
        content_types_truncated: result.content.len() > 8,
        content_preview,
        structured_content_preview,
    })
    .expect("truncated MCP tool error details serialize");
    debug_assert!(truncated.len() <= details_budget);
    format!("{summary}{DETAILS_PREFIX}{truncated}")
}

#[derive(Serialize)]
struct McpToolErrorDetails<'a> {
    content: ModelVisibleContents<'a>,
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    structured_content: Option<&'a serde_json::Map<String, Value>>,
}

#[derive(Serialize)]
struct TruncatedMcpToolErrorDetails {
    truncated: bool,
    #[serde(rename = "contentTypes")]
    content_types: Vec<String>,
    #[serde(
        rename = "contentTypesTruncated",
        skip_serializing_if = "std::ops::Not::not"
    )]
    content_types_truncated: bool,
    #[serde(rename = "contentPreview")]
    content_preview: String,
    #[serde(
        rename = "structuredContentPreview",
        skip_serializing_if = "Option::is_none"
    )]
    structured_content_preview: Option<String>,
}

struct ModelVisibleContents<'a>(&'a [McpContent]);

impl Serialize for ModelVisibleContents<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for content in self.0 {
            sequence.serialize_element(&ModelVisibleContent(content))?;
        }
        sequence.end()
    }
}

struct ModelVisibleContent<'a>(&'a McpContent);

impl Serialize for ModelVisibleContent<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            McpContent::Text {
                text, annotations, ..
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "text")?;
                map.serialize_entry("text", text)?;
                if let Some(annotations) = annotations {
                    map.serialize_entry("annotations", annotations)?;
                }
                map.end()
            }
            McpContent::Image {
                data,
                mime_type,
                annotations,
                ..
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "image")?;
                map.serialize_entry("data", data)?;
                map.serialize_entry("mimeType", mime_type)?;
                if let Some(annotations) = annotations {
                    map.serialize_entry("annotations", annotations)?;
                }
                map.end()
            }
            McpContent::Audio {
                data,
                mime_type,
                annotations,
                ..
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "audio")?;
                map.serialize_entry("data", data)?;
                map.serialize_entry("mimeType", mime_type)?;
                if let Some(annotations) = annotations {
                    map.serialize_entry("annotations", annotations)?;
                }
                map.end()
            }
            McpContent::ResourceLink { resource } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "resource_link")?;
                map.serialize_entry("uri", &resource.uri)?;
                map.serialize_entry("name", &resource.name)?;
                if let Some(title) = &resource.title {
                    map.serialize_entry("title", title)?;
                }
                if let Some(description) = &resource.description {
                    map.serialize_entry("description", description)?;
                }
                if let Some(mime_type) = &resource.mime_type {
                    map.serialize_entry("mimeType", mime_type)?;
                }
                if let Some(icons) = &resource.icons {
                    map.serialize_entry("icons", &ModelVisibleIcons(icons))?;
                }
                if let Some(annotations) = &resource.annotations {
                    map.serialize_entry("annotations", annotations)?;
                }
                if let Some(size) = resource.size {
                    map.serialize_entry("size", &size)?;
                }
                map.end()
            }
            McpContent::Resource {
                resource,
                annotations,
                ..
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "resource")?;
                map.serialize_entry("resource", &ModelVisibleResourceContents(resource))?;
                if let Some(annotations) = annotations {
                    map.serialize_entry("annotations", annotations)?;
                }
                map.end()
            }
            McpContent::Unknown(value) => ModelVisibleValue(value).serialize(serializer),
        }
    }
}

struct ModelVisibleResourceContents<'a>(&'a crate::McpResourceContents);

struct ModelVisibleIcons<'a>(&'a [crate::McpIcon]);

impl Serialize for ModelVisibleIcons<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for icon in self.0 {
            sequence.serialize_element(&ModelVisibleIcon(icon))?;
        }
        sequence.end()
    }
}

struct ModelVisibleIcon<'a>(&'a crate::McpIcon);

impl Serialize for ModelVisibleIcon<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let icon = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("src", &icon.src)?;
        if let Some(mime_type) = &icon.mime_type {
            map.serialize_entry("mimeType", mime_type)?;
        }
        if let Some(sizes) = &icon.sizes {
            map.serialize_entry("sizes", sizes)?;
        }
        if let Some(theme) = &icon.theme {
            map.serialize_entry("theme", theme)?;
        }
        for (key, value) in &icon.extra {
            if key != "_meta" {
                map.serialize_entry(key, &ModelVisibleValue(value))?;
            }
        }
        map.end()
    }
}

impl Serialize for ModelVisibleResourceContents<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        match self.0 {
            crate::McpResourceContents::Text {
                uri,
                mime_type,
                text,
                ..
            } => {
                map.serialize_entry("uri", uri)?;
                if let Some(mime_type) = mime_type {
                    map.serialize_entry("mimeType", mime_type)?;
                }
                map.serialize_entry("text", text)?;
            }
            crate::McpResourceContents::Blob {
                uri,
                mime_type,
                blob,
                ..
            } => {
                map.serialize_entry("uri", uri)?;
                if let Some(mime_type) = mime_type {
                    map.serialize_entry("mimeType", mime_type)?;
                }
                map.serialize_entry("blob", blob)?;
            }
        }
        map.end()
    }
}

struct ModelVisibleValue<'a>(&'a Value);

impl Serialize for ModelVisibleValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            Value::Object(object) => {
                let mut map = serializer.serialize_map(None)?;
                for (key, value) in object {
                    if key != "_meta" {
                        map.serialize_entry(key, &ModelVisibleValue(value))?;
                    }
                }
                map.end()
            }
            Value::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(&ModelVisibleValue(value))?;
                }
                sequence.end()
            }
            value => value.serialize(serializer),
        }
    }
}

fn content_type_name(content: &McpContent) -> &str {
    match content {
        McpContent::Text { .. } => "text",
        McpContent::Image { .. } => "image",
        McpContent::Audio { .. } => "audio",
        McpContent::ResourceLink { .. } => "resource_link",
        McpContent::Resource { .. } => "resource",
        McpContent::Unknown(value) => value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
    }
}

fn json_prefix<T>(value: &T, limit: usize) -> (String, bool)
where
    T: Serialize + ?Sized,
{
    let mut writer = LimitedWriter::new(limit);
    let result = serde_json::to_writer(&mut writer, value);
    let truncated = writer.truncated;
    if let Err(error) = result {
        assert!(truncated, "MCP error detail serialization failed: {error}");
    }
    (
        String::from_utf8_lossy(&writer.bytes).into_owned(),
        truncated,
    )
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit),
            limit,
            truncated: false,
        }
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let remaining = self.limit.saturating_sub(self.bytes.len());
        if buffer.len() <= remaining {
            self.bytes.extend_from_slice(buffer);
            return Ok(buffer.len());
        }
        self.bytes.extend_from_slice(&buffer[..remaining]);
        self.truncated = true;
        Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "MCP error detail limit reached",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn truncate_utf8(value: &str, max_bytes: usize, suffix: &str) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let keep = max_bytes.saturating_sub(suffix.len());
    let boundary = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= keep)
        .last()
        .unwrap_or(0);
    format!("{}{suffix}", &value[..boundary])
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
    let validator = if schema.get("$schema").is_some() {
        jsonschema::validator_for(schema)
    } else {
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(schema)
    }
    .map_err(|error| {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use harness_contracts::{ToolResult, ToolResultPart};
    use serde_json::json;

    use super::{into_tool_result, result_error_message, validate_input_schema};
    use crate::{
        McpAnnotations, McpContent, McpIcon, McpResource, McpResourceContents, McpToolResult,
    };

    #[test]
    fn wrapper_preserves_model_content_but_drops_protocol_metadata() {
        let result = into_tool_result(McpToolResult {
            content: vec![
                McpContent::Text {
                    text: "fallback".to_owned(),
                    annotations: Some(McpAnnotations {
                        audience: None,
                        priority: Some(0.8),
                        last_modified: None,
                    }),
                    meta: BTreeMap::from([("textSecret".to_owned(), json!("hidden"))]),
                },
                McpContent::Unknown(json!({
                    "type": "vendor",
                    "raw": true,
                    "_meta": { "contentSecret": "hidden" },
                    "nested": { "_meta": { "nestedSecret": "hidden" }, "value": 7 }
                })),
                McpContent::Resource {
                    resource: McpResourceContents::Text {
                        uri: "file:///tmp/report.txt".to_owned(),
                        mime_type: Some("text/plain".to_owned()),
                        text: "report".to_owned(),
                        meta: BTreeMap::from([("resourceSecret".to_owned(), json!("hidden"))]),
                    },
                    annotations: None,
                    meta: BTreeMap::from([("blockSecret".to_owned(), json!("hidden"))]),
                },
                McpContent::ResourceLink {
                    resource: Box::new(McpResource {
                        uri: "file:///tmp/icon.txt".to_owned(),
                        name: "icon resource".to_owned(),
                        title: None,
                        description: None,
                        mime_type: None,
                        icons: Some(vec![McpIcon {
                            src: "data:image/png;base64,AA==".to_owned(),
                            mime_type: Some("image/png".to_owned()),
                            sizes: None,
                            theme: None,
                            extra: json!({
                                "_meta": { "iconSecret": "hidden" },
                                "vendor": {
                                    "visible": true,
                                    "_meta": { "nestedIconSecret": "hidden" }
                                }
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        }]),
                        annotations: None,
                        size: None,
                        meta: BTreeMap::new(),
                    }),
                },
            ],
            structured_content: Some(
                json!({ "answer": 42 })
                    .as_object()
                    .expect("object fixture")
                    .clone(),
            ),
            is_error: false,
            meta: BTreeMap::from([("trace".to_owned(), json!("abc"))]),
        });

        let ToolResult::Mixed(parts) = result else {
            panic!("text and structured MCP data must remain mixed");
        };
        assert!(matches!(
            &parts[0],
            ToolResultPart::Text { text } if text == "fallback"
        ));
        assert!(parts.iter().any(|part| matches!(
            part,
            ToolResultPart::Structured { value, .. } if value == &json!({ "answer": 42 })
        )));
        let model_visible = serde_json::to_string(&parts).unwrap();
        assert!(!model_visible.contains("_meta"));
        assert!(!model_visible.contains("trace"));
        assert!(!model_visible.contains("Secret"));
        assert!(model_visible.contains("annotations"));
        assert!(model_visible.contains("nested"));
        assert!(model_visible.contains("resource"));
    }

    #[test]
    fn wrapper_error_message_preserves_bounded_protocol_details() {
        let result = McpToolResult {
            content: vec![
                McpContent::text("upstream failed"),
                McpContent::Unknown(json!({
                    "type": "vendor_error",
                    "code": 17,
                    "_meta": { "contentSecret": "hidden" }
                })),
            ],
            structured_content: Some(json!({ "reason": "quota" }).as_object().unwrap().clone()),
            is_error: true,
            meta: BTreeMap::from([("trace".to_owned(), json!("abc"))]),
        };

        let message = result_error_message(&result);
        assert_eq!(message, result_error_message(&result));
        let (_, details) = message
            .split_once("\nMCP error details: ")
            .expect("structured MCP error details");
        let details: serde_json::Value = serde_json::from_str(details).unwrap();
        assert_eq!(
            details,
            json!({
                "content": [
                    { "type": "text", "text": "upstream failed" },
                    { "type": "vendor_error", "code": 17 }
                ],
                "structuredContent": { "reason": "quota" }
            })
        );

        let oversized = McpToolResult {
            content: vec![McpContent::Unknown(json!({
                "type": "vendor_error",
                "payload": "x".repeat(32 * 1024)
            }))],
            structured_content: Some(
                json!({ "reason": "y".repeat(32 * 1024) })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            is_error: true,
            meta: BTreeMap::from([("trace".to_owned(), json!("z".repeat(32 * 1024)))]),
        };
        let oversized_message = result_error_message(&oversized);
        assert!(oversized_message.len() <= 16 * 1024);
        let (_, details) = oversized_message
            .split_once("\nMCP error details: ")
            .expect("truncated MCP error details");
        let details: serde_json::Value = serde_json::from_str(details).unwrap();
        assert_eq!(details.get("truncated"), Some(&json!(true)));
        assert!(details["contentTypes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind == "vendor_error"));
        assert!(details["contentPreview"]
            .as_str()
            .unwrap()
            .contains("payload"));
        assert!(details["structuredContentPreview"]
            .as_str()
            .unwrap()
            .contains("reason"));
        assert!(details.get("_metaPreview").is_none());
        assert!(!oversized_message.contains("trace"));
    }

    #[test]
    fn schema_defaults_to_2020_12_and_honors_an_explicit_draft() {
        let draft_2020_12 = json!({
            "type": "object",
            "properties": {
                "tuple": {
                    "type": "array",
                    "prefixItems": [{ "type": "string" }],
                    "items": { "type": "number" }
                }
            }
        });
        validate_input_schema(&draft_2020_12, &json!({ "tuple": ["first", 2] })).unwrap();

        let draft_7 = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "tuple": {
                    "type": "array",
                    "items": [{ "type": "string" }],
                    "additionalItems": false
                }
            }
        });
        validate_input_schema(&draft_7, &json!({ "tuple": ["first"] })).unwrap();
    }
}
