use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use async_trait::async_trait;
use aws_sdk_bedrockruntime::types as br;
use aws_smithy_types::{Document, Number};
use futures::stream as futures_stream;
use harness_contracts::{
    MessagePart, MessageRole, ModelError, StopReason, ToolDescriptor, ToolResult, UsageSnapshot,
};
use serde_json::Value;

use crate::{
    wrap_stream_with_cancel_deadline, ContentDelta, ContentType, ConversationModelCapability,
    ErrorClass, ErrorHints, HealthStatus, InferContext, ModelDescriptor, ModelLifecycle,
    ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
    ThinkingDelta,
};

const PROVIDER_ID: &str = "bedrock";

#[derive(Clone)]
pub struct BedrockProvider {
    transport: Arc<dyn BedrockTransport>,
}

impl BedrockProvider {
    pub async fn new() -> Self {
        Self {
            transport: Arc::new(AwsBedrockTransport::new().await),
        }
    }

    #[doc(hidden)]
    #[must_use]
    #[cfg(any(test, feature = "testing"))]
    pub fn from_events(events: Vec<ModelStreamEvent>) -> Self {
        Self {
            transport: Arc::new(StaticBedrockTransport { events }),
        }
    }
}

#[async_trait]
pub trait BedrockTransport: Send + Sync + 'static {
    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError>;
}

struct AwsBedrockTransport {
    client: aws_sdk_bedrockruntime::Client,
}

impl AwsBedrockTransport {
    async fn new() -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        Self {
            client: aws_sdk_bedrockruntime::Client::new(&config),
        }
    }
}

#[async_trait]
impl BedrockTransport for AwsBedrockTransport {
    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let mut builder = self.client.converse_stream().model_id(req.model_id.clone());
        let messages = bedrock_messages(&req)?;
        if !messages.is_empty() {
            builder = builder.set_messages(Some(messages));
        }
        if let Some(system) = req.system.as_ref().filter(|system| !system.is_empty()) {
            builder = builder.system(br::SystemContentBlock::Text(system.clone()));
        }
        if req.max_tokens.is_some() || req.temperature.is_some() {
            builder = builder.inference_config(inference_config(&req));
        }
        if let Some(tools) = bedrock_tools(req.tools.as_deref())? {
            builder = builder.tool_config(tools);
        }

        let output = builder
            .send()
            .await
            .map_err(|error| ModelError::ProviderUnavailable(error.to_string()))?;
        Ok(bedrock_event_stream(output.stream))
    }
}

#[cfg(any(test, feature = "testing"))]
struct StaticBedrockTransport {
    events: Vec<ModelStreamEvent>,
}

#[cfg(any(test, feature = "testing"))]
#[async_trait]
impl BedrockTransport for StaticBedrockTransport {
    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures_stream::iter(self.events.clone())))
    }
}

fn bedrock_messages(req: &ModelRequest) -> Result<Vec<br::Message>, ModelError> {
    req.messages
        .iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::Assistant => br::ConversationRole::Assistant,
                MessageRole::User | MessageRole::Tool | MessageRole::System => {
                    br::ConversationRole::User
                }
                _ => br::ConversationRole::User,
            };
            let content = message
                .parts
                .iter()
                .map(bedrock_content_block)
                .collect::<Result<Vec<_>, _>>()?;
            br::Message::builder()
                .role(role)
                .set_content(Some(content))
                .build()
                .map_err(|error| ModelError::InvalidRequest(error.to_string()))
        })
        .collect()
}

fn bedrock_content_block(part: &MessagePart) -> Result<br::ContentBlock, ModelError> {
    match part {
        MessagePart::Text(text) => Ok(br::ContentBlock::Text(text.clone())),
        MessagePart::ToolUse { id, name, input } => {
            let block = br::ToolUseBlock::builder()
                .tool_use_id(id.to_string())
                .name(name.clone())
                .input(json_to_document(input))
                .build()
                .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;
            Ok(br::ContentBlock::ToolUse(block))
        }
        MessagePart::ToolResult {
            tool_use_id,
            content,
        } => {
            let block = br::ToolResultBlock::builder()
                .tool_use_id(tool_use_id.to_string())
                .content(tool_result_content(content))
                .build()
                .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;
            Ok(br::ContentBlock::ToolResult(block))
        }
        MessagePart::Thinking(thinking) => Ok(br::ContentBlock::Text(
            thinking.text.clone().unwrap_or_default(),
        )),
        MessagePart::Image { .. } => Err(ModelError::InvalidRequest(
            "BedrockProvider image blocks require blob materialization before inference".to_owned(),
        )),
        MessagePart::Video { .. } | MessagePart::File { .. } => Err(ModelError::InvalidRequest(
            "BedrockProvider does not inline file or video message parts yet".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Bedrock message part".to_owned(),
        )),
    }
}

fn tool_result_content(content: &ToolResult) -> br::ToolResultContentBlock {
    match content {
        ToolResult::Text(text) => br::ToolResultContentBlock::Text(text.clone()),
        ToolResult::Structured(value) => br::ToolResultContentBlock::Json(json_to_document(value)),
        ToolResult::Mixed(parts) => br::ToolResultContentBlock::Text(
            serde_json::to_string(parts).unwrap_or_else(|_| "[]".to_owned()),
        ),
        ToolResult::Blob { .. } => br::ToolResultContentBlock::Text("[blob result]".to_owned()),
        _ => br::ToolResultContentBlock::Text("[unsupported result]".to_owned()),
    }
}

fn inference_config(req: &ModelRequest) -> br::InferenceConfiguration {
    br::InferenceConfiguration::builder()
        .set_max_tokens(req.max_tokens.map(|value| value as i32))
        .set_temperature(req.temperature)
        .build()
}

fn bedrock_tools(
    tools: Option<&[ToolDescriptor]>,
) -> Result<Option<br::ToolConfiguration>, ModelError> {
    let Some(tools) = tools.filter(|tools| !tools.is_empty()) else {
        return Ok(None);
    };
    let tools = tools
        .iter()
        .map(|tool| {
            let spec = br::ToolSpecification::builder()
                .name(tool.name.clone())
                .description(tool.description.clone())
                .input_schema(br::ToolInputSchema::Json(json_to_document(
                    &tool.input_schema,
                )))
                .build()
                .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;
            Ok(br::Tool::ToolSpec(spec))
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    Ok(Some(
        br::ToolConfiguration::builder()
            .set_tools(Some(tools))
            .build()
            .map_err(|error| ModelError::InvalidRequest(error.to_string()))?,
    ))
}

fn json_to_document(value: &Value) -> Document {
    match value {
        Value::Null => Document::Null,
        Value::Bool(value) => Document::Bool(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                Document::Number(Number::PosInt(value))
            } else if let Some(value) = value.as_i64() {
                Document::Number(Number::NegInt(value))
            } else {
                Document::Number(Number::Float(value.as_f64().unwrap_or_default()))
            }
        }
        Value::String(value) => Document::String(value.clone()),
        Value::Array(values) => Document::Array(values.iter().map(json_to_document).collect()),
        Value::Object(values) => Document::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), json_to_document(value)))
                .collect(),
        ),
    }
}

fn bedrock_event_stream(
    mut events: aws_sdk_bedrockruntime::primitives::event_stream::EventReceiver<
        br::ConverseStreamOutput,
        br::error::ConverseStreamOutputError,
    >,
) -> ModelStream {
    Box::pin(stream! {
        loop {
            let event = match events.recv().await {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(error) => {
                    yield ModelStreamEvent::StreamError {
                        error: ModelError::UnexpectedResponse(error.to_string()),
                        class: ErrorClass::Transient,
                        hints: ErrorHints::default(),
                    };
                    break;
                }
            };
            for mapped in map_bedrock_event(event) {
                yield mapped;
            }
        }
    })
}

fn map_bedrock_event(event: br::ConverseStreamOutput) -> Vec<ModelStreamEvent> {
    match event {
        br::ConverseStreamOutput::MessageStart(_) => vec![ModelStreamEvent::MessageStart {
            message_id: String::new(),
            usage: UsageSnapshot::default(),
        }],
        br::ConverseStreamOutput::ContentBlockStart(event) => map_content_block_start(&event),
        br::ConverseStreamOutput::ContentBlockDelta(event) => map_content_block_delta(&event),
        br::ConverseStreamOutput::ContentBlockStop(event) => {
            vec![ModelStreamEvent::ContentBlockStop {
                index: event.content_block_index() as u32,
            }]
        }
        br::ConverseStreamOutput::MessageStop(event) => vec![
            ModelStreamEvent::MessageDelta {
                stop_reason: Some(map_stop_reason(event.stop_reason())),
                usage_delta: UsageSnapshot::default(),
            },
            ModelStreamEvent::MessageStop,
        ],
        br::ConverseStreamOutput::Metadata(event) => event
            .usage()
            .map(|usage| {
                vec![ModelStreamEvent::MessageDelta {
                    stop_reason: None,
                    usage_delta: usage_snapshot(usage),
                }]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn map_content_block_start(event: &br::ContentBlockStartEvent) -> Vec<ModelStreamEvent> {
    let index = event.content_block_index() as u32;
    match event.start() {
        Some(br::ContentBlockStart::ToolUse(tool)) => vec![
            ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::ToolUse,
            },
            ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ToolUseStart {
                    id: tool.tool_use_id().to_owned(),
                    name: tool.name().to_owned(),
                },
            },
        ],
        _ => Vec::new(),
    }
}

fn map_content_block_delta(event: &br::ContentBlockDeltaEvent) -> Vec<ModelStreamEvent> {
    let index = event.content_block_index() as u32;
    let Some(delta) = event.delta() else {
        return Vec::new();
    };
    match delta {
        br::ContentBlockDelta::Text(text) => vec![ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::Text(text.clone()),
        }],
        br::ContentBlockDelta::ToolUse(tool) => vec![ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::ToolUseInputJson(tool.input().to_owned()),
        }],
        br::ContentBlockDelta::ReasoningContent(reasoning) => {
            vec![ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::Thinking(match reasoning {
                    br::ReasoningContentBlockDelta::Text(text) => ThinkingDelta {
                        text: Some(text.clone()),
                        provider_native: None,
                        signature: None,
                    },
                    br::ReasoningContentBlockDelta::Signature(signature) => ThinkingDelta {
                        text: None,
                        provider_native: None,
                        signature: Some(signature.clone()),
                    },
                    _ => ThinkingDelta {
                        text: None,
                        provider_native: Some(Value::String("redacted_or_unknown".to_owned())),
                        signature: None,
                    },
                }),
            }]
        }
        _ => Vec::new(),
    }
}

fn map_stop_reason(reason: &br::StopReason) -> StopReason {
    match reason {
        br::StopReason::EndTurn | br::StopReason::StopSequence => StopReason::EndTurn,
        br::StopReason::ToolUse => StopReason::ToolUse,
        br::StopReason::MaxTokens | br::StopReason::ModelContextWindowExceeded => {
            StopReason::Error(reason.as_str().to_owned())
        }
        _ => StopReason::Error(reason.as_str().to_owned()),
    }
}

fn usage_snapshot(usage: &br::TokenUsage) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: usage.input_tokens().max(0) as u64,
        output_tokens: usage.output_tokens().max(0) as u64,
        cache_read_tokens: usage.cache_read_input_tokens().unwrap_or_default().max(0) as u64,
        cache_write_tokens: usage.cache_write_input_tokens().unwrap_or_default().max(0) as u64,
        cost_micros: 0,
        tool_calls: 0,
    }
}

#[async_trait]
impl ModelProvider for BedrockProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![descriptor(
            "anthropic.claude-3-5-sonnet-20241022-v2:0",
            "Claude 3.5 Sonnet on Bedrock",
        )]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        validate_request(&req, &ctx)?;
        let stream = self.transport.infer(req, ctx.clone()).await?;
        Ok(wrap_stream_with_cancel_deadline(stream, &ctx))
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Messages
    }
    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

fn validate_request(req: &ModelRequest, ctx: &InferContext) -> Result<(), ModelError> {
    if req.protocol != ModelProtocol::Messages {
        return Err(ModelError::InvalidRequest(
            "BedrockProvider only supports ModelProtocol::Messages".to_owned(),
        ));
    }
    if ctx.cancel.is_cancelled() {
        return Err(ModelError::Cancelled);
    }
    if let Some(deadline) = ctx.deadline {
        if Instant::now() >= deadline {
            return Err(ModelError::DeadlineExceeded(Duration::ZERO));
        }
    }
    bedrock_messages(req).map(|_| ())
}

fn descriptor(model_id: &str, display_name: &str) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "bedrock".to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::Messages,
        context_window: 200_000,
        max_output_tokens: 8192,
        conversation_capability: ConversationModelCapability {
            context_window: 200_000,
            max_output_tokens: 8192,
            tool_calling: true,
            reasoning: true,
            prompt_cache: false,
            streaming: true,
            structured_output: false,
            input_modalities: vec![ModelModality::Text],
            output_modalities: vec![ModelModality::Text],
        },
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}
