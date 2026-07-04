//! Memory provider registry.
//!
//! Supports multiple providers with fanout recall, deduplication,
//! provider-level budget enforcement, and write target selection.

use std::collections::HashMap;
use std::sync::Arc;

use harness_contracts::MemoryError;

use crate::{MemoryProvider, MemoryProviderDescriptor};

#[derive(Clone, Default)]
pub struct MemoryProviderRegistry {
    providers: HashMap<String, Arc<dyn MemoryProvider>>,
}

impl MemoryProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider. Returns an error if a provider with the same ID already exists.
    pub fn register(&mut self, provider: Arc<dyn MemoryProvider>) -> Result<(), MemoryError> {
        let id = provider.provider_id().to_owned();
        if self.providers.contains_key(&id) {
            return Err(MemoryError::Message(format!(
                "provider already registered: {id}"
            )));
        }
        self.providers.insert(id, provider);
        Ok(())
    }

    /// Remove a provider by ID.
    pub fn unregister(&mut self, provider_id: &str) {
        self.providers.remove(provider_id);
    }

    /// Iterate over all registered provider descriptors.
    pub fn providers(&self) -> impl Iterator<Item = MemoryProviderDescriptor> + '_ {
        self.providers.values().map(|p| p.descriptor())
    }

    /// Get a provider's descriptor by ID.
    pub fn descriptor(&self, provider_id: &str) -> Option<MemoryProviderDescriptor> {
        self.providers.get(provider_id).map(|p| p.descriptor())
    }

    /// Get a reference to a provider by ID.
    pub fn get(&self, provider_id: &str) -> Option<Arc<dyn MemoryProvider>> {
        self.providers.get(provider_id).cloned()
    }

    /// All provider arcs, sorted by priority (highest first).
    pub fn provider_arcs_sorted(&self) -> Vec<Arc<dyn MemoryProvider>> {
        let mut providers: Vec<_> = self.providers.values().cloned().collect();
        providers.sort_by_key(|p| -p.descriptor().priority);
        providers
    }

    /// All readable providers, sorted by priority (highest first).
    pub fn readable_providers_sorted(&self) -> Vec<MemoryProviderDescriptor> {
        let mut descriptors: Vec<_> = self
            .providers
            .values()
            .filter(|p| p.descriptor().readable)
            .map(|p| p.descriptor())
            .collect();
        descriptors.sort_by_key(|d| -d.priority);
        descriptors
    }

    /// All readable provider arcs, sorted by priority (highest first).
    pub fn readable_provider_arcs_sorted(&self) -> Vec<Arc<dyn MemoryProvider>> {
        let mut providers: Vec<_> = self
            .providers
            .values()
            .filter(|p| p.descriptor().readable)
            .cloned()
            .collect();
        providers.sort_by_key(|p| -p.descriptor().priority);
        providers
    }

    /// All writable providers.
    pub fn write_targets(&self) -> Vec<MemoryProviderDescriptor> {
        self.providers
            .values()
            .filter(|p| p.descriptor().writable)
            .map(|p| p.descriptor())
            .collect()
    }

    /// All writable providers as arcs, sorted by priority (highest first).
    pub fn writable_providers_sorted(&self) -> Vec<Arc<dyn MemoryProvider>> {
        let mut providers: Vec<_> = self
            .providers
            .values()
            .filter(|p| p.descriptor().writable)
            .cloned()
            .collect();
        providers.sort_by_key(|p| -p.descriptor().priority);
        providers
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
