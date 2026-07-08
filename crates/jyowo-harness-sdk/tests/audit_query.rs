#![cfg(feature = "testing")]

use std::sync::Arc;

use futures::executor::block_on;
use harness_contracts::{
    ConfigHash, Event, SessionCreatedEvent, SessionId, SnapshotId, TenantId, TrustLevel,
};
use harness_journal::{AuditFilter, AuditOrder, AuditQuery, AuditScope, EventStore};
use jyowo_harness_sdk::{testing::*, Harness};

#[test]
fn audit_query_enforces_trust_and_tenant_scope() {
    block_on(async {
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .unwrap();
        let session_id = SessionId::new();
        let other_session_id = SessionId::new();
        store
            .append(
                TenantId::SINGLE,
                session_id,
                &[session_created(session_id, TenantId::SINGLE)],
            )
            .await
            .unwrap();
        let other_tenant = TenantId::new();
        store
            .append(
                other_tenant,
                other_session_id,
                &[session_created(other_session_id, other_tenant)],
            )
            .await
            .unwrap();

        let denied = harness
            .audit_query(TenantId::SINGLE, tenant_query(), TrustLevel::UserControlled)
            .await
            .unwrap_err();
        assert!(matches!(
            denied,
            harness_contracts::HarnessError::PermissionDenied(_)
        ));

        let page = harness
            .audit_query(TenantId::SINGLE, tenant_query(), TrustLevel::AdminTrusted)
            .await
            .unwrap();

        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].session_id, session_id);
    });
}

fn tenant_query() -> AuditQuery {
    AuditQuery {
        scope: AuditScope::Tenant,
        filter: AuditFilter::default(),
        order: AuditOrder::EventIdAsc,
        limit: 16,
    }
}

fn session_created(session_id: SessionId, tenant_id: TenantId) -> Event {
    Event::SessionCreated(SessionCreatedEvent {
        session_id,
        tenant_id,
        options_hash: [1; 32],
        snapshot_id: SnapshotId::from_u128(1),
        effective_config_hash: ConfigHash([2; 32]),
        created_at: harness_contracts::now(),
    })
}
