//! `jyowo-harness-budget`
//!
//! Shared resource quota and token budget carriers for harness runtimes.

#![forbid(unsafe_code)]

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceQuota {
    pub max_tokens: Option<u64>,
    pub max_tool_calls: Option<u64>,
    pub max_duration: Option<Duration>,
    pub max_cost_cents: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TokenBudget {
    pub max_tokens_per_turn: u64,
    pub max_tokens_per_session: u64,
    pub soft_budget_ratio: f32,
    pub hard_budget_ratio: f32,
    pub per_tool_max_chars: u64,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            max_tokens_per_turn: 200_000,
            max_tokens_per_session: 1_000_000,
            soft_budget_ratio: 0.8,
            hard_budget_ratio: 0.95,
            per_tool_max_chars: 30_000,
        }
    }
}
