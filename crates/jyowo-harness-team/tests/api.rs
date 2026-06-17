use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    AgentId, ContextVisibility, Event, NoopRedactor, Recipient, SessionId, TeamTerminationReason,
    TenantId,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
use harness_team::{MessageBus, Team, TeamBuilder, TeamJournalContext, Topology};

#[tokio::test]
async fn public_team_api_dispatches_pauses_resumes_and_terminates() {
    let (team, store, session_id, sender) = team_fixture();

    let message = team
        .dispatch(sender, Recipient::Broadcast, "ship")
        .await
        .expect("dispatch should post through bus");
    assert_eq!(
        message.payload,
        harness_team::MessagePayload::Text("ship".to_owned())
    );

    team.pause();
    assert!(team.is_paused());
    let error = team
        .dispatch(sender, Recipient::Broadcast, "blocked")
        .await
        .expect_err("paused team should reject dispatch");
    assert!(error.to_string().contains("paused"));
    team.resume();
    assert!(!team.is_paused());

    let report = team
        .terminate(TeamTerminationReason::Cancelled)
        .await
        .expect("terminate should emit lifecycle events");
    assert_eq!(report.final_state["terminated"], "cancelled");
    assert_ne!(report.report_hash, [0; 32]);
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::AgentMessageSent(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamTerminated(_))));
}

fn team_fixture() -> (Team, Arc<InMemoryEventStore>, SessionId, AgentId) {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let worker = AgentId::new();
    let spec = TeamBuilder::new("api", Topology::PeerToPeer)
        .member(sender, "sender", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    (
        Team::new(
            spec,
            bus,
            journal,
            store.clone(),
            Arc::new(InMemoryBlobStore::default()),
        ),
        store,
        session_id,
        sender,
    )
}
