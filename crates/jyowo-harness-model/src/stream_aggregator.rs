use std::collections::BTreeMap;

use harness_contracts::{ModelError, StopReason, ToolUseId, UsageSnapshot};
use serde_json::Value;

use crate::{ContentDelta, ErrorClass, ErrorHints, ModelStreamEvent, ThinkingDelta};

#[derive(Debug, Clone, PartialEq)]
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
    StreamError {
        error: ModelError,
        class: ErrorClass,
        hints: ErrorHints,
    },
}

#[derive(Debug, Default)]
pub struct StreamAggregator {
    pending: BTreeMap<u32, PendingToolUse>,
    terminal: bool,
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
            ModelStreamEvent::ContentBlockStop { index } => self.finish(index),
            ModelStreamEvent::MessageDelta {
                stop_reason,
                usage_delta,
            } => vec![StreamAggregate::MessageDelta {
                stop_reason,
                usage_delta,
            }],
            ModelStreamEvent::MessageStop => self.finish_message(),
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
            ContentDelta::Text(text) => vec![StreamAggregate::TextChunk { text }],
            ContentDelta::Thinking(thinking) => {
                vec![StreamAggregate::ThinkingChunk { thinking }]
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

    fn finish(&mut self, index: u32) -> Vec<StreamAggregate> {
        self.pending
            .remove(&index)
            .map_or_else(Vec::new, |pending| self.finish_pending(pending))
    }

    fn finish_message(&mut self) -> Vec<StreamAggregate> {
        let mut output = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        for pending in pending.into_values() {
            output.extend(self.finish_pending(pending));
            if self.terminal {
                return output;
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
