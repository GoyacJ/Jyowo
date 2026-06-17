use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::BlobStore;
use harness_contracts::{
    AgentId, CorrelationId, EndReason, Event, MemberLeaveReason, NoopRedactor, Recipient,
    RunEndedEvent, RunStartedEvent, SessionId, StalledAction, TeamTerminationReason, TenantId,
    UsageSnapshot,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
use harness_team::{
    AgentMessage, ContextVisibility, CoordinatorWorkerRuntime, MessageBus, MessagePayload,
    PeerToPeerRuntime, RoleRoutedRuntime, SharedMemorySpec, Team, TeamBuilder, TeamError,
    TeamJournalContext, TeamLifecycle, TeamMember, TeamMemberEngineConfig, TeamMemberRunOutcome,
    TeamMemberRunRequest, TeamMemberRunner, TeamQuotaKind, Topology,
};

#[tokio::test]
async fn coordinator_worker_dispatch_runs_coordinator_without_implicit_worker_fanout() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let coder = AgentId::new();
    let reviewer = AgentId::new();
    let team = TeamBuilder::new("implementation", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .member(coder, "coder", ContextVisibility::Private)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .coordinator_worker(coordinator, vec![coder, reviewer])
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = CoordinatorWorkerRuntime::new(
        team.clone(),
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        coordinator,
        Arc::new(RecordingMemberRunner::new(
            store.clone(),
            "coordinator completed",
            11,
        )),
    )
    .with_member_runner(
        coder,
        Arc::new(RecordingMemberRunner::new(store.clone(), "patch ready", 3)),
    )
    .with_member_runner(
        reviewer,
        Arc::new(RecordingMemberRunner::new(
            store.clone(),
            "review passed",
            5,
        )),
    );

    let report = runtime.dispatch_goal("ship m6").await.unwrap();

    assert_eq!(report.team_id, team.team_id);
    assert_eq!(report.message_count, 0);
    assert_eq!(report.final_state["coordinator_engine"], true);
    assert_eq!(report.final_state["responses"], 0);
    assert_eq!(report.members_usage[&coordinator].output_tokens, 11);
    assert!(!report.members_usage.contains_key(&coder));
    assert!(!report.members_usage.contains_key(&reviewer));
    let team_envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let events: Vec<_> = team_envelopes
        .iter()
        .map(|envelope| envelope.payload.clone())
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::AgentMessageSent(_)))
            .count(),
        0
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamCreated(_))));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TeamMemberJoined(_)))
            .count(),
        3
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamTurnCompleted(completed) if completed.usage.output_tokens == 11
        )
    }));
    let worker_response_bodies: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::AgentMessageSent(sent) if sent.from == coder || sent.from == reviewer => {
                match &sent.payload {
                    MessagePayload::Response { body, .. } => {
                        body.get("body").and_then(serde_json::Value::as_str)
                    }
                    _ => None,
                }
            }
            _ => None,
        })
        .collect();
    assert!(worker_response_bodies.is_empty());
    let turn_correlation = team_envelopes
        .iter()
        .find_map(|envelope| match envelope.payload {
            Event::TeamTurnCompleted(_) => Some(envelope.correlation_id),
            _ => None,
        })
        .unwrap();

    let all_events = store
        .query_after(TenantId::SINGLE, None, 128)
        .await
        .unwrap();
    assert_eq!(
        all_events
            .iter()
            .filter(|envelope| matches!(envelope.payload, Event::RunStarted(_)))
            .count(),
        1
    );
    assert_eq!(
        all_events
            .iter()
            .filter(|envelope| matches!(envelope.payload, Event::RunEnded(_)))
            .count(),
        1
    );
    assert!(all_events.iter().all(|envelope| {
        !matches!(envelope.payload, Event::RunStarted(_) | Event::RunEnded(_))
            || envelope.correlation_id == turn_correlation
    }));
}

#[tokio::test]
async fn coordinator_worker_requires_registered_coordinator_runner() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let team = TeamBuilder::new("coordinator-required", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .coordinator_worker(coordinator, vec![worker])
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = CoordinatorWorkerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        worker,
        Arc::new(RecordingMemberRunner::new(store, "should not run", 1)),
    );

    let error = runtime
        .dispatch_goal("coordinate")
        .await
        .expect_err("missing coordinator runner should fail closed");

    assert!(matches!(
        error,
        TeamError::InvalidSpec(message)
            if message.contains("coordinator runner is required")
    ));
}

#[tokio::test]
async fn coordinator_worker_turn_limit_times_out_team() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let mut team = TeamBuilder::new("coordinator-limit", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .coordinator_worker(coordinator, vec![])
        .build();
    team.max_turns_per_goal = 1;
    let team_id = team.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let runtime = CoordinatorWorkerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        coordinator,
        Arc::new(RecordingMemberRunner::new(store.clone(), "done", 1)),
    );

    runtime.dispatch_goal("same").await.unwrap();
    let error = runtime
        .dispatch_goal("same")
        .await
        .expect_err("second coordinator turn should timeout team");

    assert!(matches!(
        error,
        TeamError::TurnLimitExceeded {
            team_id: actual_team_id,
            limit: 1
        } if actual_team_id == team_id
    ));
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
                if terminated.reason == TeamTerminationReason::Timeout
        )
    }));
}

#[tokio::test]
async fn coordinator_worker_runs_coordinator_engine_when_runner_is_registered() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let team = TeamBuilder::new("coordinator-engine", Topology::CoordinatorWorker)
        .member(coordinator, "lead", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .coordinator_worker(coordinator, vec![worker])
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = CoordinatorWorkerRuntime::new(
        team.clone(),
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        coordinator,
        Arc::new(RecordingMemberRunner::new(store.clone(), "plan issued", 7)),
    );

    let report = runtime.dispatch_goal("coordinate").await.unwrap();

    assert_eq!(report.final_state["coordinator_engine"], true);
    assert_eq!(report.members_usage[&coordinator].output_tokens, 7);
    assert_eq!(report.message_count, 0);
}

#[tokio::test]
async fn peer_to_peer_turn_limit_times_out_team() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let mut team = TeamBuilder::new("peer-limit", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    team.max_turns_per_goal = 1;
    let team_id = team.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        worker,
        Arc::new(RecordingMemberRunner::new(store.clone(), "done", 1)),
    );

    runtime.dispatch_goal(initiator, "same").await.unwrap();
    let error = runtime
        .dispatch_goal(initiator, "same")
        .await
        .expect_err("second peer turn should timeout team");

    assert!(matches!(
        error,
        TeamError::TurnLimitExceeded {
            team_id: actual_team_id,
            limit: 1
        } if actual_team_id == team_id
    ));
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
                if terminated.reason == TeamTerminationReason::Timeout
        )
    }));
}

#[tokio::test]
async fn team_runtime_injects_shared_memory_with_team_correlation() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let mut team = TeamBuilder::new("memory", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    team.shared_memory = SharedMemorySpec::Enabled {
        provider_id: "team-shared".to_owned(),
        write_policy: harness_team::SharedWritePolicy::Unrestricted,
    };
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(worker, Arc::new(MemoryWritingRunner));

    runtime.dispatch_goal(initiator, "remember").await.unwrap();

    let envelopes = store
        .query_after(TenantId::SINGLE, None, 128)
        .await
        .unwrap();
    let turn_correlation = envelopes
        .iter()
        .find_map(|envelope| match envelope.payload {
            Event::TeamTurnCompleted(_) => Some(envelope.correlation_id),
            _ => None,
        })
        .unwrap();
    assert!(envelopes.iter().any(|envelope| {
        envelope.correlation_id == turn_correlation
            && matches!(envelope.payload, Event::MemoryUpserted(_))
    }));
}

#[tokio::test]
async fn team_post_rejects_active_member_wrong_correlation() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let spec = TeamBuilder::new("active-correlation", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec.clone(),
        bus,
        journal,
        store,
        Arc::new(InMemoryBlobStore::default()),
    );
    let runtime = PeerToPeerRuntime::from_team(team.clone())
        .with_member_runner(worker, Arc::new(WrongCorrelationPostRunner { team }));

    let error = runtime
        .dispatch_goal(initiator, "post wrong")
        .await
        .unwrap_err();

    assert!(matches!(error, TeamError::CorrelationMismatch { .. }));
}

#[tokio::test]
async fn peer_runtime_sees_members_added_through_public_team_after_initialization() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let first = AgentId::new();
    let late = AgentId::new();
    let spec = TeamBuilder::new("dynamic-runtime", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(first, "first", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec.clone(),
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let runtime = PeerToPeerRuntime::from_team(team.clone())
        .with_member_runner(
            first,
            Arc::new(RecordingMemberRunner::new(store.clone(), "first ready", 1)),
        )
        .with_member_runner(
            late,
            Arc::new(RecordingMemberRunner::new(store.clone(), "late ready", 7)),
        );

    runtime
        .dispatch_goal(initiator, "first pass")
        .await
        .unwrap();
    team.add_member(TeamMember {
        agent_id: late,
        role: "late".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: TeamMemberEngineConfig::default(),
    })
    .await
    .unwrap();
    let report = runtime
        .dispatch_goal(initiator, "second pass")
        .await
        .unwrap();

    assert!(report.members_usage.contains_key(&first));
    assert!(report.members_usage.contains_key(&late));
    assert_eq!(report.members_usage[&late].output_tokens, 7);
}

#[tokio::test]
async fn peer_runtime_watchdog_reports_stalled_members() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let team = TeamBuilder::new("watchdog", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        worker,
        Arc::new(RecordingMemberRunner::new(store.clone(), "ready", 1)),
    );

    runtime.dispatch_goal(initiator, "sync").await.unwrap();
    let stalled = runtime.watchdog_tick(Duration::ZERO).await.unwrap();

    assert!(stalled.contains(&worker));
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamMemberStalled(stalled) if stalled.agent_id == worker
        )
    }));
}

#[tokio::test]
async fn peer_runtime_watchdog_removed_marks_member_left_and_removes_from_runtime() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let team = TeamBuilder::new("watchdog-remove", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        worker,
        Arc::new(RecordingMemberRunner::new(store.clone(), "ready", 1)),
    );

    runtime.dispatch_goal(initiator, "sync").await.unwrap();
    let stalled = runtime
        .watchdog_tick_with_action(Duration::ZERO, StalledAction::Removed)
        .await
        .unwrap();

    assert!(stalled.contains(&worker));
    let report = runtime.dispatch_goal(initiator, "next").await.unwrap();
    assert!(!report.members_usage.contains_key(&worker));
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamMemberStalled(stalled)
                if stalled.agent_id == worker && stalled.action == StalledAction::Removed
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamMemberLeft(left)
                if left.agent_id == worker && left.reason == MemberLeaveReason::StalledRemoved
        )
    }));
}

#[tokio::test]
async fn persistent_runtime_can_dispatch_multiple_goals_before_idle_timeout() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let mut team = TeamBuilder::new("persistent-runtime", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    team.lifecycle = TeamLifecycle::Persistent {
        max_idle: Duration::from_secs(60),
    };
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        worker,
        Arc::new(RecordingMemberRunner::new(store.clone(), "ready", 1)),
    );

    runtime.dispatch_goal(initiator, "first").await.unwrap();
    let report = runtime.dispatch_goal(initiator, "second").await.unwrap();
    let now = Utc::now();

    assert_eq!(report.final_state["responses"], 1);
    assert!(runtime
        .lifecycle_tick(now + chrono::Duration::seconds(59))
        .await
        .unwrap()
        .is_none());
    let terminated = runtime
        .lifecycle_tick(now + chrono::Duration::seconds(61))
        .await
        .unwrap()
        .expect("persistent runtime should idle terminate");
    assert_eq!(terminated.final_state["terminated"], "idle_timeout");

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TeamTurnCompleted(_)))
            .count(),
        2
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamTerminated(terminated)
                if terminated.reason == TeamTerminationReason::IdleTimeout
        )
    }));
}

#[tokio::test]
async fn team_turn_completed_records_participating_agents() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let coder = AgentId::new();
    let reviewer = AgentId::new();
    let team = TeamBuilder::new("participants", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(coder, "coder", ContextVisibility::Private)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        coder,
        Arc::new(RecordingMemberRunner::new(store.clone(), "patch ready", 2)),
    )
    .with_member_runner(
        reviewer,
        Arc::new(RecordingMemberRunner::new(
            store.clone(),
            "review passed",
            4,
        )),
    );

    runtime
        .dispatch_goal(initiator, "sync participants")
        .await
        .unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let completed = events
        .iter()
        .find_map(|event| match event {
            Event::TeamTurnCompleted(completed) => Some(completed),
            _ => None,
        })
        .expect("turn completion should be recorded");
    assert_eq!(completed.participating_agents.len(), 2);
    assert!(completed.participating_agents.contains(&coder));
    assert!(completed.participating_agents.contains(&reviewer));
}

#[tokio::test]
async fn peer_to_peer_dispatch_runs_peers_and_completes_turn() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let coder = AgentId::new();
    let reviewer = AgentId::new();
    let team = TeamBuilder::new("peer-team", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(coder, "coder", ContextVisibility::Private)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team.clone(),
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        coder,
        Arc::new(RecordingMemberRunner::new(store.clone(), "patch ready", 2)),
    )
    .with_member_runner(
        reviewer,
        Arc::new(RecordingMemberRunner::new(
            store.clone(),
            "review passed",
            4,
        )),
    );

    let report = runtime.dispatch_goal(initiator, "sync m6").await.unwrap();

    assert_eq!(report.team_id, team.team_id);
    assert_eq!(report.message_count, 3);
    assert_eq!(report.final_state["responses"], 2);
    assert_eq!(report.members_usage[&coder].output_tokens, 2);
    assert_eq!(report.members_usage[&reviewer].output_tokens, 4);
    let team_envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let events: Vec<_> = team_envelopes
        .iter()
        .map(|envelope| envelope.payload.clone())
        .collect();
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamCreated(_))));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TeamMemberJoined(_)))
            .count(),
        3
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::TeamTurnCompleted(completed) if completed.usage.output_tokens == 6
        )
    }));
    assert_eq!(
        store
            .query_after(TenantId::SINGLE, None, 128)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.payload, Event::RunStarted(_)))
            .count(),
        2
    );
}

#[tokio::test]
async fn peer_to_peer_dispatch_respects_max_messages_quota() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let mut team = TeamBuilder::new("message-quota", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    team.quota.max_messages = Some(1);
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        worker,
        Arc::new(RecordingMemberRunner::new(store.clone(), "ready", 1)),
    );

    let error = runtime
        .dispatch_goal(initiator, "quota")
        .await
        .expect_err("second message in turn should exceed max_messages quota");

    assert!(matches!(
        error,
        TeamError::QuotaExceeded {
            kind: TeamQuotaKind::Messages
        }
    ));
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
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::TeamTurnCompleted(_))));
}

#[tokio::test]
async fn peer_to_peer_dispatch_respects_max_duration_quota() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let worker = AgentId::new();
    let mut team = TeamBuilder::new("duration-quota", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    team.quota.max_duration = Some(Duration::from_millis(1));
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(worker, Arc::new(SlowMemberRunner));

    let error = runtime
        .dispatch_goal(initiator, "slow")
        .await
        .expect_err("slow member should exceed max_duration quota");

    assert!(matches!(
        error,
        TeamError::QuotaExceeded {
            kind: TeamQuotaKind::WallClock
        }
    ));
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::TeamTurnCompleted(_))));
}

#[tokio::test]
async fn team_turn_completed_writes_transcript_blob_when_enabled() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let initiator = AgentId::new();
    let coder = AgentId::new();
    let reviewer = AgentId::new();
    let mut team = TeamBuilder::new("transcript", Topology::PeerToPeer)
        .member(initiator, "initiator", ContextVisibility::All)
        .member(coder, "coder", ContextVisibility::Private)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .build();
    team.observability.capture_transcript = true;
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = PeerToPeerRuntime::new(team, bus, journal, store.clone(), blob_store.clone())
        .with_member_runner(
            coder,
            Arc::new(RecordingMemberRunner::new(store.clone(), "patch ready", 2)),
        )
        .with_member_runner(
            reviewer,
            Arc::new(RecordingMemberRunner::new(
                store.clone(),
                "review passed",
                4,
            )),
        );

    runtime
        .dispatch_goal(initiator, "sync transcript")
        .await
        .unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let transcript_ref = events
        .iter()
        .find_map(|event| match event {
            Event::TeamTurnCompleted(completed) => completed.transcript_ref.as_ref(),
            _ => None,
        })
        .expect("turn completion should carry transcript ref");
    let mut stream = blob_store
        .get(TenantId::SINGLE, &transcript_ref.blob)
        .await
        .expect("transcript blob should exist");
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk);
    }
    let transcript: serde_json::Value =
        serde_json::from_slice(&bytes).expect("transcript should be json");
    assert_eq!(
        transcript["participating_agents"]
            .as_array()
            .expect("participants should be an array")
            .len(),
        2
    );
    assert_eq!(transcript["responses"].as_array().unwrap().len(), 2);
    assert_eq!(transcript["usage"]["output_tokens"], 6);
}

#[tokio::test]
async fn role_routed_dispatch_runs_matching_members_and_completes_turn() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let coder = AgentId::new();
    let reviewer = AgentId::new();
    let team = TeamBuilder::new("role-team", Topology::RoleRouted)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .member(coder, "coder", ContextVisibility::Private)
        .member(reviewer, "reviewer", ContextVisibility::Private)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = RoleRoutedRuntime::new(
        team.clone(),
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        reviewer,
        Arc::new(RecordingMemberRunner::new(
            store.clone(),
            "review passed",
            9,
        )),
    );

    let report = runtime
        .dispatch_goal(
            coordinator,
            Recipient::Role("reviewer".to_owned()),
            "review m6",
        )
        .await
        .unwrap();

    assert_eq!(report.team_id, team.team_id);
    assert_eq!(report.message_count, 2);
    assert_eq!(report.final_state["responses"], 1);
    assert_eq!(report.members_usage[&reviewer].output_tokens, 9);
    assert!(!report.members_usage.contains_key(&coder));
    let team_events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(team_events.iter().any(|event| {
        matches!(
            event,
            Event::TeamTurnCompleted(completed) if completed.usage.output_tokens == 9
        )
    }));
    assert_eq!(
        store
            .query_after(TenantId::SINGLE, None, 128)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.payload, Event::RunEnded(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn role_routed_turn_limit_times_out_team() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let reviewer = AgentId::new();
    let mut team = TeamBuilder::new("role-limit", Topology::RoleRouted)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::All)
        .build();
    team.max_turns_per_goal = 1;
    let team_id = team.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let runtime = RoleRoutedRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_member_runner(
        reviewer,
        Arc::new(RecordingMemberRunner::new(store.clone(), "done", 1)),
    );

    runtime
        .dispatch_goal(coordinator, Recipient::Role("reviewer".to_owned()), "same")
        .await
        .unwrap();
    let error = runtime
        .dispatch_goal(coordinator, Recipient::Role("reviewer".to_owned()), "same")
        .await
        .expect_err("second role-routed turn should timeout team");

    assert!(matches!(
        error,
        TeamError::TurnLimitExceeded {
            team_id: actual_team_id,
            limit: 1
        } if actual_team_id == team_id
    ));
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
                if terminated.reason == TeamTerminationReason::Timeout
        )
    }));
}

#[tokio::test]
async fn spawn_worker_with_runner_makes_new_member_runnable() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let spawned = AgentId::new();
    let team = TeamBuilder::new("dynamic-role-team", Topology::RoleRouted)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = RoleRoutedRuntime::new(
        team.clone(),
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );

    runtime
        .control_handle()
        .spawn_worker_with_runner(
            TeamMember {
                agent_id: spawned,
                role: "reviewer".to_owned(),
                visibility: ContextVisibility::All,
                engine_config: TeamMemberEngineConfig::default(),
            },
            Arc::new(RecordingMemberRunner::new(
                store.clone(),
                "spawned reviewer ready",
                4,
            )),
        )
        .await
        .unwrap();
    let report = runtime
        .dispatch_goal(
            coordinator,
            Recipient::Role("reviewer".to_owned()),
            "review",
        )
        .await
        .unwrap();

    assert_eq!(report.final_state["responses"], 1);
    assert_eq!(report.members_usage[&spawned].output_tokens, 4);
}

#[tokio::test]
async fn spawn_worker_reuses_joined_session_for_runtime_request() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let coordinator = AgentId::new();
    let spawned = AgentId::new();
    let team = TeamBuilder::new("dynamic-session-team", Topology::RoleRouted)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(team.team_id, 16, journal, store.clone());
    let runtime = RoleRoutedRuntime::new(
        team,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let observed_session = Arc::new(tokio::sync::Mutex::new(None));

    runtime
        .control_handle()
        .spawn_worker_with_runner(
            TeamMember {
                agent_id: spawned,
                role: "reviewer".to_owned(),
                visibility: ContextVisibility::All,
                engine_config: TeamMemberEngineConfig::default(),
            },
            Arc::new(CapturingSessionRunner {
                observed_session: Arc::clone(&observed_session),
            }),
        )
        .await
        .unwrap();

    runtime
        .dispatch_goal(
            coordinator,
            Recipient::Role("reviewer".to_owned()),
            "review",
        )
        .await
        .unwrap();

    let joined_session = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .filter_map(|event| async move {
            match event {
                Event::TeamMemberJoined(joined) if joined.agent_id == spawned => {
                    Some(joined.session_id)
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .next()
        .expect("spawned member should have joined event");
    let runtime_session = observed_session
        .lock()
        .await
        .expect("runner should observe request session");

    assert_eq!(joined_session, runtime_session);
}

struct RecordingMemberRunner {
    store: Arc<InMemoryEventStore>,
    response: &'static str,
    output_tokens: u64,
}

struct CapturingSessionRunner {
    observed_session: Arc<tokio::sync::Mutex<Option<SessionId>>>,
}

struct MemoryWritingRunner;

#[async_trait]
impl TeamMemberRunner for MemoryWritingRunner {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, harness_team::TeamError> {
        let memory = request
            .shared_memory
            .as_ref()
            .expect("runtime should inject shared memory");
        memory
            .write_from_context(request.memory_write_context()?, "member fact")
            .await?;
        Ok(TeamMemberRunOutcome {
            body: "wrote memory".to_owned(),
            usage: UsageSnapshot::default(),
        })
    }
}

#[async_trait]
impl TeamMemberRunner for CapturingSessionRunner {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, harness_team::TeamError> {
        *self.observed_session.lock().await = Some(request.session_id);
        Ok(TeamMemberRunOutcome {
            body: "captured session".to_owned(),
            usage: UsageSnapshot::default(),
        })
    }
}

struct WrongCorrelationPostRunner {
    team: Team,
}

struct SlowMemberRunner;

#[async_trait]
impl TeamMemberRunner for WrongCorrelationPostRunner {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, harness_team::TeamError> {
        let message = AgentMessage::text_with_correlation(
            request.team_id,
            request.agent_id,
            Recipient::Coordinator,
            "wrong correlation",
            CorrelationId::new(),
        );
        self.team.post(message).await?;
        Ok(TeamMemberRunOutcome {
            body: "unexpected".to_owned(),
            usage: UsageSnapshot::default(),
        })
    }
}

impl RecordingMemberRunner {
    fn new(store: Arc<InMemoryEventStore>, response: &'static str, output_tokens: u64) -> Self {
        Self {
            store,
            response,
            output_tokens,
        }
    }
}

#[async_trait]
impl TeamMemberRunner for SlowMemberRunner {
    async fn run_member(
        &self,
        _request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, harness_team::TeamError> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(TeamMemberRunOutcome {
            body: "slow".to_owned(),
            usage: UsageSnapshot::default(),
        })
    }
}

#[async_trait]
impl TeamMemberRunner for RecordingMemberRunner {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, harness_team::TeamError> {
        let usage = UsageSnapshot {
            input_tokens: 1,
            output_tokens: self.output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: self.output_tokens,
            tool_calls: 0,
        };
        self.store
            .append_with_metadata(
                request.tenant_id,
                request.session_id,
                harness_journal::AppendMetadata {
                    run_id: Some(request.run_id),
                    correlation_id: request.correlation_id,
                    ..harness_journal::AppendMetadata::default()
                },
                &[
                    Event::RunStarted(RunStartedEvent {
                        run_id: request.run_id,
                        session_id: request.session_id,
                        tenant_id: request.tenant_id,
                        parent_run_id: request.parent_run_id,
                        input: request.input.clone(),
                        snapshot_id: harness_contracts::SnapshotId::from_u128(1),
                        effective_config_hash: harness_contracts::ConfigHash([0; 32]),
                        started_at: harness_contracts::now(),
                        correlation_id: request.correlation_id,
                    }),
                    Event::RunEnded(RunEndedEvent {
                        run_id: request.run_id,
                        reason: EndReason::Completed,
                        usage: Some(usage.clone()),
                        ended_at: harness_contracts::now(),
                    }),
                ],
            )
            .await
            .map_err(|error| harness_team::TeamError::Journal(error.to_string()))?;
        Ok(TeamMemberRunOutcome {
            body: format!("{}: {}", request.goal, self.response),
            usage,
        })
    }
}
