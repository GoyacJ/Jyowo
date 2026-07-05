//! Memory provider registry.
//!
//! Supports multiple providers with fanout recall, deduplication,
//! provider-level budget enforcement, and write target selection.

use std::collections::HashMap;
use std::sync::Arc;

use harness_contracts::{
    MemoryError, MemoryProviderDurability, MemoryProviderKind, MemoryVisibility,
    MemoryVisibilityClass,
};

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
        let descriptor = provider.descriptor();
        validate_descriptor(&id, &descriptor)?;
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
        sort_providers_by_policy(&mut providers);
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
        descriptors.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.provider_id.cmp(&right.provider_id))
        });
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
        sort_providers_by_policy(&mut providers);
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
        let mut providers: Vec<_> = self.writable_provider_candidates();
        sort_providers_by_policy(&mut providers);
        providers
    }

    /// Select the single provider used for direct writes.
    pub fn select_write_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        let mut ordered_targets = self.writable_provider_candidates();
        sort_providers_by_policy(&mut ordered_targets);
        if ordered_targets.is_empty() {
            None
        } else {
            Some(ordered_targets.remove(0))
        }
    }

    /// Select the provider for a direct write with a known target visibility.
    pub fn select_write_provider_for_visibility(
        &self,
        visibility: &MemoryVisibility,
    ) -> Option<Arc<dyn MemoryProvider>> {
        let visibility_class = visibility_class(visibility)?;
        let mut ordered_targets: Vec<_> = self
            .writable_provider_candidates()
            .into_iter()
            .filter(|provider| {
                let descriptor = provider.descriptor();
                descriptor.allowed_visibility.contains(&visibility_class)
                    && descriptor.workspace_scope.is_none()
            })
            .collect();
        sort_providers_by_policy(&mut ordered_targets);
        if ordered_targets.is_empty() {
            None
        } else {
            Some(ordered_targets.remove(0))
        }
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

fn visibility_class(visibility: &MemoryVisibility) -> Option<MemoryVisibilityClass> {
    match visibility {
        MemoryVisibility::Private { .. } => Some(MemoryVisibilityClass::Private),
        MemoryVisibility::User { .. } => Some(MemoryVisibilityClass::User),
        MemoryVisibility::Team { .. } => Some(MemoryVisibilityClass::Team),
        MemoryVisibility::Tenant => Some(MemoryVisibilityClass::Tenant),
        _ => None,
    }
}

fn validate_descriptor(
    provider_id: &str,
    descriptor: &MemoryProviderDescriptor,
) -> Result<(), MemoryError> {
    if provider_id.is_empty() || descriptor.provider_id.is_empty() {
        return Err(MemoryError::Message(
            "memory provider descriptor requires provider_id".to_owned(),
        ));
    }
    if descriptor.provider_id != provider_id {
        return Err(MemoryError::Message(format!(
            "memory provider descriptor id mismatch: expected {provider_id}, got {}",
            descriptor.provider_id
        )));
    }
    if descriptor.tenant_scope.is_some() && descriptor.workspace_scope.is_some() {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} cannot declare both tenant_scope and workspace_scope"
        )));
    }
    if !descriptor.readable && !descriptor.writable {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} must be readable or writable"
        )));
    }
    if descriptor.allowed_visibility.is_empty() {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} must declare allowed_visibility"
        )));
    }
    if descriptor.writable && !descriptor.supports_evidence {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} writable descriptor must support evidence"
        )));
    }
    if descriptor.writable
        && descriptor.durability != MemoryProviderDurability::Durable
        && descriptor.provider_kind != MemoryProviderKind::Team
    {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} writable descriptor must be durable"
        )));
    }
    if descriptor.timeout_ms == 0 {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} must declare timeout_ms"
        )));
    }
    if descriptor.max_records_per_recall == 0 {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} must declare max_records_per_recall"
        )));
    }
    if descriptor.max_chars_per_recall == 0 {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} must declare max_chars_per_recall"
        )));
    }
    if descriptor.max_bytes_per_record == 0 {
        return Err(MemoryError::Message(format!(
            "memory provider {provider_id} must declare max_bytes_per_record"
        )));
    }
    Ok(())
}

fn sort_providers_by_policy(providers: &mut [Arc<dyn MemoryProvider>]) {
    providers.sort_by(|left, right| {
        let left_descriptor = left.descriptor();
        let right_descriptor = right.descriptor();
        right_descriptor
            .priority
            .cmp(&left_descriptor.priority)
            .then_with(|| {
                left_descriptor
                    .provider_id
                    .cmp(&right_descriptor.provider_id)
            })
    });
}

impl MemoryProviderRegistry {
    fn writable_provider_candidates(&self) -> Vec<Arc<dyn MemoryProvider>> {
        self.providers
            .values()
            .filter(|p| {
                let descriptor = p.descriptor();
                descriptor.writable
                    && (descriptor.durability == MemoryProviderDurability::Durable
                        || descriptor.provider_kind == MemoryProviderKind::Team)
                    && descriptor.supports_evidence
            })
            .cloned()
            .collect()
    }
}
