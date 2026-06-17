use harness_contracts::ToolSearchMode;

pub use harness_contracts::DeferredToolsDeltaAttachment as DeferredToolsDelta;

#[derive(Debug, Clone, Default)]
pub struct DeferredThresholdEvaluator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdMetrics {
    pub token_count: Option<u64>,
    pub char_count: u64,
    pub threshold_tokens: u64,
    pub absolute_floor: u64,
}

impl DeferredThresholdEvaluator {
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::match_same_arms)]
    pub fn evaluate_chars(
        &self,
        mode: &ToolSearchMode,
        deferred_schema_chars: u64,
        model_context_tokens: u64,
    ) -> (bool, ThresholdMetrics) {
        let estimated_tokens = (deferred_schema_chars as f64 / 2.5).ceil() as u64;
        let (enabled, threshold_tokens, absolute_floor) = match mode {
            ToolSearchMode::Always => (true, 0, 0),
            ToolSearchMode::Auto {
                ratio,
                min_absolute_tokens,
            } => {
                let threshold_tokens = (model_context_tokens as f32 * *ratio).ceil() as u64;
                let absolute_floor = u64::from(*min_absolute_tokens);
                (
                    estimated_tokens >= threshold_tokens.max(absolute_floor),
                    threshold_tokens,
                    absolute_floor,
                )
            }
            ToolSearchMode::Disabled => (false, 0, 0),
            _ => (false, 0, 0),
        };
        (
            enabled,
            ThresholdMetrics {
                token_count: None,
                char_count: deferred_schema_chars,
                threshold_tokens,
                absolute_floor,
            },
        )
    }
}
