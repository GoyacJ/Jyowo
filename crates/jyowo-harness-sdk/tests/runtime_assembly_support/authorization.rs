use super::*;

pub fn test_authorization_service(
    broker: Arc<dyn harness_permission::PermissionBroker>,
    event_store: Arc<dyn EventStore>,
) -> Arc<harness_execution::AuthorizationService> {
    let decision_store = Arc::new(RuntimeTransientDecisionStore::default());
    let authority = Arc::new(
        harness_permission::PermissionAuthority::builder()
            .with_policy_broker(broker)
            .with_transient_decision_store(decision_store)
            .build()
            .expect("test permission authority should build"),
    );
    Arc::new(harness_execution::AuthorizationService::new(
        authority,
        ExecutionPreflightRegistry::new(
            Arc::new(NoopSandbox::new()),
            None,
            Arc::new(CapabilityRegistry::default()),
        ),
        Arc::new(RuntimeAuthorizationEventSink { event_store }),
        Arc::new(harness_execution::TicketLedger::default()),
    ))
}

#[derive(Default)]
struct RuntimeTransientDecisionStore {
    decisions: Mutex<Vec<harness_permission::PersistedDecision>>,
}

#[async_trait]
impl harness_permission::DecisionPersistence for RuntimeTransientDecisionStore {
    async fn persist(
        &self,
        decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        self.decisions.lock().unwrap().push(decision);
        Ok(())
    }
}

#[async_trait]
impl harness_permission::DecisionHistory for RuntimeTransientDecisionStore {
    async fn find_scoped_decision(
        &self,
        _lookup: harness_permission::DecisionLookup,
    ) -> Result<Option<harness_permission::PersistedDecision>, harness_contracts::PermissionError>
    {
        Ok(None)
    }
}

struct RuntimeAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
}

#[async_trait]
impl harness_execution::AuthorizationEventSink for RuntimeAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        events: Vec<Event>,
    ) -> Result<(), harness_execution::ExecutionError> {
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map_err(|error| harness_execution::ExecutionError::EventSinkFailed {
                reason: error.to_string(),
            })?;
        Ok(())
    }
}
