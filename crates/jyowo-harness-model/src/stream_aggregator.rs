use std::collections::BTreeMap;
use std::fmt;

use harness_contracts::{ModelError, StopReason, ToolUseId, UsageSnapshot};
use harness_provider_state::ProviderContinuationKind;
use serde_json::Value;

use crate::{
    thinking_tag_normalizer::ThinkingTagNormalizer, ContentDelta, ErrorClass, ErrorHints,
    ModelStreamEvent, ReasoningSummaryDelta, ThinkingDelta,
};

#[derive(Clone, PartialEq)]
pub enum StreamAggregate {
    MessageStart {
        usage: UsageSnapshot,
    },
    TextChunk {
        text: String,
    },
    ThinkingChunk {
        thinking: ThinkingDelta,
    },
    ReasoningSummaryChunk {
        summary: ReasoningSummaryDelta,
    },
    ToolUseStart {
        tool_use_id: ToolUseId,
        tool_name: String,
    },
    ToolUseInputDelta {
        tool_use_id: ToolUseId,
        delta: String,
    },
    ToolCallReady {
        tool_use_id: ToolUseId,
        tool_name: String,
        input: Value,
    },
    MessageDelta {
        stop_reason: Option<StopReason>,
        usage_delta: UsageSnapshot,
    },
    MessageDone,
    ProviderContinuationDelta {
        kind: ProviderContinuationKind,
        payload: Value,
    },
    StreamError {
        error: ModelError,
        class: ErrorClass,
        hints: ErrorHints,
    },
}

impl fmt::Debug for StreamAggregate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageStart { usage } => formatter
                .debug_struct("MessageStart")
                .field("usage", usage)
                .finish(),
            Self::TextChunk { text } => formatter
                .debug_struct("TextChunk")
                .field("text", text)
                .finish(),
            Self::ThinkingChunk { thinking } => formatter
                .debug_struct("ThinkingChunk")
                .field("thinking", thinking)
                .finish(),
            Self::ReasoningSummaryChunk { summary } => formatter
                .debug_struct("ReasoningSummaryChunk")
                .field("summary", summary)
                .finish(),
            Self::ToolUseStart {
                tool_use_id,
                tool_name,
            } => formatter
                .debug_struct("ToolUseStart")
                .field("tool_use_id", tool_use_id)
                .field("tool_name", tool_name)
                .finish(),
            Self::ToolUseInputDelta { tool_use_id, delta } => formatter
                .debug_struct("ToolUseInputDelta")
                .field("tool_use_id", tool_use_id)
                .field("delta", delta)
                .finish(),
            Self::ToolCallReady {
                tool_use_id,
                tool_name,
                input,
            } => formatter
                .debug_struct("ToolCallReady")
                .field("tool_use_id", tool_use_id)
                .field("tool_name", tool_name)
                .field("input", input)
                .finish(),
            Self::MessageDelta {
                stop_reason,
                usage_delta,
            } => formatter
                .debug_struct("MessageDelta")
                .field("stop_reason", stop_reason)
                .field("usage_delta", usage_delta)
                .finish(),
            Self::MessageDone => formatter.debug_struct("MessageDone").finish(),
            Self::ProviderContinuationDelta { kind, .. } => formatter
                .debug_struct("ProviderContinuationDelta")
                .field("kind", kind)
                .field("payload", &"<redacted>")
                .finish(),
            Self::StreamError {
                error,
                class,
                hints,
            } => formatter
                .debug_struct("StreamError")
                .field("error", error)
                .field("class", class)
                .field("hints", hints)
                .finish(),
        }
    }
}

#[derive(Debug, Default)]
pub struct StreamAggregator {
    pending: BTreeMap<u32, PendingToolUse>,
    stop_reason: Option<StopReason>,
    terminal: bool,
    thinking_normalizer: ThinkingTagNormalizer,
}

impl StreamAggregator {
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.terminal
    }

    pub fn push(&mut self, event: ModelStreamEvent) -> Vec<StreamAggregate> {
        if self.terminal {
            return Vec::new();
        }

        match event {
            ModelStreamEvent::MessageStart { usage, .. } => {
                vec![StreamAggregate::MessageStart { usage }]
            }
            ModelStreamEvent::ContentBlockDelta { index, delta } => self.push_delta(index, delta),
            ModelStreamEvent::ContentBlockStop { .. } => {
                // Providers can report the terminal reason after closing content blocks.
                // Defer tool JSON parsing until MessageStop so truncated calls can be discarded.
                Vec::new()
            }
            ModelStreamEvent::MessageDelta {
                stop_reason,
                usage_delta,
            } => {
                if let Some(reason) = stop_reason.as_ref() {
                    self.stop_reason = Some(reason.clone());
                }
                vec![StreamAggregate::MessageDelta {
                    stop_reason,
                    usage_delta,
                }]
            }
            ModelStreamEvent::MessageStop => self.finish_message(),
            ModelStreamEvent::ProviderContinuationDelta { kind, payload } => {
                vec![StreamAggregate::ProviderContinuationDelta { kind, payload }]
            }
            ModelStreamEvent::StreamError {
                error,
                class,
                hints,
            } => self.stream_error(error, class, hints),
            ModelStreamEvent::ContentBlockStart { .. } => Vec::new(),
        }
    }

    fn push_delta(&mut self, index: u32, delta: ContentDelta) -> Vec<StreamAggregate> {
        match delta {
            ContentDelta::Text(text) => self
                .thinking_normalizer
                .push(text)
                .into_iter()
                .flat_map(|normalized| match normalized {
                    ContentDelta::Text(chunk) => vec![StreamAggregate::TextChunk { text: chunk }],
                    ContentDelta::Thinking(thinking) => {
                        vec![StreamAggregate::ThinkingChunk { thinking }]
                    }
                    ContentDelta::ReasoningSummary(summary) => {
                        vec![StreamAggregate::ReasoningSummaryChunk { summary }]
                    }
                    _ => Vec::new(),
                })
                .collect(),
            ContentDelta::Thinking(thinking) => {
                vec![StreamAggregate::ThinkingChunk { thinking }]
            }
            ContentDelta::ReasoningSummary(summary) => {
                vec![StreamAggregate::ReasoningSummaryChunk { summary }]
            }
            ContentDelta::ToolUseComplete { id, name, input } => {
                self.pending.remove(&index);
                vec![StreamAggregate::ToolCallReady {
                    tool_use_id: id,
                    tool_name: name,
                    input,
                }]
            }
            ContentDelta::ToolUseStart { id, name } => {
                let tool_use_id = ToolUseId::new();
                self.pending.insert(
                    index,
                    PendingToolUse {
                        tool_use_id,
                        provider_id: id,
                        name: name.clone(),
                        input_json: String::new(),
                    },
                );
                vec![StreamAggregate::ToolUseStart {
                    tool_use_id,
                    tool_name: name,
                }]
            }
            ContentDelta::ToolUseInputJson(delta) => {
                let Some(pending) = self.pending.get_mut(&index) else {
                    return self.fatal(format!(
                        "tool input delta received before tool start for content block {index}"
                    ));
                };
                pending.input_json.push_str(&delta);
                vec![StreamAggregate::ToolUseInputDelta {
                    tool_use_id: pending.tool_use_id,
                    delta,
                }]
            }
        }
    }

    fn finish_message(&mut self) -> Vec<StreamAggregate> {
        let mut output = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        if !matches!(self.stop_reason.as_ref(), Some(StopReason::MaxIterations)) {
            for pending in pending.into_values() {
                output.extend(self.finish_pending(pending));
                if self.terminal {
                    return output;
                }
            }
        }
        self.terminal = true;
        output.push(StreamAggregate::MessageDone);
        output
    }

    fn finish_pending(&mut self, pending: PendingToolUse) -> Vec<StreamAggregate> {
        let input = if pending.input_json.trim().is_empty() {
            Value::Null
        } else {
            match serde_json::from_str(&pending.input_json) {
                Ok(input) => input,
                Err(error) => {
                    return self.fatal(format!(
                        "invalid tool input json for {} (provider id {}): {error}",
                        pending.name, pending.provider_id
                    ));
                }
            }
        };
        vec![StreamAggregate::ToolCallReady {
            tool_use_id: pending.tool_use_id,
            tool_name: pending.name,
            input,
        }]
    }

    fn fatal(&mut self, message: String) -> Vec<StreamAggregate> {
        self.stream_error(
            ModelError::UnexpectedResponse(message),
            ErrorClass::Fatal,
            ErrorHints::default(),
        )
    }

    fn stream_error(
        &mut self,
        error: ModelError,
        class: ErrorClass,
        hints: ErrorHints,
    ) -> Vec<StreamAggregate> {
        self.pending.clear();
        self.terminal = true;
        vec![StreamAggregate::StreamError {
            error,
            class,
            hints,
        }]
    }
}

#[derive(Debug)]
struct PendingToolUse {
    tool_use_id: ToolUseId,
    provider_id: String,
    name: String,
    input_json: String,
}
