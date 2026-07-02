use super::*;

#[derive(Clone)]
pub struct HarnessSamplingProvider {
    model: Arc<dyn ModelProvider>,
    default_model_id: String,
    tenant_id: TenantId,
    session_id: Option<harness_contracts::SessionId>,
    run_id: Option<RunId>,
}

impl HarnessSamplingProvider {
    #[must_use]
    pub fn new(
        model: Arc<dyn ModelProvider>,
        default_model_id: impl Into<String>,
        tenant_id: TenantId,
        session_id: Option<harness_contracts::SessionId>,
        run_id: Option<RunId>,
    ) -> Self {
        Self {
            model,
            default_model_id: default_model_id.into(),
            tenant_id,
            session_id,
            run_id,
        }
    }
}

#[async_trait]
impl SamplingProvider for HarnessSamplingProvider {
    async fn create_message(
        &self,
        request: SamplingRequest,
    ) -> Result<SamplingResponse, harness_mcp::McpError> {
        let model_id = request
            .model_id
            .clone()
            .unwrap_or_else(|| self.default_model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.model.as_ref(), &model_id)
            .map_err(|error| harness_mcp::McpError::Protocol(error.to_string()))?;
        let messages = sampling_messages_from_params(&request.params)?;
        let model_request = ModelRequest {
            model_id: model_id.clone(),
            messages,
            tools: None,
            system: sampling_string_param(&request.params, "systemPrompt")
                .or_else(|| sampling_string_param(&request.params, "system")),
            temperature: request
                .params
                .get("temperature")
                .and_then(Value::as_f64)
                .map(|value| value as f32),
            max_tokens: (request.max_output_tokens > 0)
                .then(|| request.max_output_tokens.min(u64::from(u32::MAX)) as u32),
            stream: true,
            cache_breakpoints: Vec::new(),
            protocol: model_snapshot.protocol,
            extra: json!({
                "source": "mcp_sampling",
                "server_id": request.server_id.0,
                "request_id": request.request_id,
                "prompt_cache_namespace": request.prompt_cache_namespace,
            }),
            provider_context: harness_model::ProviderRequestContext::default(),
        };
        let mut context = InferContext::for_test();
        context.tenant_id = self.tenant_id;
        context.session_id = self.session_id.or(Some(request.session_id));
        context.run_id = self.run_id.or(request.run_id);
        let mut stream = self
            .model
            .infer(model_request, context)
            .await
            .map_err(|error| harness_mcp::McpError::Protocol(error.to_string()))?;
        let mut text = String::new();
        let mut usage = harness_contracts::UsageSnapshot::default();
        while let Some(event) = stream.next().await {
            match event {
                ModelStreamEvent::MessageStart {
                    usage: start_usage, ..
                } => add_usage(&mut usage, &start_usage),
                ModelStreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    ContentDelta::Text(delta) => text.push_str(&delta),
                    ContentDelta::Thinking(thinking) => {
                        if let Some(delta) = thinking.text {
                            text.push_str(&delta);
                        }
                    }
                    ContentDelta::ReasoningSummary(_) => {}
                    ContentDelta::ToolUseStart { .. }
                    | ContentDelta::ToolUseInputJson(_)
                    | ContentDelta::ToolUseComplete { .. } => {}
                },
                ModelStreamEvent::MessageDelta { usage_delta, .. } => {
                    add_usage(&mut usage, &usage_delta);
                }
                ModelStreamEvent::StreamError { error, .. } => {
                    return Err(harness_mcp::McpError::Protocol(error.to_string()));
                }
                ModelStreamEvent::MessageStop
                | ModelStreamEvent::ProviderContinuationDelta { .. }
                | ModelStreamEvent::ContentBlockStart { .. }
                | ModelStreamEvent::ContentBlockStop { .. } => {}
            }
        }
        Ok(SamplingResponse {
            model_id,
            content: json!({ "type": "text", "text": text }),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        })
    }
}

fn sampling_messages_from_params(params: &Value) -> Result<Vec<Message>, harness_mcp::McpError> {
    let Some(messages) = params.get("messages").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    messages
        .iter()
        .map(|message| {
            let role = match message.get("role").and_then(Value::as_str) {
                Some("assistant") => MessageRole::Assistant,
                Some("system") => MessageRole::System,
                Some("tool") => MessageRole::Tool,
                Some("user") | None => MessageRole::User,
                Some(other) => {
                    return Err(harness_mcp::McpError::Protocol(format!(
                        "unsupported sampling message role: {other}"
                    )))
                }
            };
            let content = message.get("content").unwrap_or(&Value::Null);
            Ok(Message {
                id: MessageId::new(),
                role,
                parts: vec![MessagePart::Text(sampling_content_text(content))],
                created_at: harness_contracts::now(),
            })
        })
        .collect()
}

fn sampling_content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    if let Some(text) = content.get("text").and_then(Value::as_str) {
        return text.to_owned();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.as_str().map(ToOwned::to_owned).or_else(|| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
            })
            .collect::<Vec<_>>()
            .join("");
    }
    String::new()
}

fn sampling_string_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn add_usage(
    total: &mut harness_contracts::UsageSnapshot,
    delta: &harness_contracts::UsageSnapshot,
) {
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
