#![cfg(all(feature = "agents-team", feature = "testing"))]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harness_contracts::{
    CapabilityRegistry, Event, PermissionError, RedactPatternSet, RedactRules, RedactScope,
    Redactor, SessionId, TenantId,
};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionError, ExecutionPreflightRegistry,
    TicketLedger,
};
use harness_journal::EventStore;
use harness_permission::{
    DecisionHistory, DecisionLookup, DecisionPersistence, PermissionAuthority, PermissionBroker,
    PersistedDecision,
};
use harness_sandbox::NoopSandbox;

pub fn test_authorization_service(
    broker: Arc<dyn PermissionBroker>,
    event_store: Arc<dyn EventStore>,
) -> Arc<AuthorizationService> {
    let decision_store = Arc::new(TransientDecisionStore::default());
    let authority = Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(broker)
            .with_transient_decision_store(decision_store)
            .build()
            .expect("test permission authority should build"),
    );
    Arc::new(AuthorizationService::new(
        authority,
        ExecutionPreflightRegistry::new(
            Arc::new(NoopSandbox::new()),
            None,
            Arc::new(CapabilityRegistry::default()),
        ),
        Arc::new(JournalAuthorizationEventSink { event_store }),
        Arc::new(TicketLedger::default()),
    ))
}

#[derive(Default)]
struct TransientDecisionStore {
    decisions: Mutex<Vec<PersistedDecision>>,
}

#[async_trait]
impl DecisionPersistence for TransientDecisionStore {
    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.decisions.lock().unwrap().push(decision);
        Ok(())
    }
}

#[async_trait]
impl DecisionHistory for TransientDecisionStore {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(None)
    }
}

struct JournalAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
}

#[async_trait]
impl AuthorizationEventSink for JournalAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        let events = events
            .into_iter()
            .map(redact_event_for_test)
            .collect::<Vec<_>>();
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map_err(|error| ExecutionError::EventSinkFailed {
                reason: error.to_string(),
            })?;
        Ok(())
    }
}

fn redact_event_for_test(event: Event) -> Event {
    let Ok(mut value) = serde_json::to_value(&event) else {
        return event;
    };
    redact_json_strings_for_test(&mut value, &TestSecretRedactor);
    serde_json::from_value(value).unwrap_or(event)
}

fn redact_json_strings_for_test(value: &mut serde_json::Value, redactor: &dyn Redactor) {
    match value {
        serde_json::Value::String(text) => {
            *text = redactor.redact(
                text,
                &RedactRules {
                    scope: RedactScope::EventBody,
                    replacement: "[REDACTED]".to_owned(),
                    pattern_set: RedactPatternSet::Default,
                },
            );
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json_strings_for_test(item, redactor);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                redact_json_strings_for_test(item, redactor);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

struct TestSecretRedactor;

impl Redactor for TestSecretRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        input.replace("sk-abcdefghijklmnopqrstuvwxyz", &rules.replacement)
    }
}
