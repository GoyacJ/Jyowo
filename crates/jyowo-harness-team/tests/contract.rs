use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use harness_contracts::{
    AgentId, Event, NoopRedactor, Recipient, RunId, SessionId, TenantId, UsageSnapshot,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
use harness_team::{
    AgentMessage, BusBackpressure, BusPersistence, ContextVisibility, Coordinator, MessageBus,
    MessageOrdering, MessagePayload, ReplayWindowSpec, ResourceQuota, RoleRoutedStrategy,
    RoleRoutingRule, RouteFallback, RoutingPattern, SharedMemorySpec, Team, TeamBuilder,
    TeamJournalContext, TeamLifecycle, TeamMember, TeamMemberEngineConfig, TeamObservability,
    TeamResourceQuota, TeamSpec, TokenBudget, Topology,
};

#[test]
fn team_spec_declares_single_process_constraint() {
    let spec = TeamSpec::new("triage", Topology::CoordinatorWorker);

    assert!(spec.single_process_only);
}

#[test]
fn team_spec_exposes_bus_memory_lifecycle_observability_and_quota_carriers() {
    let spec = TeamSpec::new("triage", Topology::CoordinatorWorker);

    assert_eq!(spec.message_bus.persistence, BusPersistence::InMemory);
    assert_eq!(spec.message_bus.ordering, MessageOrdering::Fifo);
    assert_eq!(spec.message_bus.replay_window, ReplayWindowSpec::All);
    assert_eq!(spec.message_bus.buffer_size, 256);
    assert_eq!(spec.shared_memory, SharedMemorySpec::Disabled);
    assert_eq!(spec.lifecycle, TeamLifecycle::OneShot);
    assert_eq!(spec.observability, TeamObservability::default());
    assert_eq!(spec.quota, TeamResourceQuota::default());
}

#[test]
fn team_member_engine_config_exposes_member_quota_and_token_budget() {
    let config = TeamMemberEngineConfig {
        quota: Some(ResourceQuota {
            max_tokens: Some(4_096),
            max_tool_calls: Some(5),
            max_duration: Some(Duration::from_secs(60)),
            max_cost_cents: Some(200),
        }),
        token_budget: TokenBudget {
            max_tokens_per_turn: 128_000,
            max_tokens_per_session: 512_000,
            soft_budget_ratio: 0.7,
            hard_budget_ratio: 0.9,
            per_tool_max_chars: 16_000,
        },
        ..TeamMemberEngineConfig::default()
    };

    let value = serde_json::to_value(&config).unwrap();

    assert_eq!(value["quota"]["max_tokens"], 4_096);
    assert_eq!(value["quota"]["max_tool_calls"], 5);
    assert_eq!(value["quota"]["max_cost_cents"], 200);
    assert_eq!(value["token_budget"]["max_tokens_per_turn"], 128_000);
    assert_eq!(value["token_budget"]["per_tool_max_chars"], 16_000);
    let decoded: TeamMemberEngineConfig = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, config);
}

#[tokio::test]
async fn team_member_joined_spec_hash_includes_member_budget_config() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let spec = TeamBuilder::new("hash-budget", Topology::PeerToPeer).build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let limited = TeamMember {
        agent_id: AgentId::new(),
        role: "limited".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: TeamMemberEngineConfig {
            quota: Some(ResourceQuota {
                max_tokens: Some(256),
                ..ResourceQuota::default()
            }),
            ..TeamMemberEngineConfig::default()
        },
    };
    let defaulted = TeamMember {
        agent_id: AgentId::new(),
        role: "defaulted".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: TeamMemberEngineConfig::default(),
    };

    team.add_member(limited.clone()).await.unwrap();
    team.add_member(defaulted.clone()).await.unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let joined: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::TeamMemberJoined(joined) => Some(joined),
            _ => None,
        })
        .collect();
    let limited_hash = *blake3::hash(&serde_json::to_vec(&limited).unwrap()).as_bytes();
    let defaulted_hash = *blake3::hash(&serde_json::to_vec(&defaulted).unwrap()).as_bytes();

    assert_ne!(limited_hash, defaulted_hash);
    assert!(joined.iter().any(|event| event.spec_hash == limited_hash));
    assert!(joined.iter().any(|event| event.spec_hash == defaulted_hash));
}

#[test]
fn team_spec_exposes_route_fallback_rules_and_custom_strategy_id() {
    let mut spec = TeamSpec::new("custom", Topology::Custom);
    spec.topology_config.custom_strategy_id = Some("weighted-handoff".to_owned());
    spec.topology_config.route_fallback = RouteFallback::Broadcast;
    spec.topology_config.role_rules.push(RoleRoutingRule {
        rule_id: "security".to_owned(),
        priority: 10,
        pattern: RoutingPattern::KeywordAny {
            keywords: vec!["security".to_owned()],
            roles: vec!["reviewer".to_owned()],
        },
    });

    assert_eq!(
        spec.topology_config.custom_strategy_id.as_deref(),
        Some("weighted-handoff")
    );
    assert_eq!(
        spec.topology_config.route_fallback,
        RouteFallback::Broadcast
    );
    assert!(matches!(
        &spec.topology_config.role_rules[0].pattern,
        RoutingPattern::KeywordAny { roles, .. } if roles == &vec!["reviewer".to_owned()]
    ));
}

#[test]
fn custom_topology_requires_strategy_id() {
    let spec = TeamSpec::new("custom", Topology::Custom);

    assert!(spec.validate().is_err());
}

#[test]
fn send_to_coordinator_fallback_requires_coordinator() {
    let mut spec = TeamSpec::new("routing", Topology::RoleRouted);
    spec.topology_config.route_fallback = RouteFallback::SendToCoordinator;

    assert!(spec.validate().is_err());
}

#[test]
fn coordinator_worker_topology_requires_explicit_member_ids() {
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let spec = TeamBuilder::new("triage", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .coordinator_worker(coordinator, vec![worker])
        .build();

    assert_eq!(spec.coordinator_id().unwrap(), coordinator);
    assert_eq!(spec.coordinator_workers(), &[worker]);
    assert!(spec.validate().is_ok());
}

#[test]
fn coordinator_worker_topology_rejects_missing_explicit_coordinator() {
    let worker = AgentId::new();
    let spec = TeamBuilder::new("triage", Topology::CoordinatorWorker)
        .member(worker, "worker", ContextVisibility::All)
        .coordinator_worker(AgentId::new(), vec![worker])
        .build();

    assert!(spec.validate().is_err());
}

#[test]
fn team_spec_rejects_unsupported_bus_and_lifecycle_modes() {
    let mut spec = TeamSpec::new("triage", Topology::PeerToPeer);
    spec.message_bus.persistence = BusPersistence::Durable;
    assert!(spec.validate().is_ok());

    let mut spec = TeamSpec::new("triage", Topology::PeerToPeer);
    spec.message_bus.ordering = MessageOrdering::Total;
    assert!(spec.validate().is_ok());

    let mut spec = TeamSpec::new("triage", Topology::PeerToPeer);
    spec.message_bus.replay_window = ReplayWindowSpec::Last(16);
    assert!(spec.validate().is_ok());

    let mut spec = TeamSpec::new("triage", Topology::PeerToPeer);
    spec.message_bus.backpressure = BusBackpressure::RejectNew;
    assert!(spec.validate().is_ok());

    let mut spec = TeamSpec::new("triage", Topology::PeerToPeer);
    spec.lifecycle = TeamLifecycle::Persistent {
        max_idle: Duration::from_secs(30),
    };
    assert!(spec.validate().is_ok());
}

#[test]
fn persistent_lifecycle_requires_idle_timeout() {
    let mut spec = TeamSpec::new("triage", Topology::PeerToPeer);
    spec.lifecycle = TeamLifecycle::Persistent {
        max_idle: Duration::ZERO,
    };

    assert!(spec.validate().is_err());
}

#[test]
fn team_turn_completed_contract_exposes_turn_id_and_participants() {
    let participant = AgentId::new();
    let event = Event::TeamTurnCompleted(harness_contracts::TeamTurnCompletedEvent {
        team_id: harness_contracts::TeamId::new(),
        turn_id: RunId::new(),
        participating_agents: vec![participant],
        usage: UsageSnapshot::default(),
        transcript_ref: None,
        at: harness_contracts::now(),
    });

    assert!(matches!(
        event,
        Event::TeamTurnCompleted(completed)
            if completed.participating_agents == vec![participant]
    ));
}

#[test]
fn crate_defaults_enable_stable_builtin_topologies() {
    let manifest = include_str!("../Cargo.toml");

    assert!(manifest.contains("default = [\"coordinator-worker\", \"peer-to-peer\"]"));
}

#[tokio::test]
async fn message_bus_fans_out_and_replays_messages() {
    let team = TeamSpec::new("triage", Topology::PeerToPeer);
    let bus = MessageBus::journaled(
        team.team_id,
        16,
        TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
        },
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
    );
    let mut rx_a = bus.subscribe();
    let mut rx_b = bus.subscribe();
    let message = AgentMessage::text(team.team_id, AgentId::new(), Recipient::Broadcast, "hello");

    bus.send(message.clone()).await.unwrap();

    assert_eq!(rx_a.recv().await.unwrap(), message);
    assert_eq!(rx_b.recv().await.unwrap(), message);
    assert_eq!(bus.replay().await, vec![message]);
}

#[test]
fn role_routed_strategy_resolves_members_by_role() {
    let analyst = AgentId::new();
    let reviewer = AgentId::new();
    let team = TeamBuilder::new("triage", Topology::RoleRouted)
        .member(analyst, "analyst", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .build();
    let message = AgentMessage::new(
        team.team_id,
        analyst,
        Recipient::Role("reviewer".to_owned()),
        MessagePayload::Text("check".to_owned()),
    );

    assert_eq!(RoleRoutedStrategy.route(&message, &team), vec![reviewer]);
}

#[test]
fn role_routed_strategy_uses_explicit_role_route_targets() {
    let analyst = AgentId::new();
    let reviewer = AgentId::new();
    let backup = AgentId::new();
    let team = TeamBuilder::new("triage", Topology::RoleRouted)
        .member(analyst, "analyst", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .member(backup, "reviewer", ContextVisibility::Private)
        .role_route("reviewer", vec![backup])
        .build();
    let message = AgentMessage::new(
        team.team_id,
        analyst,
        Recipient::Role("reviewer".to_owned()),
        MessagePayload::Text("check".to_owned()),
    );

    assert!(team.validate().is_ok());
    assert_eq!(RoleRoutedStrategy.route(&message, &team), vec![backup]);
}

#[test]
fn coordinator_rejects_normal_tool_execution() {
    let coordinator = Coordinator::default();

    assert!(coordinator.execute_normal_tool("read_file").is_err());
    assert!(coordinator.dispatch(AgentId::new(), "summarize").is_ok());
}
