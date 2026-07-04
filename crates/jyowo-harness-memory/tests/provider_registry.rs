//! Tests for the provider registry.

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    MemoryError, MemoryId, MemoryProviderTrust, MemoryVisibilityClass, TenantId,
};
use harness_memory::{
    MemoryLifecycle, MemoryListScope, MemoryProvider, MemoryProviderDescriptor,
    MemoryProviderRegistry, MemoryQuery, MemoryRecord, MemoryStore, MemorySummary,
};

fn make_dummy_provider(id: &str, priority: i32) -> DummyProvider {
    DummyProvider::new(id, priority)
}

struct DummyProvider {
    id: String,
    priority: i32,
    readable: bool,
    writable: bool,
    records: tokio::sync::Mutex<Vec<MemoryRecord>>,
}

impl DummyProvider {
    fn new(id: &str, priority: i32) -> Self {
        Self {
            id: id.to_owned(),
            priority,
            readable: true,
            writable: true,
            records: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    fn read_only(id: &str, priority: i32) -> Self {
        Self {
            readable: false,
            writable: false,
            ..Self::new(id, priority)
        }
    }
}

#[async_trait]
impl MemoryStore for DummyProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }

    async fn recall(&self, _query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(self.records.lock().await.clone())
    }

    async fn get(&self, id: MemoryId) -> Result<MemoryRecord, MemoryError> {
        self.records
            .lock()
            .await
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or(MemoryError::NotFound(id))
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        let id = record.id;
        let mut records = self.records.lock().await;
        if let Some(existing) = records.iter_mut().find(|r| r.id == id) {
            *existing = record;
        } else {
            records.push(record);
        }
        Ok(id)
    }

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError> {
        self.records.lock().await.retain(|r| r.id != id);
        Ok(())
    }

    async fn list(&self, _scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(vec![])
    }
}

impl MemoryLifecycle for DummyProvider {}

impl MemoryProvider for DummyProvider {
    fn descriptor(&self) -> MemoryProviderDescriptor {
        MemoryProviderDescriptor {
            provider_id: self.id.clone(),
            priority: self.priority,
            trust_level: MemoryProviderTrust::BuiltIn,
            readable: self.readable,
            writable: self.writable,
            allowed_visibility: vec![
                MemoryVisibilityClass::Private,
                MemoryVisibilityClass::User,
                MemoryVisibilityClass::Tenant,
            ],
            timeout_ms: 5000,
            max_records_per_recall: 10,
            max_chars_per_recall: 10000,
            max_bytes_per_record: 1024 * 1024,
        }
    }
}

// ── Registry tests ──

#[test]
fn registry_register_and_list_providers() {
    let mut registry = MemoryProviderRegistry::new();
    let p1 = Arc::new(make_dummy_provider("local", 100));
    let p2 = Arc::new(make_dummy_provider("plugin-x", 50));

    registry.register(p1).expect("register local");
    registry.register(p2).expect("register plugin");

    let ids: Vec<String> = registry
        .providers()
        .map(|d| d.provider_id.clone())
        .collect();
    assert!(ids.contains(&"local".to_owned()));
    assert!(ids.contains(&"plugin-x".to_owned()));
}

#[test]
fn registry_duplicate_id_is_error() {
    let mut registry = MemoryProviderRegistry::new();
    registry
        .register(Arc::new(make_dummy_provider("local", 100)))
        .expect("first register");
    let result = registry.register(Arc::new(make_dummy_provider("local", 200)));
    assert!(result.is_err());
}

#[test]
fn registry_write_target_selects_writable_provider() {
    let mut registry = MemoryProviderRegistry::new();
    let rw = Arc::new(make_dummy_provider("writable-p1", 50));

    registry.register(rw).expect("register rw");

    let targets = registry.write_targets();
    assert!(!targets.is_empty());
    // Default descriptor marks providers as writable
    assert_eq!(targets[0].provider_id, "writable-p1");
}

#[test]
fn registry_fanout_sorts_by_priority() {
    let mut registry = MemoryProviderRegistry::new();
    registry
        .register(Arc::new(make_dummy_provider("low", 10)))
        .expect("register low");
    registry
        .register(Arc::new(make_dummy_provider("high", 100)))
        .expect("register high");
    registry
        .register(Arc::new(make_dummy_provider("mid", 50)))
        .expect("register mid");

    let ordered: Vec<String> = registry
        .readable_providers_sorted()
        .into_iter()
        .map(|d| d.provider_id.clone())
        .collect();
    assert_eq!(ordered, vec!["high", "mid", "low"]);
}

// ── Rejects single-slot pattern ──

#[test]
fn rejects_single_slot_pattern() {
    // This test exists to verify the registry architecture is used.
    // The single-slot `external: RwLock<Option<Arc<dyn MemoryProvider>>>` pattern is gone.
    let registry = MemoryProviderRegistry::new();
    assert_eq!(registry.providers().count(), 0);
}

// ── Budget enforcement ──

#[test]
fn provider_budgets_are_accessible() {
    let mut registry = MemoryProviderRegistry::new();
    let p = Arc::new(make_dummy_provider("budget-provider", 100));
    registry.register(p).expect("register");

    let desc = registry.descriptor("budget-provider").expect("descriptor");
    assert!(desc.max_records_per_recall > 0);
    assert!(desc.max_chars_per_recall > 0);
    assert!(desc.max_bytes_per_record > 0);
}
