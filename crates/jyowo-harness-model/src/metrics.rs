use std::time::Duration;

pub trait ModelMetricsSink: Send + Sync + 'static {
    fn record_credential_pool_cooldown(&self, _model_id: &str) {}

    fn record_aux_queue_wait(&self, _model_id: &str, _duration: Duration) {}
}

#[derive(Default)]
pub struct NoopModelMetricsSink;

impl ModelMetricsSink for NoopModelMetricsSink {}
