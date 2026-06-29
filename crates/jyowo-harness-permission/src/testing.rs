use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{Decision, PermissionError};
use parking_lot::Mutex;

use crate::{
    DecisionPersistence, NoopDecisionPersistence, PermissionBroker, PermissionContext,
    PermissionRequest, PersistedDecision,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TestBrokerCall {
    pub request: PermissionRequest,
    pub ctx: PermissionContext,
}

#[derive(Clone)]
pub struct TestBroker {
    decisions: Arc<Mutex<VecDeque<Decision>>>,
    calls: Arc<Mutex<Vec<TestBrokerCall>>>,
    persistence: Arc<dyn DecisionPersistence>,
}

impl TestBroker {
    pub fn new(decisions: Vec<Decision>) -> Self {
        Self {
            decisions: Arc::new(Mutex::new(decisions.into())),
            calls: Arc::new(Mutex::new(Vec::new())),
            persistence: Arc::new(NoopDecisionPersistence),
        }
    }

    #[must_use]
    pub fn with_persistence(mut self, persistence: Arc<dyn DecisionPersistence>) -> Self {
        self.persistence = persistence;
        self
    }

    pub fn calls(&self) -> Vec<TestBrokerCall> {
        self.calls.lock().clone()
    }
}

impl Default for TestBroker {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[async_trait]
impl PermissionBroker for TestBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.calls.lock().push(TestBrokerCall { request, ctx });
        self.decisions
            .lock()
            .pop_front()
            .unwrap_or(Decision::DenyOnce)
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persistence.persist(decision).await
    }
}
