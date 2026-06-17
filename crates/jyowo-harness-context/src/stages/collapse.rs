use async_trait::async_trait;
use harness_contracts::{ContextError, ContextStageId};

use crate::{CompactHint, ContextBuffer, ContextOutcome, ContextProvider};

use super::{first_tool_result, first_tool_use, message_bytes, tool_use_names};

pub struct CollapseProvider {
    merge_threshold_chars: usize,
}

impl CollapseProvider {
    pub fn new(merge_threshold_chars: usize) -> Self {
        Self {
            merge_threshold_chars,
        }
    }
}

#[async_trait]
impl ContextProvider for CollapseProvider {
    fn provider_id(&self) -> &'static str {
        "collapse"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Collapse
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        let tool_names = tool_use_names(&ctx.active.history);
        let mut index = 0_usize;
        let mut bytes_saved = 0_u64;

        while index + 2 < ctx.active.history.len() {
            let first_result = first_tool_result(&ctx.active.history[index]);
            let between_tool_use = first_tool_use(&ctx.active.history[index + 1]);
            let second_result = first_tool_result(&ctx.active.history[index + 2]);

            let (
                Some((first_use, first_text)),
                Some((second_use, second_text)),
                Some((between_use, between_name)),
            ) = (first_result, second_result, between_tool_use)
            else {
                index += 1;
                continue;
            };

            if second_use != between_use {
                index += 1;
                continue;
            }

            if tool_names.get(&first_use) != Some(&between_name)
                || tool_names.get(&second_use) != Some(&between_name)
            {
                index += 1;
                continue;
            }

            if first_text.len().saturating_add(second_text.len()) > self.merge_threshold_chars {
                index += 1;
                continue;
            }

            let removed = ctx.active.history.remove(index + 2);
            let mut removed_parts = removed.parts;
            ctx.active.history[index].parts.append(&mut removed_parts);
            bytes_saved =
                bytes_saved.saturating_add(message_bytes(&ctx.active.history[index]) as u64);
            ctx.bookkeeping.offloads.remove(&removed.id);
        }

        if bytes_saved == 0 {
            Ok(ContextOutcome::NoChange)
        } else {
            Ok(ContextOutcome::Modified { bytes_saved })
        }
    }
}
