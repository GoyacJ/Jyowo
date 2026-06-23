use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use harness_contracts::{
    AgentId, CorrelationId, Event, EventId, ForkReason, JournalError, JournalOffset, NoopRedactor,
    Recipient, SessionId, TenantId,
};
use harness_journal::{
    EventEnvelope, EventStore, InMemoryBlobStore, InMemoryEventStore, PrunePolicy, PruneReport,
    ReplayCursor, SessionFilter, SessionSnapshot, SessionSummary,
};
use harness_team::{
    AgentMessage, BusBackpressure, BusPersistence, ContextVisibility, CoordinatorWorkerStrategy,
    MessageBus, MessageBusSpec, MessageOrdering, MessagePayload, PeerToPeerStrategy,
    ReplayWindowSpec, RoutingStrategy, Team, TeamBuilder, TeamJournalContext, Topology,
};

#[tokio::test]
async fn message_bus_preserves_correlation_id_in_replay() {
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let session_id = SessionId::new();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let bus = MessageBus::journaled(
        team.team_id,
        16,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id,
        },
        store,
    );
    let correlation_id = CorrelationId::new();
    let message = AgentMessage::text_with_correlation(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "hello",
        correlation_id,
    );

    bus.send(message.clone()).await.unwrap();

    let replayed = bus.replay().await;
    assert_eq!(replayed[0].correlation_id, correlation_id);
}

#[tokio::test]
async fn message_bus_writes_sent_and_routed_events_to_journal_for_replay() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(
        team.team_id,
        16,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id,
        },
        store.clone(),
    );
    let correlation_id = CorrelationId::new();
    let message = AgentMessage::text_with_correlation(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "hello",
        correlation_id,
    );

    bus.send(message.clone()).await.unwrap();

    let replayed = bus.replay_from_journal().await.unwrap();
    assert_eq!(replayed, vec![message]);
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
        .any(|event| matches!(event, Event::AgentMessageRouted(_))));
}

#[tokio::test]
async fn raw_message_bus_send_rejects_correlation_limit_and_writes_engine_failed() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(
        team.team_id,
        16,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id,
        },
        store.clone(),
    )
    .with_spec(MessageBusSpec {
        max_messages_per_correlation: 1,
        ..MessageBusSpec::default()
    });
    let correlation_id = CorrelationId::new();
    let first = AgentMessage::text_with_correlation(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "first",
        correlation_id,
    );
    let second = AgentMessage::text_with_correlation(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "second",
        correlation_id,
    );

    bus.send(first.clone()).await.unwrap();
    let error = bus.send(second).await.unwrap_err();

    assert!(matches!(
        error,
        harness_team::TeamError::RoutingLimitExceeded(reason)
            if reason == "max_messages_per_correlation"
    ));
    assert_eq!(bus.replay().await, vec![first]);
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::EngineFailed(failed)
                if failed
                    .error
                    .to_string()
                    .contains("cyclic routing: max_messages_per_correlation")
        )
    }));
    let observations = bus.observation_snapshot().await;
    assert_eq!(observations.cyclic_routing_detected, 1);
}

#[tokio::test]
async fn message_bus_does_not_broadcast_or_memory_replay_when_journal_fails() {
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(
        team.team_id,
        16,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
        },
        Arc::new(FailingEventStore),
    );
    let mut rx = bus.subscribe();
    let message = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "hello",
    );

    assert!(bus.send(message).await.is_err());
    assert!(bus.replay().await.is_empty());
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn message_bus_replay_window_none_last_since_and_all() {
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(
        team.team_id,
        16,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
        },
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
    );
    let first = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "one",
    );
    bus.send(first.clone()).await.unwrap();
    let since = harness_contracts::now();
    let second = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "two",
    );
    let third = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "three",
    );
    bus.send(second.clone()).await.unwrap();
    bus.send(third.clone()).await.unwrap();

    assert!(bus.replay_for_spec(ReplayWindowSpec::None).await.is_empty());
    assert_eq!(
        bus.replay_for_spec(ReplayWindowSpec::Last(2)).await,
        vec![second.clone(), third.clone()]
    );
    assert_eq!(
        bus.replay_for_spec(ReplayWindowSpec::Since(since)).await,
        vec![second.clone(), third.clone()]
    );
    assert_eq!(
        bus.replay_for_spec(ReplayWindowSpec::All).await,
        vec![first, second, third]
    );
}

#[tokio::test]
async fn message_bus_reject_new_backpressure_fails_without_journaling_message() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(
        team.team_id,
        1,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id,
        },
        store.clone(),
    )
    .with_spec(MessageBusSpec {
        buffer_size: 1,
        persistence: BusPersistence::Journaled,
        ordering: MessageOrdering::Fifo,
        replay_window: ReplayWindowSpec::All,
        backpressure: BusBackpressure::RejectNew,
        max_messages_per_correlation: 256,
    });
    let first = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "one",
    );
    let second = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "two",
    );
    bus.send(first.clone()).await.unwrap();

    let error = bus.send(second).await.unwrap_err();

    assert!(matches!(
        error,
        harness_team::TeamError::MessageBusBackpressure { depth: 1, .. }
    ));
    assert_eq!(bus.replay().await, vec![first]);
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::AgentMessageSent(_)))
            .count(),
        1
    );
    let observations = bus.observation_snapshot().await;
    assert_eq!(observations.message_bus_backpressure, 1);
}

#[tokio::test]
async fn message_bus_drop_oldest_backpressure_preserves_capacity() {
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(AgentId::new(), "analyst", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(
        team.team_id,
        1,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
        },
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
    )
    .with_spec(MessageBusSpec {
        buffer_size: 1,
        persistence: BusPersistence::InMemory,
        ordering: MessageOrdering::Fifo,
        replay_window: ReplayWindowSpec::All,
        backpressure: BusBackpressure::DropOldest,
        max_messages_per_correlation: 256,
    });
    let first = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "one",
    );
    let second = AgentMessage::text(
        team.team_id,
        team.members[0].agent_id,
        Recipient::Broadcast,
        "two",
    );

    bus.send(first).await.unwrap();
    bus.send(second.clone()).await.unwrap();

    assert_eq!(bus.replay().await, vec![second]);
}

#[tokio::test]
async fn journaled_and_durable_bus_replay_from_event_store() {
    for persistence in [BusPersistence::Journaled, BusPersistence::Durable] {
        let team = TeamBuilder::new("triage", Topology::PeerToPeer)
            .member(AgentId::new(), "analyst", ContextVisibility::All)
            .build();
        let bus = MessageBus::journaled(
            team.team_id,
            16,
            TeamJournalContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
            },
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        )
        .with_spec(MessageBusSpec {
            buffer_size: 16,
            persistence,
            ordering: MessageOrdering::Total,
            replay_window: ReplayWindowSpec::All,
            backpressure: BusBackpressure::DropOldest,
            max_messages_per_correlation: 256,
        });
        let message = AgentMessage::text(
            team.team_id,
            team.members[0].agent_id,
            Recipient::Broadcast,
            "hello",
        );

        bus.send(message.clone()).await.unwrap();

        assert_eq!(bus.replay_from_journal().await.unwrap(), vec![message]);
    }
}

#[test]
fn coordinator_worker_routes_worker_response_back_to_coordinator() {
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let team = TeamBuilder::new("triage", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::Private)
        .coordinator_worker(coordinator, vec![worker])
        .build();
    let message = AgentMessage::new(
        team.team_id,
        worker,
        Recipient::Coordinator,
        MessagePayload::Response {
            in_reply_to: harness_contracts::MessageId::new(),
            body: serde_json::json!({ "done": true }),
        },
    );

    assert_eq!(
        CoordinatorWorkerStrategy.route(&message, &team),
        vec![coordinator]
    );
}

#[test]
fn peer_to_peer_broadcast_excludes_sender() {
    let sender = AgentId::new();
    let peer = AgentId::new();
    let team = TeamBuilder::new("triage", Topology::PeerToPeer)
        .member(sender, "analyst", ContextVisibility::All)
        .member(peer, "reviewer", ContextVisibility::All)
        .build();
    let message = AgentMessage::text(team.team_id, sender, Recipient::Broadcast, "help");

    assert_eq!(PeerToPeerStrategy.route(&message, &team), vec![peer]);
}

#[tokio::test]
async fn allowlist_quote_hides_unquoted_allowlisted_message() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let lead = AgentId::new();
    let analyst = AgentId::new();
    let observer = AgentId::new();
    let spec = TeamBuilder::new("quote", Topology::PeerToPeer)
        .member(lead, "lead", ContextVisibility::All)
        .member(analyst, "analyst", ContextVisibility::All)
        .member(
            observer,
            "observer",
            ContextVisibility::AllowlistQuote(vec![lead]),
        )
        .build();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let team = Team::new(
        spec.clone(),
        MessageBus::journaled(spec.team_id, 16, journal, store.clone()),
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let message = AgentMessage::text(spec.team_id, lead, Recipient::Broadcast, "status");

    team.post(message.clone()).await.unwrap();

    let routed = routed_recipients(&store, session_id, message.message_id).await;
    assert_eq!(routed, vec![analyst]);
}

#[tokio::test]
async fn allowlist_quote_allows_message_quoting_allowlisted_history() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let lead = AgentId::new();
    let analyst = AgentId::new();
    let observer = AgentId::new();
    let spec = TeamBuilder::new("quote", Topology::PeerToPeer)
        .member(lead, "lead", ContextVisibility::All)
        .member(analyst, "analyst", ContextVisibility::All)
        .member(
            observer,
            "observer",
            ContextVisibility::AllowlistQuote(vec![lead]),
        )
        .build();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let team = Team::new(
        spec.clone(),
        MessageBus::journaled(spec.team_id, 16, journal, store.clone()),
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let quoted = AgentMessage::text(spec.team_id, lead, Recipient::Broadcast, "status");
    team.post(quoted.clone()).await.unwrap();
    let response = AgentMessage::new(
        spec.team_id,
        analyst,
        Recipient::Broadcast,
        MessagePayload::Response {
            in_reply_to: quoted.message_id,
            body: serde_json::json!({ "body": "quoted status" }),
        },
    );

    team.post(response.clone()).await.unwrap();

    let routed = routed_recipients(&store, session_id, response.message_id).await;
    assert!(routed.contains(&observer));
}

async fn routed_recipients(
    store: &InMemoryEventStore,
    session_id: SessionId,
    message_id: harness_contracts::MessageId,
) -> Vec<AgentId> {
    store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .filter_map(|event| async move {
            match event {
                Event::AgentMessageRouted(routed) if routed.message_id == message_id => {
                    Some(routed.resolved_recipients)
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .next()
        .unwrap_or_default()
}

struct FailingEventStore;

#[async_trait]
impl EventStore for FailingEventStore {
    async fn append(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        Err(JournalError::Message("append failed".to_owned()))
    }

    async fn read_envelopes(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        Ok(Box::pin(stream::iter(Vec::new())))
    }

    async fn query_after(
        &self,
        _tenant: TenantId,
        _after: Option<EventId>,
        _limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Ok(Vec::new())
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(None)
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Err(JournalError::Message("delete failed".to_owned()))
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Ok(PruneReport::default())
    }
}
