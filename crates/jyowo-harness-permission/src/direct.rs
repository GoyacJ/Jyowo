use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use harness_contracts::{Decision, PermissionError};

use crate::{
    DecisionPersistence, NoopDecisionPersistence, PermissionBroker, PermissionContext,
    PermissionRequest, PersistedDecision,
};

pub struct DirectBroker<F>
where
    F: Fn(PermissionRequest, PermissionContext) -> BoxFuture<'static, Decision>
        + Send
        + Sync
        + 'static,
{
    callback: F,
    persistence: Arc<dyn DecisionPersistence>,
}

impl<F> DirectBroker<F>
where
    F: Fn(PermissionRequest, PermissionContext) -> BoxFuture<'static, Decision>
        + Send
        + Sync
        + 'static,
{
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            persistence: Arc::new(NoopDecisionPersistence),
        }
    }

    #[must_use]
    pub fn with_persistence(mut self, persistence: Arc<dyn DecisionPersistence>) -> Self {
        self.persistence = persistence;
        self
    }
}

#[async_trait]
impl<F> PermissionBroker for DirectBroker<F>
where
    F: Fn(PermissionRequest, PermissionContext) -> BoxFuture<'static, Decision>
        + Send
        + Sync
        + 'static,
{
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        (self.callback)(request, ctx).await
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persistence.persist(decision).await
    }
}
