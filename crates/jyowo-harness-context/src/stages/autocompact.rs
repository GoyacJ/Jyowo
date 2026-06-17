use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    ContextError, ContextStageId, Message, MessageId, MessagePart, MessageRole,
};
use harness_model::{AuxExecutor, AuxModelProvider, AuxTask, ModelMetricsSink};

use crate::{CompactHint, ContextBuffer, ContextOutcome, ContextProvider};

use super::microcompact::{
    aux_request, prepare_summary, remove_selected, select_compaction_batch, selected_messages,
    AuxFailureBudget, CompactSummaryLimits,
};
use super::{effective_estimate, AUTOCOMPACT_MARKER, PROTECTED_RECENT_N};

pub struct AutocompactProvider {
    aux_executor: Option<AuxExecutor>,
    hard_budget_per_mille: u32,
    limits: CompactSummaryLimits,
    failure_budget: AuxFailureBudget,
}

impl AutocompactProvider {
    pub fn new(aux_provider: Option<Arc<dyn AuxModelProvider>>) -> Self {
        Self {
            aux_executor: aux_provider.map(AuxExecutor::new),
            hard_budget_per_mille: 950,
            limits: CompactSummaryLimits::default(),
            failure_budget: AuxFailureBudget::default(),
        }
    }

    #[must_use]
    pub fn with_model_metrics_sink(mut self, metrics_sink: Arc<dyn ModelMetricsSink>) -> Self {
        if let Some(aux_executor) = self.aux_executor.take() {
            self.aux_executor = Some(aux_executor.with_metrics_sink(metrics_sink));
        }
        self
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn with_hard_budget_ratio(mut self, hard_budget_ratio: f32) -> Self {
        self.hard_budget_per_mille = (hard_budget_ratio.clamp(0.0, 1.0) * 1_000.0).round() as u32;
        self
    }

    #[must_use]
    pub fn with_limits(mut self, limits: CompactSummaryLimits) -> Self {
        self.limits = limits;
        self
    }

    #[must_use]
    pub fn with_failure_budget(mut self, failure_budget: AuxFailureBudget) -> Self {
        self.failure_budget = failure_budget;
        self
    }
}

#[async_trait]
impl ContextProvider for AutocompactProvider {
    fn provider_id(&self) -> &'static str {
        "autocompact"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Autocompact
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        let hard_budget = hint
            .target_tokens
            .unwrap_or(ctx.bookkeeping.budget_snapshot.max_tokens_per_turn)
            .max(1);
        let threshold = hard_budget
            .saturating_mul(u64::from(self.hard_budget_per_mille))
            .div_ceil(1_000);
        if effective_estimate(ctx, hint) < threshold {
            return Ok(ContextOutcome::NoChange);
        }

        let handoff = if let Some(aux_executor) = &self.aux_executor {
            let selected_messages = ctx.active.history.iter().collect::<Vec<_>>();
            let aux_provider = aux_executor.provider();
            let req = aux_request(aux_provider.as_ref(), selected_messages, self.limits);
            match aux_executor.call(AuxTask::Compact, req).await {
                Ok(Some(summary)) => prepare_summary(summary, self.limits)
                    .unwrap_or_else(|| fallback_handoff(&ctx.active.history)),
                Ok(None) | Err(_) => fallback_handoff(&ctx.active.history),
            }
        } else {
            fallback_handoff(&ctx.active.history)
        };

        let selected = select_compaction_batch(ctx, PROTECTED_RECENT_N, usize::MAX)
            .map(|(selected, _)| selected)
            .unwrap_or_default();
        let bytes_before = selected_messages(ctx, &selected)
            .iter()
            .map(|message| super::message_bytes(message))
            .sum::<usize>() as u64;
        remove_selected(ctx, &selected);
        ctx.active.history.insert(
            0,
            Message {
                id: MessageId::new(),
                role: MessageRole::Assistant,
                parts: vec![MessagePart::Text(format!(
                    "{AUTOCOMPACT_MARKER}\n{handoff}"
                ))],
                created_at: chrono::Utc::now(),
            },
        );
        let _ = bytes_before.saturating_sub(handoff.len() as u64);
        let _ = self.failure_budget;

        Ok(ContextOutcome::Forked {
            new_session_id: harness_contracts::SessionId::new(),
        })
    }
}

fn fallback_handoff(messages: &[Message]) -> String {
    let last_user = messages.iter().rev().find_map(|message| {
        (message.role == MessageRole::User).then(|| {
            message.parts.iter().find_map(|part| match part {
                MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
        })?
    });

    format!(
        "Active Task: {}",
        last_user.unwrap_or("continue the current task")
    )
}
