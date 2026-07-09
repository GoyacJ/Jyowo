use harness_contracts::{
    AssistantDeltaProducedEvent, DeltaChunk, Event, MessageId, ModelError, RunId, StopReason,
    UsageSnapshot,
};
use harness_model::{ErrorClass, ModelStreamEvent, StreamAggregate, StreamAggregator};
use harness_provider_state::ProviderContinuationKind;
use harness_tool::ToolCall;
use serde_json::Value;
use std::fmt;

#[derive(Clone, PartialEq)]
pub struct ProviderContinuationCapture {
    pub kind: ProviderContinuationKind,
    pub payload: Value,
}

impl fmt::Debug for ProviderContinuationCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderContinuationCapture")
            .field("kind", &self.kind)
            .field("payload", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnAssemblyStreamError {
    pub error: ModelError,
    pub class: ErrorClass,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TurnAssemblyStep {
    pub events: Vec<Event>,
    pub usage_delta: UsageSnapshot,
    pub stream_error: Option<TurnAssemblyStreamError>,
}

#[derive(Debug)]
pub struct TurnAssembly {
    assistant_message_id: MessageId,
    assistant_text: String,
    tool_calls: Vec<ToolCall>,
    stream_aggregator: StreamAggregator,
    stop_reason: StopReason,
    model_call_usage: UsageSnapshot,
    provider_continuations: Vec<ProviderContinuationCapture>,
}

impl TurnAssembly {
    #[must_use]
    pub fn new(assistant_message_id: MessageId) -> Self {
        Self {
            assistant_message_id,
            assistant_text: String::new(),
            tool_calls: Vec::new(),
            stream_aggregator: StreamAggregator::default(),
            stop_reason: StopReason::EndTurn,
            model_call_usage: UsageSnapshot::default(),
            provider_continuations: Vec::new(),
        }
    }

    #[must_use]
    pub fn assistant_message_id(&self) -> MessageId {
        self.assistant_message_id
    }

    #[must_use]
    pub fn assistant_text(&self) -> &str {
        &self.assistant_text
    }

    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }

    pub fn replace_tool_calls(&mut self, tool_calls: Vec<ToolCall>) {
        self.tool_calls = tool_calls;
    }

    #[must_use]
    pub fn stop_reason(&self) -> StopReason {
        self.stop_reason.clone()
    }

    #[must_use]
    pub fn model_call_usage(&self) -> &UsageSnapshot {
        &self.model_call_usage
    }

    #[must_use]
    pub fn provider_continuations(&self) -> &[ProviderContinuationCapture] {
        &self.provider_continuations
    }

    pub fn push_event(&mut self, run_id: RunId, event: ModelStreamEvent) -> TurnAssemblyStep {
        let mut step = TurnAssemblyStep::default();
        for aggregate in self.stream_aggregator.push(event) {
            self.push_aggregate(run_id, aggregate, &mut step);
            if step.stream_error.is_some() {
                break;
            }
        }
        step
    }

    fn push_aggregate(
        &mut self,
        run_id: RunId,
        aggregate: StreamAggregate,
        step: &mut TurnAssemblyStep,
    ) {
        match aggregate {
            StreamAggregate::MessageStart { usage } => {
                add_usage(&mut self.model_call_usage, &usage);
                add_usage(&mut step.usage_delta, &usage);
            }
            StreamAggregate::TextChunk { text } => {
                self.assistant_text.push_str(&text);
                step.events
                    .push(self.assistant_delta(run_id, DeltaChunk::Text(text)));
            }
            StreamAggregate::ThinkingChunk { thinking } => {
                let has_private_thinking_signal = thinking
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.is_empty())
                    || thinking.provider_native.is_some()
                    || thinking.signature.is_some();
                if has_private_thinking_signal {
                    step.events.push(self.assistant_delta(
                        run_id,
                        DeltaChunk::Thought(harness_contracts::ThoughtChunk {
                            text: thinking.text,
                            provider_id: "harness_model".to_owned(),
                            provider_native: thinking.provider_native,
                            signature: thinking.signature,
                        }),
                    ));
                }
            }
            StreamAggregate::ToolCallReady {
                tool_use_id,
                tool_name,
                input,
            } => {
                self.tool_calls.push(ToolCall {
                    tool_use_id,
                    tool_name,
                    input,
                });
                step.events
                    .push(self.assistant_delta(run_id, DeltaChunk::ToolUseEnd { tool_use_id }));
            }
            StreamAggregate::ReasoningSummaryChunk { summary } => {
                if !summary.text.is_empty() {
                    step.events.push(self.assistant_delta(
                        run_id,
                        DeltaChunk::ReasoningSummary(harness_contracts::ReasoningSummaryChunk {
                            text: summary.text,
                            provider_id: "harness_model".to_owned(),
                            provider_native: summary.provider_native,
                        }),
                    ));
                }
            }
            StreamAggregate::ToolUseStart {
                tool_use_id,
                tool_name,
            } => {
                step.events.push(self.assistant_delta(
                    run_id,
                    DeltaChunk::ToolUseStart {
                        tool_use_id,
                        tool_name,
                    },
                ));
            }
            StreamAggregate::ToolUseInputDelta { tool_use_id, delta } => {
                step.events.push(
                    self.assistant_delta(
                        run_id,
                        DeltaChunk::ToolUseInputDelta { tool_use_id, delta },
                    ),
                );
            }
            StreamAggregate::MessageDelta {
                stop_reason,
                usage_delta,
            } => {
                add_usage(&mut self.model_call_usage, &usage_delta);
                add_usage(&mut step.usage_delta, &usage_delta);
                if let Some(stop_reason) = stop_reason {
                    self.stop_reason = stop_reason;
                }
            }
            StreamAggregate::StreamError { error, class, .. } => {
                step.stream_error = Some(TurnAssemblyStreamError { error, class });
            }
            StreamAggregate::ProviderContinuationDelta { kind, payload } => {
                self.provider_continuations
                    .push(ProviderContinuationCapture { kind, payload });
            }
            StreamAggregate::MessageDone => {}
        }
    }

    fn assistant_delta(&self, run_id: RunId, delta: DeltaChunk) -> Event {
        Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
            run_id,
            message_id: self.assistant_message_id,
            delta,
            at: harness_contracts::now(),
        })
    }
}

fn add_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.tool_calls = total.tool_calls.saturating_add(delta.tool_calls);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
}
