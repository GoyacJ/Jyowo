use std::sync::Arc;

use harness_contracts::{ModelRef, PricingSnapshotId, SessionId, TenantId, UsageSnapshot};
use harness_observability::{
    CostCalculator, PricingBillingMode, PricingSource, PricingTableEntry, UsageAccumulator,
    UsageCost, UsageScope,
};
use rust_decimal::Decimal;

#[test]
fn usage_accumulator_records_global_and_selected_scope() {
    let usage = UsageAccumulator::default();
    let tenant_id = TenantId::new();

    usage.record(
        UsageScope::Tenant(tenant_id),
        None,
        UsageSnapshot {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 2,
            cache_write_tokens: 1,
            cost_micros: 17,
            tool_calls: 0,
        },
    );

    assert_eq!(usage.snapshot(UsageScope::Global).input_tokens, 10);
    assert_eq!(
        usage.snapshot(UsageScope::Tenant(tenant_id)).output_tokens,
        5
    );
    assert_eq!(
        usage.snapshot(UsageScope::Tenant(TenantId::new())),
        UsageSnapshot::default()
    );
}

#[test]
fn usage_accumulator_resets_one_scope_without_losing_global_total() {
    let usage = UsageAccumulator::default();
    let session_id = SessionId::new();

    usage.record(
        UsageScope::Session(session_id),
        None,
        UsageSnapshot {
            input_tokens: 3,
            output_tokens: 4,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 9,
            tool_calls: 0,
        },
    );
    usage.reset(UsageScope::Session(session_id));

    assert_eq!(
        usage.snapshot(UsageScope::Session(session_id)),
        UsageSnapshot::default()
    );
    assert_eq!(usage.snapshot(UsageScope::Global).cost_micros, 9);
}

#[test]
fn usage_accumulator_uses_pricing_snapshot_for_cost_calculation() {
    let usage = UsageAccumulator::builder()
        .with_cost_calculator(Arc::new(SnapshotCostCalculator))
        .build();
    let snapshot = PricingSnapshotId {
        pricing_id: "anthropic-2026-04".to_owned(),
        version: 7,
    };

    usage.record_with_pricing(
        UsageScope::Model("claude-sonnet".to_owned()),
        Some(ModelRef {
            provider_id: "anthropic".to_owned(),
            model_id: "claude-sonnet".to_owned(),
        }),
        Some(snapshot),
        UsageSnapshot {
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    );

    let model_usage = usage.snapshot(UsageScope::Model("claude-sonnet".to_owned()));
    assert_eq!(model_usage.cost_micros, 820);
    assert_eq!(usage.snapshot(UsageScope::Global).cost_micros, 820);
}

#[test]
fn default_usage_accumulator_uses_registered_pricing_table() {
    let usage = UsageAccumulator::default();
    let snapshot = PricingSnapshotId {
        pricing_id: "mock-pricing".to_owned(),
        version: 3,
    };
    usage.register_pricing(PricingTableEntry {
        pricing_id: snapshot.pricing_id.clone(),
        pricing_version: snapshot.version,
        input_per_million: Decimal::new(10, 0),
        output_per_million: Decimal::new(20, 0),
        cache_creation_per_million: None,
        cache_read_per_million: None,
        last_updated: harness_contracts::now(),
        source: PricingSource::BusinessProvided,
        billing_mode: PricingBillingMode::Standard,
    });

    usage.record_with_pricing(
        UsageScope::Global,
        Some(ModelRef {
            provider_id: "mock".to_owned(),
            model_id: "usage-model".to_owned(),
        }),
        Some(snapshot),
        UsageSnapshot {
            input_tokens: 11,
            output_tokens: 7,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    );

    assert_eq!(usage.snapshot(UsageScope::Global).cost_micros, 250);
}

#[test]
fn default_pricing_table_keeps_missing_pricing_as_none() {
    let usage = UsageAccumulator::default();
    usage.record_with_pricing(
        UsageScope::Global,
        Some(ModelRef {
            provider_id: "mock".to_owned(),
            model_id: "missing-pricing-model".to_owned(),
        }),
        None,
        UsageSnapshot {
            input_tokens: 11,
            output_tokens: 7,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    );

    assert_eq!(usage.snapshot(UsageScope::Global).cost_micros, 0);
}

struct SnapshotCostCalculator;

impl CostCalculator for SnapshotCostCalculator {
    fn calculator_id(&self) -> &'static str {
        "snapshot-test"
    }

    fn compute(
        &self,
        _model_ref: &ModelRef,
        pricing_snapshot_id: Option<&PricingSnapshotId>,
        usage: &UsageSnapshot,
    ) -> Option<UsageCost> {
        let snapshot = pricing_snapshot_id?;
        Some(UsageCost {
            cost_micros: usage.input_tokens
                + usage.output_tokens
                + u64::from(snapshot.version) * 100,
            pricing_snapshot_id: Some(snapshot.clone()),
        })
    }
}
