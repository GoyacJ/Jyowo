use std::collections::HashSet;

use harness_contracts::{Message, MessageId, MessagePart, ToolResult, ToolUseId};

use crate::{estimate_message_tokens, CompactHint, ContextBuffer};

mod autocompact;
mod budget;
mod collapse;
mod microcompact;
mod snip;

pub use autocompact::AutocompactProvider;
pub use budget::ToolResultBudgetProvider;
pub use collapse::CollapseProvider;
pub use microcompact::{AuxFailureBudget, CompactSummaryLimits, MicrocompactProvider};
pub use snip::SnipProvider;

pub(crate) const PROTECTED_RECENT_N: usize = 3;
pub(crate) const MICROCOMPACT_MARKER: &str = "[MICROCOMPACT_SUMMARY]";
pub(crate) const AUTOCOMPACT_MARKER: &str = "[AUTOCOMPACT_HANDOFF]";

pub(crate) fn effective_estimate(ctx: &ContextBuffer, hint: &CompactHint) -> u64 {
    if hint.estimated_tokens > 0 {
        hint.estimated_tokens
    } else {
        ctx.bookkeeping.estimated_tokens
    }
}

pub(crate) fn next_snip_group(
    ctx: &ContextBuffer,
    protected_recent_n: usize,
) -> Option<HashSet<MessageId>> {
    let protected_start = ctx.active.history.len().saturating_sub(protected_recent_n);
    let protected_ids = ctx.active.history[protected_start..]
        .iter()
        .map(|message| message.id)
        .collect::<HashSet<_>>();
    let pairs = crate::buffer::rebuild_tool_use_pairs(&ctx.active.history);

    for candidate in &ctx.active.history[..protected_start] {
        if let Some(pair) = pairs
            .iter()
            .find(|pair| pair.tool_use_message_id == candidate.id)
        {
            let Some(result_id) = pair.tool_result_message_id else {
                continue;
            };
            let group = HashSet::from([candidate.id, result_id]);
            if group.is_disjoint(&protected_ids) {
                return Some(group);
            }
            continue;
        }

        if pairs.iter().any(|pair| {
            pair.tool_result_message_id == Some(candidate.id)
                && pair.tool_use_message_id != candidate.id
        }) {
            let pair = pairs
                .iter()
                .find(|pair| pair.tool_result_message_id == Some(candidate.id))?;
            let group = HashSet::from([candidate.id, pair.tool_use_message_id]);
            if group.is_disjoint(&protected_ids) {
                return Some(group);
            }
            continue;
        }

        return Some(HashSet::from([candidate.id]));
    }

    None
}

pub(crate) fn remove_messages(ctx: &mut ContextBuffer, drop_ids: &HashSet<MessageId>) -> u64 {
    let mut bytes_saved = 0_u64;
    ctx.active.history.retain(|message| {
        if drop_ids.contains(&message.id) {
            bytes_saved = bytes_saved.saturating_add(message_bytes(message) as u64);
            false
        } else {
            true
        }
    });

    for id in drop_ids {
        ctx.bookkeeping.offloads.remove(id);
    }

    bytes_saved
}

pub(crate) fn message_bytes(message: &Message) -> usize {
    message
        .parts
        .iter()
        .map(|part| format!("{part:?}").len())
        .sum()
}

pub(crate) fn tool_use_names(messages: &[Message]) -> std::collections::HashMap<ToolUseId, String> {
    let mut names = std::collections::HashMap::new();
    for message in messages {
        for part in &message.parts {
            if let MessagePart::ToolUse { id, name, .. } = part {
                names.insert(*id, name.clone());
            }
        }
    }
    names
}

pub(crate) fn first_tool_use(message: &Message) -> Option<(ToolUseId, String)> {
    message.parts.iter().find_map(|part| match part {
        MessagePart::ToolUse { id, name, .. } => Some((*id, name.clone())),
        _ => None,
    })
}

pub(crate) fn first_tool_result(message: &Message) -> Option<(ToolUseId, String)> {
    message.parts.iter().find_map(|part| match part {
        MessagePart::ToolResult {
            tool_use_id,
            content,
        } => tool_result_text(content).map(|text| (*tool_use_id, text)),
        _ => None,
    })
}

pub(crate) fn tool_result_text(result: &ToolResult) -> Option<String> {
    match result {
        ToolResult::Text(text) => Some(text.clone()),
        ToolResult::Structured(value) => Some(value.to_string()),
        ToolResult::Mixed(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    harness_contracts::ToolResultPart::Text { text: part_text } => {
                        text.push_str(part_text);
                    }
                    harness_contracts::ToolResultPart::Structured { value, .. } => {
                        text.push_str(&value.to_string());
                    }
                    harness_contracts::ToolResultPart::Code { text: code, .. } => {
                        text.push_str(code);
                    }
                    _ => {}
                }
            }
            Some(text)
        }
        _ => None,
    }
}

pub(crate) fn estimate_messages(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

pub(crate) fn contains_text_marker(message: &Message, marker: &str) -> bool {
    message.parts.iter().any(|part| match part {
        MessagePart::Text(text) => text.contains(marker),
        _ => false,
    })
}
