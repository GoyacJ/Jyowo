use harness_contracts::{BudgetMetric, OverflowAction, ResultBudget};

pub fn default_result_budget() -> ResultBudget {
    ResultBudget {
        metric: BudgetMetric::Chars,
        limit: 30_000,
        on_overflow: OverflowAction::Offload,
        preview_head_chars: 2_000,
        preview_tail_chars: 2_000,
    }
}
