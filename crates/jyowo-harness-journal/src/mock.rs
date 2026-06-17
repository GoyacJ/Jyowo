//! Mock journal store for contract tests and SDK consumers.

use std::sync::Arc;

use harness_contracts::Redactor;

use crate::InMemoryEventStore;

pub type MockEventStore = InMemoryEventStore;

pub fn mock_event_store(redactor: Arc<dyn Redactor>) -> MockEventStore {
    InMemoryEventStore::new(redactor)
}
