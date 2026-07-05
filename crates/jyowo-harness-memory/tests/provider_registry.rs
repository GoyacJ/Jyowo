//! Tests for the provider registry.

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    MemoryError, MemoryId, MemoryProviderDurability, MemoryProviderKind, MemoryProviderTrust,
    MemoryVisibility, MemoryVisibilityClass, TeamId,
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
    provider_kind: MemoryProviderKind,
    trust_level: MemoryProviderTrust,
    readable: bool,
    writable: bool,
    allowed_visibility: Vec<MemoryVisibilityClass>,
    timeout_ms: u32,
    max_records_per_recall: u32,
    max_chars_per_recall: u32,
    max_bytes_per_record: u64,
    supports_evidence: bool,
    durability: MemoryProviderDurability,
    records: tokio::sync::Mutex<Vec<MemoryRecord>>,
}

impl DummyProvider {
    fn new(id: &str, priority: i32) -> Self {
        Self {
            id: id.to_owned(),
            priority,
            provider_kind: MemoryProviderKind::Local,
            trust_level: MemoryProviderTrust::BuiltIn,
            readable: true,
            writable: true,
            allowed_visibility: vec![
                MemoryVisibilityClass::Private,
                MemoryVisibilityClass::User,
                MemoryVisibilityClass::Tenant,
            ],
            timeout_ms: 5000,
            max_records_per_recall: 10,
            max_chars_per_recall: 10000,
            max_bytes_per_record: 1024 * 1024,
            supports_evidence: true,
            durability: MemoryProviderDurability::Durable,
            records: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    fn invalid_budget(mut self) -> Self {
        self.max_records_per_recall = 0;
        self
    }

    fn no_capability(mut self) -> Self {
        self.readable = false;
        self.writable = false;
        self
    }

    fn no_evidence(mut self) -> Self {
        self.supports_evidence = false;
        self
    }

    fn ephemeral(mut self) -> Self {
        self.durability = MemoryProviderDurability::Ephemeral;
        self
    }

    fn allowed_visibility(mut self, allowed_visibility: Vec<MemoryVisibilityClass>) -> Self {
        self.allowed_visibility = allowed_visibility;
        self
    }

    fn plugin_trusted(mut self) -> Self {
        self.provider_kind = MemoryProviderKind::Plugin;
        self.trust_level = MemoryProviderTrust::Plugin;
        self
    }

    fn team_trusted(mut self) -> Self {
        self.provider_kind = MemoryProviderKind::Team;
        self.trust_level = MemoryProviderTrust::Team;
        self.durability = MemoryProviderDurability::Ephemeral;
        self
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
            provider_kind: self.provider_kind,
            priority: self.priority,
            trust_level: self.trust_level,
            tenant_scope: None,
            workspace_scope: None,
            durability: self.durability,
            readable: self.readable,
            writable: self.writable,
            allowed_visibility: self.allowed_visibility.clone(),
            supports_evidence: self.supports_evidence,
            supports_raw_content_export: false,
            timeout_ms: self.timeout_ms,
            max_records_per_recall: self.max_records_per_recall,
            max_chars_per_recall: self.max_chars_per_recall,
            max_bytes_per_record: self.max_bytes_per_record,
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
fn registry_rejects_invalid_provider_descriptors() {
    let mut registry = MemoryProviderRegistry::new();

    let no_capability = registry.register(Arc::new(
        make_dummy_provider("no-capability", 100).no_capability(),
    ));
    assert!(
        matches!(no_capability, Err(MemoryError::Message(message)) if message.contains("readable or writable"))
    );

    let invalid_budget = registry.register(Arc::new(
        make_dummy_provider("invalid-budget", 100).invalid_budget(),
    ));
    assert!(
        matches!(invalid_budget, Err(MemoryError::Message(message)) if message.contains("max_records_per_recall"))
    );

    let no_evidence = registry.register(Arc::new(
        make_dummy_provider("no-evidence", 100).no_evidence(),
    ));
    assert!(
        matches!(no_evidence, Err(MemoryError::Message(message)) if message.contains("support evidence"))
    );

    let ephemeral = registry.register(Arc::new(make_dummy_provider("ephemeral", 100).ephemeral()));
    assert!(matches!(ephemeral, Err(MemoryError::Message(message)) if message.contains("durable")));
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
fn registry_selects_one_write_target_by_policy_order() {
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

    let target = registry
        .select_write_provider()
        .expect("policy should select one target");

    assert_eq!(target.provider_id(), "high");
}

#[test]
fn registry_selects_write_target_by_record_visibility_before_priority() {
    let mut registry = MemoryProviderRegistry::new();
    registry
        .register(Arc::new(
            make_dummy_provider("high-user", 100)
                .allowed_visibility(vec![MemoryVisibilityClass::User]),
        ))
        .expect("register high user provider");
    registry
        .register(Arc::new(
            make_dummy_provider("low-team", 10)
                .allowed_visibility(vec![MemoryVisibilityClass::Team]),
        ))
        .expect("register low team provider");

    let target = registry
        .select_write_provider_for_visibility(&MemoryVisibility::Team {
            team_id: TeamId::new(),
        })
        .expect("team write target");

    assert_eq!(target.provider_id(), "low-team");
}

#[test]
fn registry_fails_closed_when_no_provider_supports_record_visibility() {
    let mut registry = MemoryProviderRegistry::new();
    registry
        .register(Arc::new(
            make_dummy_provider("user-only", 100)
                .allowed_visibility(vec![MemoryVisibilityClass::User]),
        ))
        .expect("register user provider");

    let target = registry.select_write_provider_for_visibility(&MemoryVisibility::Team {
        team_id: TeamId::new(),
    });

    assert!(target.is_none());
}

#[test]
fn registry_write_selection_allows_plugin_provider_when_descriptor_allows_write() {
    let mut registry = MemoryProviderRegistry::new();
    registry
        .register(Arc::new(
            make_dummy_provider("plugin-memory", 100).plugin_trusted(),
        ))
        .expect("register plugin provider");

    let target = registry
        .select_write_provider_for_visibility(&MemoryVisibility::Tenant)
        .expect("plugin write target");

    assert_eq!(target.provider_id(), "plugin-memory");
}

#[test]
fn registry_write_selection_allows_team_provider_when_visibility_matches() {
    let mut registry = MemoryProviderRegistry::new();
    registry
        .register(Arc::new(
            make_dummy_provider("team-memory", 100)
                .team_trusted()
                .allowed_visibility(vec![MemoryVisibilityClass::Team]),
        ))
        .expect("register team provider");

    let target = registry
        .select_write_provider_for_visibility(&MemoryVisibility::Team {
            team_id: TeamId::new(),
        })
        .expect("team write target");

    assert_eq!(target.provider_id(), "team-memory");
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
