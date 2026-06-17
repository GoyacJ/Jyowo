use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use harness_contracts::*;
use harness_journal::*;

#[test]
fn event_store_trait_is_object_safe() {
    fn accepts_object_safe_trait(_store: Option<Arc<dyn EventStore>>) {}
    accepts_object_safe_trait(None);
}

struct CountingRedactor(Arc<AtomicUsize>);

impl Redactor for CountingRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        assert_eq!(rules.scope, RedactScope::EventBody);
        self.0.fetch_add(1, Ordering::SeqCst);
        input.replace("secret", &rules.replacement)
    }
}

#[test]
fn journal_redaction_uses_event_body_rules() {
    let calls = Arc::new(AtomicUsize::new(0));
    let redaction = JournalRedaction::new(Arc::new(CountingRedactor(Arc::clone(&calls))));

    assert_eq!(
        redaction.redact_event_field("token=secret"),
        "token=[REDACTED]"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct TestMigrator(SchemaVersion, SchemaVersion);

impl EventMigrator for TestMigrator {
    fn from_version(&self) -> SchemaVersion {
        self.0
    }

    fn to_version(&self) -> SchemaVersion {
        self.1
    }

    fn migrate(&self, envelope: EventEnvelope) -> Result<EventEnvelope, JournalError> {
        Ok(envelope)
    }
}

#[test]
fn migrator_chain_finds_shortest_path() {
    let chain = MigratorChain::new(vec![
        Box::new(TestMigrator(SchemaVersion::new(1), SchemaVersion::new(2))),
        Box::new(TestMigrator(SchemaVersion::new(2), SchemaVersion::new(4))),
        Box::new(TestMigrator(SchemaVersion::new(1), SchemaVersion::new(3))),
    ]);

    assert_eq!(
        chain
            .find_path(SchemaVersion::new(1), SchemaVersion::new(4))
            .unwrap()
            .len(),
        2
    );
    assert!(chain
        .find_path(SchemaVersion::new(4), SchemaVersion::new(1))
        .is_none());
}

#[test]
fn journal_reexports_projection_snapshot_and_blob_contracts() {
    fn assert_blob_store<T: ?Sized + BlobStore>() {}
    fn assert_projection<P: Projection>() {}

    assert_blob_store::<dyn BlobStore>();
    assert_projection::<SessionProjection>();
}
