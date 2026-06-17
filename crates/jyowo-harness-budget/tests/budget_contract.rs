use std::time::Duration;

use harness_budget::{ResourceQuota, TokenBudget};

#[test]
fn resource_quota_serializes_stable_shape() {
    let quota = ResourceQuota {
        max_tokens: Some(1_024),
        max_tool_calls: Some(4),
        max_duration: Some(Duration::from_secs(30)),
        max_cost_cents: Some(99),
    };

    let value = serde_json::to_value(&quota).unwrap();

    assert_eq!(value["max_tokens"], 1_024);
    assert_eq!(value["max_tool_calls"], 4);
    assert_eq!(value["max_cost_cents"], 99);
    let decoded: ResourceQuota = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, quota);
}

#[test]
fn token_budget_default_preserves_context_runtime_values() {
    let budget = TokenBudget::default();

    assert_eq!(budget.max_tokens_per_turn, 200_000);
    assert_eq!(budget.max_tokens_per_session, 1_000_000);
    assert!((budget.soft_budget_ratio - 0.8).abs() < f32::EPSILON);
    assert!((budget.hard_budget_ratio - 0.95).abs() < f32::EPSILON);
    assert_eq!(budget.per_tool_max_chars, 30_000);
}
