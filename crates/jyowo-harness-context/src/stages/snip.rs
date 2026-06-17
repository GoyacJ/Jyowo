use std::collections::HashSet;

use async_trait::async_trait;
use harness_contracts::{ContextError, ContextStageId, MessageId};

use crate::{CompactHint, ContextBuffer, ContextOutcome, ContextProvider};

use super::{effective_estimate, estimate_messages, next_snip_group, remove_messages};

pub struct SnipProvider {
    protected_recent_n: usize,
}

impl SnipProvider {
    pub fn new(protected_recent_n: usize) -> Self {
        Self { protected_recent_n }
    }
}

#[async_trait]
impl ContextProvider for SnipProvider {
    fn provider_id(&self) -> &'static str {
        "snip"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Snip
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        let Some(target_tokens) = hint.target_tokens else {
            return Ok(ContextOutcome::NoChange);
        };

        let mut estimated = effective_estimate(ctx, hint);
        if estimated <= target_tokens {
            return Ok(ContextOutcome::NoChange);
        }

        let mut dropped: HashSet<MessageId> = HashSet::new();
        let mut bytes_saved = 0_u64;

        while let Some(drop_ids) = next_snip_group(ctx, self.protected_recent_n) {
            bytes_saved = bytes_saved.saturating_add(remove_messages(ctx, &drop_ids));
            dropped.extend(drop_ids);
            estimated = estimate_messages(&ctx.active.history);

            if estimated <= target_tokens {
                break;
            }
        }

        if dropped.is_empty() {
            Ok(ContextOutcome::NoChange)
        } else {
            Ok(ContextOutcome::Modified { bytes_saved })
        }
    }
}
