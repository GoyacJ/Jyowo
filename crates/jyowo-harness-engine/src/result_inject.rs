use harness_contracts::{
    Message, MessageContent, MessageId, MessagePart, MessageRole, ToolResult, TurnInput,
};
use harness_tool::{ToolCall, ToolResultEnvelope};
use serde_json::json;

pub(crate) fn assistant_tool_message(
    message_id: MessageId,
    assistant_text: String,
    tool_calls: &[ToolCall],
) -> Message {
    Message {
        id: message_id,
        role: MessageRole::Assistant,
        parts: assistant_tool_parts(assistant_text, tool_calls),
        created_at: harness_contracts::now(),
    }
}

pub(crate) fn assistant_tool_content(
    assistant_text: String,
    tool_calls: &[ToolCall],
) -> MessageContent {
    MessageContent::Multimodal(assistant_tool_parts(assistant_text, tool_calls))
}

pub(crate) fn tool_result_messages(results: &[ToolResultEnvelope]) -> Vec<Message> {
    results
        .iter()
        .map(|result| Message {
            id: MessageId::new(),
            role: MessageRole::Tool,
            parts: vec![MessagePart::ToolResult {
                tool_use_id: result.tool_use_id,
                content: result
                    .result
                    .clone()
                    .unwrap_or_else(|error| ToolResult::Text(error.to_string())),
            }],
            created_at: harness_contracts::now(),
        })
        .collect()
}

pub(crate) fn turn_input_from_message(message: Message) -> TurnInput {
    TurnInput {
        message,
        metadata: json!({ "source": "tool_result_reinjection" }),
    }
}

fn assistant_tool_parts(assistant_text: String, tool_calls: &[ToolCall]) -> Vec<MessagePart> {
    let mut parts = Vec::new();
    if !assistant_text.is_empty() {
        parts.push(MessagePart::Text(assistant_text));
    }
    parts.extend(tool_calls.iter().map(|call| MessagePart::ToolUse {
        id: call.tool_use_id,
        name: call.tool_name.clone(),
        input: call.input.clone(),
    }));
    parts
}
