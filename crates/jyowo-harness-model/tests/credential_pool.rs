use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{CredentialPoolSharedAcrossTenantsEvent, ModelError, TenantId};
use harness_model::*;
use parking_lot::Mutex;
use secrecy::SecretString;

#[derive(Default)]
struct Source {
    seen: Mutex<Vec<CredentialKey>>,
}

#[async_trait::async_trait]
impl CredentialSource for Source {
    async fn fetch(&self, key: CredentialKey) -> Result<CredentialValue, CredentialError> {
        self.seen.lock().push(key.clone());
        Ok(CredentialValue {
            secret: SecretString::new(key.key_label.clone().into_boxed_str()),
            metadata: CredentialMetadata::default(),
        })
    }

    async fn rotate(&self, _key: CredentialKey) -> Result<(), CredentialError> {
        Ok(())
    }
}

#[derive(Default)]
struct Audit {
    events: Mutex<Vec<CredentialPoolSharedAcrossTenantsEvent>>,
}

impl CredentialPoolAuditSink for Audit {
    fn record_shared_across_tenants(&self, event: CredentialPoolSharedAcrossTenantsEvent) {
        self.events.lock().push(event);
    }
}

fn key(label: &str) -> CredentialKey {
    CredentialKey {
        tenant_id: TenantId::SINGLE,
        provider_id: "anthropic".to_owned(),
        key_label: label.to_owned(),
    }
}

#[tokio::test]
async fn fill_first_picks_first_available_key() {
    let pool = CredentialPool::builder()
        .strategy(PoolStrategy::FillFirst)
        .add_source(Arc::new(Source::default()))
        .build();

    let picked = pool.pick(&[key("primary"), key("backup")]).await.unwrap();

    assert_eq!(picked.key.key_label, "primary");
}

#[tokio::test]
async fn round_robin_and_least_used_advance_between_available_keys() {
    let round = CredentialPool::builder()
        .strategy(PoolStrategy::RoundRobin)
        .add_source(Arc::new(Source::default()))
        .build();

    assert_eq!(
        round
            .pick(&[key("a"), key("b")])
            .await
            .unwrap()
            .key
            .key_label,
        "a"
    );
    assert_eq!(
        round
            .pick(&[key("a"), key("b")])
            .await
            .unwrap()
            .key
            .key_label,
        "b"
    );

    let least = CredentialPool::builder()
        .strategy(PoolStrategy::LeastUsed)
        .add_source(Arc::new(Source::default()))
        .build();

    assert_eq!(
        least
            .pick(&[key("a"), key("b")])
            .await
            .unwrap()
            .key
            .key_label,
        "a"
    );
    assert_eq!(
        least
            .pick(&[key("a"), key("b")])
            .await
            .unwrap()
            .key
            .key_label,
        "b"
    );
}

#[tokio::test]
async fn random_strategy_picks_an_available_key_without_extra_dependencies() {
    let pool = CredentialPool::builder()
        .strategy(PoolStrategy::Random)
        .add_source(Arc::new(Source::default()))
        .build();

    let picked = pool.pick(&[key("a"), key("b")]).await.unwrap();

    assert!(["a", "b"].contains(&picked.key.key_label.as_str()));
}

#[tokio::test]
async fn cooldown_and_ban_are_scoped_by_full_credential_key() {
    let pool = CredentialPool::builder()
        .strategy(PoolStrategy::FillFirst)
        .add_source(Arc::new(Source::default()))
        .build();
    let primary = key("primary");
    let backup = key("backup");

    pool.mark_rate_limited(&primary, Duration::from_secs(60));
    assert_eq!(
        pool.pick(&[primary.clone(), backup.clone()])
            .await
            .unwrap()
            .key,
        backup
    );

    pool.mark_banned(&backup);
    let error = pool.pick(&[primary, backup]).await.unwrap_err();

    assert!(matches!(
        error,
        CredentialError::Model(ModelError::AllCredentialsBanned)
    ));
}

#[tokio::test]
async fn mark_rate_limited_records_cooldown_metric_for_model_context() {
    let metrics = Arc::new(RecordingModelMetricsSink::default());
    let pool = Arc::new(
        CredentialPool::builder()
            .strategy(PoolStrategy::FillFirst)
            .add_source(Arc::new(Source::default()))
            .metrics_sink(metrics.clone())
            .build(),
    );
    let resolver = CredentialPoolResolver::new(pool, ["primary"]);
    let context = ModelCredentialPickContext {
        tenant_id: TenantId::SINGLE,
        provider_id: "anthropic".to_owned(),
        model_id: "claude-test".to_owned(),
    };

    let picked = resolver.pick(context).await.unwrap();
    resolver.mark_rate_limited(&picked.key, Duration::from_secs(60));

    assert_eq!(metrics.cooldowns.lock().clone(), vec!["claude-test"]);
}

#[tokio::test]
async fn shared_tenant_key_emits_audit_event_once() {
    let audit = Arc::new(Audit::default());
    let pool = CredentialPool::builder()
        .strategy(PoolStrategy::FillFirst)
        .add_source(Arc::new(Source::default()))
        .audit_sink(audit.clone())
        .build();
    let shared = CredentialKey {
        tenant_id: TenantId::SHARED,
        provider_id: "anthropic".to_owned(),
        key_label: "shared".to_owned(),
    };

    pool.pick(std::slice::from_ref(&shared)).await.unwrap();
    pool.pick(&[shared]).await.unwrap();

    let events = audit.events.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tenant_id, TenantId::SHARED);
    assert_eq!(events[0].provider_id, "anthropic");
    assert_ne!(events[0].credential_key_hash, [0; 32]);
}

#[derive(Default)]
struct RecordingModelMetricsSink {
    cooldowns: Mutex<Vec<String>>,
}

impl ModelMetricsSink for RecordingModelMetricsSink {
    fn record_credential_pool_cooldown(&self, model_id: &str) {
        self.cooldowns.lock().push(model_id.to_owned());
    }

    fn record_aux_queue_wait(&self, _model_id: &str, _duration: Duration) {}
}
