#![cfg(feature = "in-memory")]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::*;
use harness_journal::*;

fn event(text: &str) -> Event {
    Event::UnexpectedError(UnexpectedErrorEvent {
        session_id: None,
        run_id: None,
        error: text.to_owned(),
        at: harness_contracts::now(),
    })
}

struct ErrorTextMigrator;

impl EventMigrator for ErrorTextMigrator {
    fn from_version(&self) -> SchemaVersion {
        SchemaVersion::new(0)
    }

    fn to_version(&self) -> SchemaVersion {
        SchemaVersion::CURRENT
    }

    fn migrate(&self, mut envelope: EventEnvelope) -> Result<EventEnvelope, JournalError> {
        envelope.schema_version = SchemaVersion::CURRENT;
        if let Event::UnexpectedError(error) = &mut envelope.payload {
            error.error = format!("migrated:{}", error.error);
        }
        Ok(envelope)
    }
}

#[tokio::test]
async fn versioned_store_applies_read_migrators_to_envelopes_and_events() {
    let inner = InMemoryEventStore::new(Arc::new(NoopRedactor));
    let session = SessionId::new();
    inner
        .append(TenantId::SINGLE, session, &[event("old")])
        .await
        .expect("append succeeds");
    inner
        .rewrite_schema_version_for_test(TenantId::SINGLE, session, SchemaVersion::new(0))
        .await
        .expect("schema rewrite succeeds");

    let store = VersionedEventStore::builder(inner)
        .with_migrator(ErrorTextMigrator)
        .build();

    let envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read envelopes succeeds")
        .collect()
        .await;
    assert_eq!(envelopes[0].schema_version, SchemaVersion::CURRENT);
    assert!(matches!(
        &envelopes[0].payload,
        Event::UnexpectedError(UnexpectedErrorEvent { error, .. }) if error == "migrated:old"
    ));

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("read succeeds")
        .collect()
        .await;
    assert!(matches!(
        &events[0],
        Event::UnexpectedError(UnexpectedErrorEvent { error, .. }) if error == "migrated:old"
    ));
}

#[tokio::test]
async fn versioned_store_strict_mode_errors_on_missing_migration_path() {
    let inner = InMemoryEventStore::new(Arc::new(NoopRedactor));
    let session = SessionId::new();
    inner
        .append(TenantId::SINGLE, session, &[event("old")])
        .await
        .expect("append succeeds");
    inner
        .rewrite_schema_version_for_test(TenantId::SINGLE, session, SchemaVersion::new(0))
        .await
        .expect("schema rewrite succeeds");

    let store = VersionedEventStore::builder(inner).build();
    let error = store
        .read_envelopes(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .err()
        .expect("missing migration path fails");
    assert!(error.to_string().contains("migration path missing"));
}

#[tokio::test]
async fn versioned_store_non_strict_mode_skips_unmigratable_envelopes() {
    let inner = InMemoryEventStore::new(Arc::new(NoopRedactor));
    let session = SessionId::new();
    inner
        .append(TenantId::SINGLE, session, &[event("old")])
        .await
        .expect("append succeeds");
    inner
        .rewrite_schema_version_for_test(TenantId::SINGLE, session, SchemaVersion::new(0))
        .await
        .expect("schema rewrite succeeds");

    let store = VersionedEventStore::builder(inner).strict(false).build();
    let envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session, ReplayCursor::FromStart)
        .await
        .expect("non strict read succeeds")
        .collect()
        .await;
    assert!(envelopes.is_empty());
}
