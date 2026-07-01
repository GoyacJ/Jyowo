use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    AgentId, ContextVisibility, Event, MemberLeaveReason, ModelRef, NoopRedactor, SessionId,
    TeamId, TeamTerminationReason, TenantId,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
use harness_session::{session_options_hash, SessionOptions};
use harness_team::{
    CoordinatorWorkerRuntime, MessageBus, Team, TeamBuilder, TeamError, TeamJournalContext,
    TeamLifecycle, TeamMember, TeamMemberEngineConfig, TeamMemberRunOutcome, TeamMemberRunRequest,
    TeamMemberRunner, TeamProjection, TeamQuotaKind, Topology,
};
use tokio::sync::Notify;

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
async fn cancelling_runtime_terminates_active_member_runner() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let team = TeamBuilder::new("cancellable", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .coordinator_worker(coordinator, Vec::new())
        .build();
    let team_id = team.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let runner = Arc::new(CancellableRunner::default());
    let runtime = Arc::new(
        CoordinatorWorkerRuntime::new(
            team,
            bus,
            journal,
            store.clone(),
            Arc::new(InMemoryBlobStore::default()),
        )
        .with_member_runner(coordinator, runner.clone()),
    );
    let dispatch_runtime = Arc::clone(&runtime);
    let dispatch = tokio::spawn(async move { dispatch_runtime.dispatch_goal("inspect").await });

    runner.started.notified().await;
    runtime
        .terminate(TeamTerminationReason::Cancelled)
        .await
        .expect("runtime cancellation should terminate team");

    let dispatch_result = dispatch.await.expect("dispatch task should join");
    assert!(matches!(dispatch_result, Err(TeamError::TeamTerminated)));
    assert!(runner.cancelled.load(Ordering::SeqCst));

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
                if terminated.team_id == team_id
                    && terminated.reason == TeamTerminationReason::Cancelled
        )
    }));
}

#[derive(Default)]
struct CancellableRunner {
    started: Notify,
    cancelled: AtomicBool,
}

#[async_trait]
impl TeamMemberRunner for CancellableRunner {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, TeamError> {
        self.started.notify_waiters();
        request.cancellation.cancelled().await;
        self.cancelled.store(true, Ordering::SeqCst);
        Err(TeamError::TeamTerminated)
    }
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
async fn public_team_add_member_uses_team_workspace_root_for_member_session() {
    let workspace = std::env::temp_dir().join(format!(
        "jyowo-team-add-member-workspace-{}-{}",
        std::process::id(),
        SessionId::new()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    let canonical_workspace = workspace.canonicalize().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let owner = AgentId::new();
    let team_spec = TeamBuilder::new("dynamic-workspace", Topology::PeerToPeer)
        .member(owner, "owner", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(team_spec.team_id, 16, journal, store.clone());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let team = Team::new_with_workspace_root(
        team_spec,
        bus,
        journal,
        store.clone(),
        blob_store,
        workspace,
    );
    let worker = AgentId::new();

    team.add_member(TeamMember {
        agent_id: worker,
        role: "worker".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: TeamMemberEngineConfig::default(),
    })
    .await
    .expect("add member should emit joined event");

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let member_session_id = events
        .iter()
        .find_map(|event| match event {
            Event::TeamMemberJoined(joined) if joined.agent_id == worker => Some(joined.session_id),
            _ => None,
        })
        .expect("member joined event should include a session id");
    let member_events: Vec<_> = store
        .read(TenantId::SINGLE, member_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let created = member_events
        .iter()
        .find_map(|event| match event {
            Event::SessionCreated(created) => Some(created),
            _ => None,
        })
        .expect("member session should be created");
    let expected_hash = session_options_hash(
        &SessionOptions::new(canonical_workspace)
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(member_session_id),
    );

    assert_eq!(created.options_hash, expected_hash);
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
