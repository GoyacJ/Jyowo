//! Testing journal store for contract tests and SDK consumers.

use std::sync::Arc;

use harness_contracts::Redactor;

use crate::InMemoryEventStore;

pub type TestEventStore = InMemoryEventStore;

pub fn test_event_store(redactor: Arc<dyn Redactor>) -> TestEventStore {
    InMemoryEventStore::new(redactor)
}
