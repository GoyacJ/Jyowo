#![cfg(feature = "redactor")]

use std::{path::PathBuf, sync::Arc};

use futures::StreamExt;
use harness_contracts::{Event, TenantId, UnexpectedErrorEvent};
use harness_journal::{EventStore, InMemoryEventStore, JsonlEventStore, SqliteEventStore};
use harness_observability::DefaultRedactor;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jyowo-observability-redactor-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("root created");
    root
}

fn secret_event() -> Event {
    Event::UnexpectedError(UnexpectedErrorEvent {
        session_id: None,
        run_id: None,
        error: "token sk-abcdefghijklmnopqrstuvwxyz and Bearer abcdefghijklmnopqrstuvwxyz"
            .to_owned(),
        at: harness_contracts::now(),
    })
}

#[tokio::test]
async fn default_redactor_runs_before_jsonl_store_persists_events() {
    let root = temp_root("jsonl");
    let store = JsonlEventStore::open(&root, Arc::new(DefaultRedactor::default()))
        .await
        .expect("jsonl opens");

    assert_store_redacts_on_append(&store).await;
}

#[tokio::test]
async fn default_redactor_runs_before_sqlite_store_persists_events() {
    let root = temp_root("sqlite");
    let store =
        SqliteEventStore::open(root.join("events.db"), Arc::new(DefaultRedactor::default()))
            .await
            .expect("sqlite opens");

    assert_store_redacts_on_append(&store).await;
}

#[tokio::test]
async fn default_redactor_runs_before_memory_store_records_events() {
    let store = InMemoryEventStore::new(Arc::new(DefaultRedactor::default()));

    assert_store_redacts_on_append(&store).await;
}

async fn assert_store_redacts_on_append(store: &dyn EventStore) {
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();

    store
        .append(tenant_id, session_id, &[secret_event()])
        .await
        .expect("append");

    let events = store
        .read(
            tenant_id,
            session_id,
            harness_journal::ReplayCursor::FromStart,
        )
        .await
        .expect("read")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(events.len(), 1);
    let Event::UnexpectedError(event) = &events[0] else {
        panic!("unexpected event type");
    };
    assert!(!event.error.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(!event.error.contains("Bearer abcdefghijklmnopqrstuvwxyz"));
    assert!(event.error.contains("[REDACTED]"));
}
