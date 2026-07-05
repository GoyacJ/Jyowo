//! Memory reference hydration.
//!
//! Resolves `ConversationContextReference::Memory` ids into actual
//! fenced memory content before model request assembly.

use async_trait::async_trait;
use harness_contracts::{MemoryError, MemoryId};

/// Resolved memory reference carrying either hydrated content or a failure reason.
#[derive(Debug, Clone)]
pub struct ResolvedMemoryReference {
    pub memory_id: MemoryId,
    pub label: String,
    pub outcome: MemoryReferenceOutcome,
}

#[derive(Debug, Clone)]
pub enum MemoryReferenceOutcome {
    /// Successfully resolved content, fenced as untrusted.
    Hydrated {
        content: String,
        provider_id: String,
    },
    /// Reference could not be resolved.
    Failed { reason: String },
}

/// Resolves memory references to actual content via a provider lookup.
#[async_trait]
pub trait ContextReferenceResolver: Send + Sync + 'static {
    /// Resolve a single memory reference by ID.
    async fn resolve_memory(
        &self,
        memory_id: MemoryId,
        label: String,
    ) -> Result<ResolvedMemoryReference, MemoryError>;
}

/// Simple resolver backed by a function.
pub struct FnMemoryResolver<F> {
    resolve_fn: F,
}

impl<F, Fut> FnMemoryResolver<F>
where
    F: Fn(MemoryId) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(String, String), MemoryError>> + Send,
{
    #[must_use]
    pub fn new(resolve_fn: F) -> Self {
        Self { resolve_fn }
    }
}

#[async_trait]
impl<F, Fut> ContextReferenceResolver for FnMemoryResolver<F>
where
    F: Fn(MemoryId) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(String, String), MemoryError>> + Send,
{
    async fn resolve_memory(
        &self,
        memory_id: MemoryId,
        label: String,
    ) -> Result<ResolvedMemoryReference, MemoryError> {
        match (self.resolve_fn)(memory_id).await {
            Ok((content, provider_id)) => Ok(ResolvedMemoryReference {
                memory_id,
                label,
                outcome: MemoryReferenceOutcome::Hydrated {
                    content,
                    provider_id,
                },
            }),
            Err(e) => Ok(ResolvedMemoryReference {
                memory_id,
                label,
                outcome: MemoryReferenceOutcome::Failed {
                    reason: e.to_string(),
                },
            }),
        }
    }
}

/// Fence memory content as untrusted context for injection into the model request.
pub fn fence_memory_content(content: &str, memory_id: MemoryId, provider_id: &str) -> String {
    crate::wrap_memory_reference_context(content, memory_id, provider_id)
}
