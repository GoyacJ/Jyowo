use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use harness_contracts::{ModelRef, PricingSnapshotId, RunId, SessionId, TenantId, UsageSnapshot};
use parking_lot::RwLock;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

const NOOP_CALCULATOR_ID: &str = "noop";
const PRICING_TABLE_CALCULATOR_ID: &str = "pricing-table";

pub trait CostCalculator: Send + Sync + 'static {
    fn calculator_id(&self) -> &str;

    fn compute(
        &self,
        model_ref: &ModelRef,
        pricing_snapshot_id: Option<&PricingSnapshotId>,
        usage: &UsageSnapshot,
    ) -> Option<UsageCost>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UsageCost {
    pub cost_micros: u64,
    pub pricing_snapshot_id: Option<PricingSnapshotId>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopCostCalculator;

impl CostCalculator for NoopCostCalculator {
    fn calculator_id(&self) -> &str {
        NOOP_CALCULATOR_ID
    }

    fn compute(
        &self,
        _model_ref: &ModelRef,
        _pricing_snapshot_id: Option<&PricingSnapshotId>,
        _usage: &UsageSnapshot,
    ) -> Option<UsageCost> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct PricingTableCostCalculator {
    pricing: Arc<RwLock<HashMap<(String, u32), PricingTableEntry>>>,
}

impl PricingTableCostCalculator {
    #[must_use]
    pub fn new(pricing: Vec<PricingTableEntry>) -> Self {
        let table = Self::default();
        for entry in pricing {
            table.upsert(entry);
        }
        table
    }

    pub fn upsert(&self, entry: PricingTableEntry) {
        self.pricing
            .write()
            .insert((entry.pricing_id.clone(), entry.pricing_version), entry);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pricing.read().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for PricingTableCostCalculator {
    fn default() -> Self {
        Self {
            pricing: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl CostCalculator for PricingTableCostCalculator {
    fn calculator_id(&self) -> &str {
        PRICING_TABLE_CALCULATOR_ID
    }

    fn compute(
        &self,
        _model_ref: &ModelRef,
        pricing_snapshot_id: Option<&PricingSnapshotId>,
        usage: &UsageSnapshot,
    ) -> Option<UsageCost> {
        let snapshot = pricing_snapshot_id?;
        let pricing = self
            .pricing
            .read()
            .get(&(snapshot.pricing_id.clone(), snapshot.version))
            .cloned()?;

        let input = token_cost_micros(
            usage.input_tokens,
            input_rate(&pricing, usage.input_tokens),
            &pricing,
        );
        let output = token_cost_micros(usage.output_tokens, pricing.output_per_million, &pricing);
        let cache_write = token_cost_micros(
            usage.cache_write_tokens,
            pricing
                .cache_creation_per_million
                .unwrap_or(pricing.input_per_million),
            &pricing,
        );
        let cache_read =
            token_cost_micros(usage.cache_read_tokens, cache_read_rate(&pricing), &pricing);

        Some(UsageCost {
            cost_micros: input
                .saturating_add(output)
                .saturating_add(cache_write)
                .saturating_add(cache_read),
            pricing_snapshot_id: Some(snapshot.clone()),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricingTableEntry {
    pub pricing_id: String,
    pub pricing_version: u32,
    pub input_per_million: Decimal,
    pub output_per_million: Decimal,
    pub cache_creation_per_million: Option<Decimal>,
    pub cache_read_per_million: Option<Decimal>,
    pub last_updated: DateTime<Utc>,
    pub source: PricingSource,
    pub billing_mode: PricingBillingMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PricingSource {
    Hardcoded,
    ProviderApi,
    ManualOverride,
    BusinessProvided,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PricingBillingMode {
    Standard,
    Cached { cache_read_discount: Ratio },
    Batched { discount: Ratio },
    Tiered { thresholds: Vec<(u64, Decimal)> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratio(pub f32);

#[derive(Clone)]
pub struct UsageAccumulator {
    inner: Arc<RwLock<UsageState>>,
    cost_calculator: Arc<dyn CostCalculator>,
    pricing_table: Option<PricingTableCostCalculator>,
}

impl UsageAccumulator {
    #[must_use]
    pub fn builder() -> UsageAccumulatorBuilder {
        UsageAccumulatorBuilder::default()
    }

    pub fn record(&self, scope: UsageScope, model_ref: Option<ModelRef>, delta: UsageSnapshot) {
        self.record_with_pricing(scope, model_ref, None, delta);
    }

    pub fn record_with_pricing(
        &self,
        scope: UsageScope,
        model_ref: Option<ModelRef>,
        pricing_snapshot_id: Option<PricingSnapshotId>,
        delta: UsageSnapshot,
    ) {
        self.record_scopes_with_pricing([scope], model_ref, pricing_snapshot_id, delta);
    }

    pub fn record_scopes_with_pricing<I>(
        &self,
        scopes: I,
        model_ref: Option<ModelRef>,
        pricing_snapshot_id: Option<PricingSnapshotId>,
        mut delta: UsageSnapshot,
    ) where
        I: IntoIterator<Item = UsageScope>,
    {
        if let Some(model_ref) = &model_ref {
            if let Some(cost) =
                self.cost_calculator
                    .compute(model_ref, pricing_snapshot_id.as_ref(), &delta)
            {
                delta.cost_micros = cost.cost_micros;
            }
        }

        let mut state = self.inner.write();
        add_usage(&mut state.global, &delta);
        for scope in scopes {
            match scope {
                UsageScope::Global => {}
                UsageScope::Tenant(tenant_id) => {
                    add_usage(state.by_tenant.entry(tenant_id).or_default(), &delta);
                }
                UsageScope::Session(session_id) => {
                    add_usage(state.by_session.entry(session_id).or_default(), &delta);
                }
                UsageScope::Run(run_id) => {
                    add_usage(state.by_run.entry(run_id).or_default(), &delta);
                }
                UsageScope::Model(model_id) => {
                    add_usage(state.by_model.entry(model_id).or_default(), &delta);
                }
            }
        }
    }

    pub fn register_pricing(&self, pricing: PricingTableEntry) {
        if let Some(table) = &self.pricing_table {
            table.upsert(pricing);
        }
    }

    #[must_use]
    pub fn compute_cost(
        &self,
        model_ref: &ModelRef,
        pricing_snapshot_id: Option<&PricingSnapshotId>,
        usage: &UsageSnapshot,
    ) -> Option<UsageCost> {
        self.cost_calculator
            .compute(model_ref, pricing_snapshot_id, usage)
    }

    #[must_use]
    pub fn snapshot(&self, scope: UsageScope) -> UsageSnapshot {
        let state = self.inner.read();
        match scope {
            UsageScope::Global => Some(state.global.clone()),
            UsageScope::Tenant(tenant_id) => state.by_tenant.get(&tenant_id).cloned(),
            UsageScope::Session(session_id) => state.by_session.get(&session_id).cloned(),
            UsageScope::Run(run_id) => state.by_run.get(&run_id).cloned(),
            UsageScope::Model(model_id) => state.by_model.get(&model_id).cloned(),
        }
        .unwrap_or_default()
    }

    pub fn reset(&self, scope: UsageScope) {
        let mut state = self.inner.write();
        match scope {
            UsageScope::Global => state.global = UsageSnapshot::default(),
            UsageScope::Tenant(tenant_id) => {
                state.by_tenant.remove(&tenant_id);
            }
            UsageScope::Session(session_id) => {
                state.by_session.remove(&session_id);
            }
            UsageScope::Run(run_id) => {
                state.by_run.remove(&run_id);
            }
            UsageScope::Model(model_id) => {
                state.by_model.remove(&model_id);
            }
        }
    }

    #[must_use]
    pub fn report(&self) -> UsageReport {
        let state = self.inner.read();
        UsageReport {
            global: state.global.clone(),
            tenants: state.by_tenant.clone(),
            sessions: state.by_session.clone(),
            runs: state.by_run.clone(),
            models: state.by_model.clone(),
        }
    }
}

impl Default for UsageAccumulator {
    fn default() -> Self {
        Self::builder().build()
    }
}

#[derive(Clone, Default)]
pub struct UsageAccumulatorBuilder {
    cost_calculator: Option<Arc<dyn CostCalculator>>,
}

impl UsageAccumulatorBuilder {
    #[must_use]
    pub fn with_cost_calculator(mut self, calculator: Arc<dyn CostCalculator>) -> Self {
        self.cost_calculator = Some(calculator);
        self
    }

    #[must_use]
    pub fn build(self) -> UsageAccumulator {
        let pricing_table = self
            .cost_calculator
            .is_none()
            .then(PricingTableCostCalculator::default);
        let cost_calculator = self.cost_calculator.unwrap_or_else(|| {
            Arc::new(pricing_table.clone().expect("pricing table exists"))
                as Arc<dyn CostCalculator>
        });
        UsageAccumulator {
            inner: Arc::new(RwLock::new(UsageState::default())),
            cost_calculator,
            pricing_table,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum UsageScope {
    Global,
    Tenant(TenantId),
    Session(SessionId),
    Run(RunId),
    Model(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageReport {
    pub global: UsageSnapshot,
    pub tenants: HashMap<TenantId, UsageSnapshot>,
    pub sessions: HashMap<SessionId, UsageSnapshot>,
    pub runs: HashMap<RunId, UsageSnapshot>,
    pub models: HashMap<String, UsageSnapshot>,
}

#[derive(Clone, Default)]
pub struct ModelMetricsAccumulator {
    inner: Arc<RwLock<ModelMetricsState>>,
}

impl ModelMetricsAccumulator {
    pub fn record_infer(&self, model: impl AsRef<str>, duration: Duration, usage: &UsageSnapshot) {
        let mut state = self.inner.write();
        let metrics = state.by_model.entry(model.as_ref().to_owned()).or_default();
        metrics.infer_duration_ms = metrics
            .infer_duration_ms
            .saturating_add(duration.as_millis().min(u128::from(u64::MAX)) as u64);
        metrics.infer_total = metrics.infer_total.saturating_add(1);
        metrics.input_tokens = metrics.input_tokens.saturating_add(usage.input_tokens);
        metrics.output_tokens = metrics.output_tokens.saturating_add(usage.output_tokens);
        metrics.cache_creation_tokens = metrics
            .cache_creation_tokens
            .saturating_add(usage.cache_write_tokens);
        metrics.cache_read_tokens = metrics
            .cache_read_tokens
            .saturating_add(usage.cache_read_tokens);
    }

    pub fn record_credential_pool_cooldown(&self, model: impl AsRef<str>) {
        let mut state = self.inner.write();
        let metrics = state.by_model.entry(model.as_ref().to_owned()).or_default();
        metrics.credential_pool_cooldowns_total =
            metrics.credential_pool_cooldowns_total.saturating_add(1);
    }

    pub fn record_model_error(&self, model: impl AsRef<str>, class: impl AsRef<str>) {
        let mut state = self.inner.write();
        let model = model.as_ref().to_owned();
        state.by_model.entry(model.clone()).or_default();
        *state
            .model_errors
            .entry(ModelErrorKey {
                model,
                class: class.as_ref().to_owned(),
            })
            .or_default() += 1;
    }

    pub fn record_stream_error(&self, model: impl AsRef<str>, class: impl AsRef<str>) {
        let mut state = self.inner.write();
        let model = model.as_ref().to_owned();
        state.by_model.entry(model.clone()).or_default();
        *state
            .stream_errors
            .entry(ModelErrorKey {
                model,
                class: class.as_ref().to_owned(),
            })
            .or_default() += 1;
    }

    pub fn record_aux_queue_wait(&self, model: impl AsRef<str>, duration: Duration) {
        let mut state = self.inner.write();
        let metrics = state.by_model.entry(model.as_ref().to_owned()).or_default();
        metrics.aux_queue_wait_ms = metrics
            .aux_queue_wait_ms
            .saturating_add(duration.as_millis().min(u128::from(u64::MAX)) as u64);
        metrics.aux_queue_wait_total = metrics.aux_queue_wait_total.saturating_add(1);
    }

    #[must_use]
    pub fn report(&self) -> ModelMetricsReport {
        let state = self.inner.read();
        ModelMetricsReport {
            models: state.by_model.clone(),
            model_errors: state.model_errors.clone(),
            stream_errors: state.stream_errors.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelMetricsReport {
    pub models: HashMap<String, ModelMetricsSnapshot>,
    pub model_errors: HashMap<ModelErrorKey, u64>,
    pub stream_errors: HashMap<ModelErrorKey, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelMetricsSnapshot {
    pub infer_duration_ms: u64,
    pub infer_total: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub credential_pool_cooldowns_total: u64,
    pub aux_queue_wait_ms: u64,
    pub aux_queue_wait_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelErrorKey {
    pub model: String,
    pub class: String,
}

#[derive(Debug, Clone, Default)]
struct ModelMetricsState {
    by_model: HashMap<String, ModelMetricsSnapshot>,
    model_errors: HashMap<ModelErrorKey, u64>,
    stream_errors: HashMap<ModelErrorKey, u64>,
}

#[derive(Debug, Clone, Default)]
struct UsageState {
    global: UsageSnapshot,
    by_tenant: HashMap<TenantId, UsageSnapshot>,
    by_session: HashMap<SessionId, UsageSnapshot>,
    by_run: HashMap<RunId, UsageSnapshot>,
    by_model: HashMap<String, UsageSnapshot>,
}

fn add_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
}

fn token_cost_micros(tokens: u64, rate_per_million: Decimal, pricing: &PricingTableEntry) -> u64 {
    if tokens == 0 {
        return 0;
    }

    let rate = apply_batch_discount(rate_per_million, pricing);
    (Decimal::from(tokens) * rate * Decimal::from(1_000_000_u64) / Decimal::from(1_000_000_u64))
        .round()
        .to_u64()
        .unwrap_or(u64::MAX)
}

fn input_rate(pricing: &PricingTableEntry, input_tokens: u64) -> Decimal {
    match &pricing.billing_mode {
        PricingBillingMode::Tiered { thresholds } => thresholds
            .iter()
            .filter(|(threshold, _)| input_tokens >= *threshold)
            .max_by_key(|(threshold, _)| *threshold)
            .map(|(_, rate)| *rate)
            .unwrap_or(pricing.input_per_million),
        _ => pricing.input_per_million,
    }
}

fn cache_read_rate(pricing: &PricingTableEntry) -> Decimal {
    match &pricing.billing_mode {
        PricingBillingMode::Cached {
            cache_read_discount,
        } => pricing
            .cache_read_per_million
            .unwrap_or_else(|| pricing.input_per_million * ratio(*cache_read_discount)),
        _ => pricing
            .cache_read_per_million
            .unwrap_or(pricing.input_per_million),
    }
}

fn apply_batch_discount(rate: Decimal, pricing: &PricingTableEntry) -> Decimal {
    match pricing.billing_mode {
        PricingBillingMode::Batched { discount } => rate * ratio(discount),
        _ => rate,
    }
}

fn ratio(value: Ratio) -> Decimal {
    Decimal::from_f32_retain(value.0).unwrap_or(Decimal::ZERO)
}
