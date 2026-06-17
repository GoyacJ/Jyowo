use std::{sync::Arc, time::Duration};

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    AgentId, ContextVisibility, Event, MemberLeaveReason, ModelRef, NoopRedactor, SessionId,
    TeamId, TeamTerminationReason, TenantId,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
use harness_team::{
    CoordinatorWorkerRuntime, MessageBus, Team, TeamBuilder, TeamError, TeamJournalContext,
    TeamLifecycle, TeamMember, TeamMemberEngineConfig, TeamProjection, TeamQuotaKind, Topology,
};

#[tokio::test]
async fn terminate_emits_member_left_and_team_terminated() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let team = TeamBuilder::new("lifecycle", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::Private)
        .coordinator_worker(coordinator, vec![worker])
        .build();
    let team_id: TeamId = team.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let runtime = CoordinatorWorkerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );

    let report = runtime
        .terminate(TeamTerminationReason::Cancelled)
        .await
        .unwrap();

    assert_eq!(report.team_id, team_id);
    assert_eq!(report.final_state["terminated"], "cancelled");
    assert_ne!(report.report_hash, [0; 32]);

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TeamMemberLeft(left) if left.reason == MemberLeaveReason::Interrupted))
            .count(),
        2
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamTerminated(terminated)
                if terminated.team_id == team_id
                    && terminated.reason == TeamTerminationReason::Cancelled
        )
    }));
}

#[tokio::test]
async fn public_team_adds_and_removes_members_with_lifecycle_events() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let owner = AgentId::new();
    let team_spec = TeamBuilder::new("dynamic", Topology::PeerToPeer)
        .member(owner, "owner", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(team_spec.team_id, 16, journal, store.clone());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let team = Team::new(team_spec, bus, journal, store.clone(), blob_store.clone());
    let worker = AgentId::new();
    let engine_config = TeamMemberEngineConfig {
        model_ref: Some(ModelRef {
            provider_id: "projection-test".to_owned(),
            model_id: "member-model".to_owned(),
        }),
        ..TeamMemberEngineConfig::default()
    };

    team.add_member(TeamMember {
        agent_id: worker,
        role: "worker".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: engine_config.clone(),
    })
    .await
    .expect("add member should emit joined event");
    team.remove_member(worker)
        .await
        .expect("remove member should emit left event");
    team.remove_member(worker)
        .await
        .expect("unknown remove should be idempotent");

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamMemberJoined(joined) if joined.agent_id == worker
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamMemberLeft(left)
                if left.agent_id == worker && left.reason == MemberLeaveReason::Removed
        )
    }));

    let projection =
        TeamProjection::replay(TenantId::SINGLE, session_id, store.clone(), blob_store)
            .await
            .expect("projection should replay team lifecycle");
    assert_eq!(projection.team_id, team.team_id().await);
    assert!(!projection.members.contains_key(&worker));
    assert!(projection.left_members.contains_key(&worker));
    assert_eq!(
        projection.left_members.get(&worker).unwrap().engine_config,
        engine_config
    );
    let observations = team.observation_snapshot().await;
    assert_eq!(observations.dynamic_member_adds, 1);
    assert_eq!(observations.dynamic_member_removes, 1);
}

#[tokio::test]
async fn public_team_add_member_respects_max_members_quota() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let owner = AgentId::new();
    let mut team_spec = TeamBuilder::new("quota", Topology::PeerToPeer)
        .member(owner, "owner", ContextVisibility::All)
        .build();
    team_spec.quota.max_members = Some(1);
    let bus = MessageBus::journaled(team_spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        team_spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let worker = AgentId::new();

    let error = team
        .add_member(TeamMember {
            agent_id: worker,
            role: "worker".to_owned(),
            visibility: ContextVisibility::All,
            engine_config: TeamMemberEngineConfig::default(),
        })
        .await
        .expect_err("max_members quota should reject dynamic add");

    assert!(matches!(
        error,
        TeamError::QuotaExceeded {
            kind: TeamQuotaKind::Members
        }
    ));
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(!events.iter().any(|event| {
        matches!(
            event,
            Event::TeamMemberJoined(joined) if joined.agent_id == worker
        )
    }));
}

#[tokio::test]
async fn persistent_team_idle_tick_terminates_after_max_idle() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let owner = AgentId::new();
    let mut team_spec = TeamBuilder::new("persistent", Topology::PeerToPeer)
        .member(owner, "owner", ContextVisibility::All)
        .build();
    team_spec.lifecycle = TeamLifecycle::Persistent {
        max_idle: Duration::from_secs(10),
    };
    let team_id = team_spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let team = Team::new(
        team_spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let now = Utc::now();

    assert!(team
        .lifecycle_tick(now + chrono::Duration::seconds(9))
        .await
        .unwrap()
        .is_none());
    let report = team
        .lifecycle_tick(now + chrono::Duration::seconds(11))
        .await
        .unwrap()
        .expect("persistent team should idle terminate");

    assert_eq!(report.team_id, team_id);
    assert_eq!(report.final_state["terminated"], "idle_timeout");
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamTerminated(terminated)
                if terminated.reason == TeamTerminationReason::IdleTimeout
        )
    }));
}

#[tokio::test]
async fn explicit_terminate_team_does_not_idle_terminate() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
    };
    let owner = AgentId::new();
    let mut team_spec = TeamBuilder::new("explicit", Topology::PeerToPeer)
        .member(owner, "owner", ContextVisibility::All)
        .build();
    team_spec.lifecycle = TeamLifecycle::ExplicitTerminate;
    let bus = MessageBus::journaled(team_spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        team_spec,
        bus,
        journal,
        store,
        Arc::new(InMemoryBlobStore::default()),
    );

    assert!(team
        .lifecycle_tick(Utc::now() + chrono::Duration::days(1))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn terminated_team_rejects_dispatch_and_post() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
    };
    let owner = AgentId::new();
    let team_spec = TeamBuilder::new("terminated", Topology::PeerToPeer)
        .member(owner, "owner", ContextVisibility::All)
        .build();
    let team_id = team_spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let team = Team::new(
        team_spec,
        bus,
        journal,
        store,
        Arc::new(InMemoryBlobStore::default()),
    );

    team.terminate(TeamTerminationReason::Cancelled)
        .await
        .unwrap();

    assert!(matches!(
        team.dispatch(owner, harness_contracts::Recipient::Broadcast, "after")
            .await,
        Err(TeamError::TeamTerminated)
    ));
    let message = harness_team::AgentMessage::text(
        team_id,
        owner,
        harness_contracts::Recipient::Broadcast,
        "after",
    );
    assert!(matches!(
        team.post(message).await,
        Err(TeamError::TeamTerminated)
    ));
}
