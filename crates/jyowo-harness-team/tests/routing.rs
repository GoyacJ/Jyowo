use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    AgentId, ContextVisibility, CorrelationId, Event, ModelError, NoopRedactor, Recipient,
    SessionId, TeamTerminationReason, TenantId, UsageSnapshot,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    AuxModelProvider, AuxOptions, AuxTask, ConversationModelCapability, HealthStatus, InferContext,
    ModelDescriptor, ModelProvider, ModelRequest, ModelStream,
};
use harness_team::{
    AgentMessage, AuxRoleClassifier, ClassifierError, ClassifierVerdict, MessageBus,
    MessageClassifier, RoleMessageClassifier, RoleRoutedRuntime, RoleRoutingRule, RoleRoutingTable,
    RouteFallback, RoutingPattern, Team, TeamBuilder, TeamError, TeamJournalContext,
    TeamMemberRunOutcome, TeamMemberRunRequest, TeamMemberRunner, Topology, TopologyStrategy,
};
use tokio::sync::Mutex;

#[tokio::test]
async fn public_team_filters_broadcast_by_context_visibility() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let public_worker = AgentId::new();
    let private_worker = AgentId::new();
    let spec = TeamBuilder::new("routing", Topology::PeerToPeer)
        .member(sender, "sender", ContextVisibility::All)
        .member(public_worker, "public", ContextVisibility::All)
        .member(private_worker, "private", ContextVisibility::Private)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );

    team.dispatch(sender, Recipient::Broadcast, "hello")
        .await
        .unwrap();
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AgentMessageRouted(routed)
                if routed.resolved_recipients == vec![public_worker]
        )
    }));
    let observations = team.observation_snapshot().await;
    assert_eq!(observations.context_visibility_blocked, 1);
}

#[tokio::test]
async fn private_visibility_keeps_sender_visible_on_own_broadcast() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let private_sender = AgentId::new();
    let public_worker = AgentId::new();
    let spec = TeamBuilder::new("routing", Topology::RoleRouted)
        .member(private_sender, "sender", ContextVisibility::Private)
        .member(public_worker, "public", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );

    team.dispatch(private_sender, Recipient::Broadcast, "hello")
        .await
        .unwrap();
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AgentMessageRouted(routed)
                if routed.resolved_recipients.contains(&private_sender)
                    && routed.resolved_recipients.contains(&public_worker)
        )
    }));
}

#[tokio::test]
async fn public_team_enforces_turn_limit_per_goal() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let worker = AgentId::new();
    let mut spec = TeamBuilder::new("limits", Topology::PeerToPeer)
        .member(sender, "sender", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    spec.max_turns_per_goal = 1;
    let team_id = spec.team_id;
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );

    team.dispatch(sender, Recipient::Broadcast, "same")
        .await
        .unwrap();
    let error = team
        .dispatch(sender, Recipient::Broadcast, "same")
        .await
        .expect_err("second turn for same goal should exceed limit");
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
    assert!(matches!(
        team.dispatch(sender, Recipient::Broadcast, "after-timeout")
            .await,
        Err(TeamError::TeamTerminated)
    ));
}

#[tokio::test]
async fn routing_limit_records_engine_failed_audit_event() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let worker = AgentId::new();
    let mut spec = TeamBuilder::new("message-limit", Topology::PeerToPeer)
        .member(sender, "sender", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    spec.max_messages_per_correlation = 1;
    let team_id = spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let correlation_id = CorrelationId::new();

    team.post(AgentMessage::text_with_correlation(
        team_id,
        sender,
        Recipient::Agent(worker),
        "first",
        correlation_id,
    ))
    .await
    .unwrap();
    team.post(AgentMessage::text_with_correlation(
        team_id,
        sender,
        Recipient::Agent(worker),
        "second",
        correlation_id,
    ))
    .await
    .expect("second message should route through fallback after audit");

    let envelopes: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(envelopes.iter().any(|envelope| {
        envelope.correlation_id == correlation_id
            && matches!(
                &envelope.payload,
                Event::EngineFailed(event)
                    if event
                        .error
                        .to_string()
                        .contains("cyclic routing: max_messages_per_correlation")
            )
    }));
}

#[tokio::test]
async fn role_rules_route_by_keyword_priority_before_fallback() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let reviewer = AgentId::new();
    let fallback = AgentId::new();
    let mut spec = TeamBuilder::new("rules", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::All)
        .member(fallback, "fallback", ContextVisibility::All)
        .role_route("fallback", vec![fallback])
        .build();
    spec.topology_config
        .role_rules
        .push(harness_team::RoleRoutingRule {
            rule_id: "security-review".to_owned(),
            priority: 20,
            pattern: RoutingPattern::KeywordAny {
                keywords: vec!["security".to_owned()],
                roles: vec!["reviewer".to_owned()],
            },
        });
    spec.topology_config.route_fallback = RouteFallback::Broadcast;
    let team_id = spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );

    team.dispatch(
        sender,
        Recipient::Role("unknown".to_owned()),
        "security review",
    )
    .await
    .unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AgentMessageRouted(routed)
                if routed.resolved_recipients == vec![reviewer]
        )
    }));
}

#[tokio::test]
async fn classifier_timeout_routes_to_configured_fallback() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let mut spec = TeamBuilder::new("classifier", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .role_route("worker", vec![worker])
        .build();
    spec.topology_config.coordinator = Some(coordinator);
    spec.topology_config.route_fallback = RouteFallback::SendToCoordinator;
    spec.topology_config
        .role_rules
        .push(harness_team::RoleRoutingRule {
            rule_id: "slow-classifier".to_owned(),
            priority: 10,
            pattern: RoutingPattern::Classifier {
                classifier_id: "slow".to_owned(),
            },
        });
    let team_id = spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    team.register_classifier(Arc::new(SlowClassifier))
        .await
        .unwrap();

    team.dispatch(
        sender,
        Recipient::Role("unknown".to_owned()),
        "please route",
    )
    .await
    .unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AgentMessageRouted(routed)
                if routed.resolved_recipients == vec![coordinator]
        )
    }));
    let observations = team.observation_snapshot().await;
    assert_eq!(observations.classifier_timeouts, 1);
    assert!(observations.classifier_confidences.is_empty());
}

#[tokio::test]
async fn classifier_confidence_is_recorded_for_successful_verdict() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let worker = AgentId::new();
    let mut spec = TeamBuilder::new("classifier-confidence", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    spec.topology_config
        .role_rules
        .push(harness_team::RoleRoutingRule {
            rule_id: "confident-classifier".to_owned(),
            priority: 10,
            pattern: RoutingPattern::Classifier {
                classifier_id: "confident".to_owned(),
            },
        });
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store,
        Arc::new(InMemoryBlobStore::default()),
    );
    team.register_classifier(Arc::new(ConfidentClassifier))
        .await
        .unwrap();

    team.dispatch(sender, Recipient::Role("unknown".to_owned()), "route")
        .await
        .unwrap();

    let observations = team.observation_snapshot().await;
    assert_eq!(
        observations.classifier_confidences,
        vec![harness_team::ClassifierConfidenceObservation {
            classifier_id: "confident".to_owned(),
            confidence: 0.42,
        }]
    );
}

#[tokio::test]
async fn custom_topology_uses_registered_strategy() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let target = AgentId::new();
    let mut spec = TeamBuilder::new("custom", Topology::Custom)
        .member(sender, "sender", ContextVisibility::All)
        .member(target, "target", ContextVisibility::All)
        .build();
    spec.topology_config.custom_strategy_id = Some("fixed".to_owned());
    let team_id = spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    team.register_topology_strategy(Arc::new(FixedStrategy { target }))
        .await
        .unwrap();

    team.dispatch(sender, Recipient::Broadcast, "custom")
        .await
        .unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AgentMessageRouted(routed)
                if routed.resolved_recipients == vec![target]
        )
    }));
}

#[tokio::test]
async fn routing_limit_falls_back_without_broadcasting_dropped_message() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let worker = AgentId::new();
    let mut spec = TeamBuilder::new("limit-fallback", Topology::PeerToPeer)
        .member(sender, "sender", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .build();
    spec.max_messages_per_correlation = 1;
    spec.topology_config.route_fallback = RouteFallback::DropMessage;
    let team_id = spec.team_id;
    let bus = MessageBus::journaled(team_id, 16, journal, store.clone());
    let mut rx = bus.subscribe();
    let team = Team::new(
        spec,
        bus,
        journal,
        store.clone(),
        Arc::new(InMemoryBlobStore::default()),
    );
    let correlation_id = CorrelationId::new();

    team.post(AgentMessage::text_with_correlation(
        team_id,
        sender,
        Recipient::Agent(worker),
        "first",
        correlation_id,
    ))
    .await
    .unwrap();
    team.post(AgentMessage::text_with_correlation(
        team_id,
        sender,
        Recipient::Agent(worker),
        "second",
        correlation_id,
    ))
    .await
    .unwrap();

    assert_eq!(
        rx.recv().await.unwrap().payload,
        harness_team::MessagePayload::Text("first".to_owned())
    );
    assert!(tokio::time::timeout(Duration::from_millis(25), rx.recv())
        .await
        .is_err());
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AgentMessageRouted(routed)
                if routed.resolved_recipients.is_empty()
        )
    }));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::EngineFailed(_))));
}

struct SlowClassifier;

#[async_trait]
impl MessageClassifier for SlowClassifier {
    fn classifier_id(&self) -> &str {
        "slow"
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(1)
    }

    async fn classify(
        &self,
        _message: &AgentMessage,
    ) -> Result<ClassifierVerdict, ClassifierError> {
        tokio::time::sleep(Duration::from_millis(25)).await;
        Ok(ClassifierVerdict {
            roles: vec!["worker".to_owned()],
            confidence: 1.0,
        })
    }
}

struct ConfidentClassifier;

#[async_trait]
impl MessageClassifier for ConfidentClassifier {
    fn classifier_id(&self) -> &str {
        "confident"
    }

    async fn classify(
        &self,
        _message: &AgentMessage,
    ) -> Result<ClassifierVerdict, ClassifierError> {
        Ok(ClassifierVerdict {
            roles: vec!["worker".to_owned()],
            confidence: 0.42,
        })
    }
}

struct FixedStrategy {
    target: AgentId,
}

#[async_trait]
impl TopologyStrategy for FixedStrategy {
    fn strategy_id(&self) -> &str {
        "fixed"
    }

    async fn route(
        &self,
        _message: &AgentMessage,
        _team: &harness_team::TeamSpec,
    ) -> Result<Vec<AgentId>, TeamError> {
        Ok(vec![self.target])
    }
}

#[tokio::test]
async fn role_routed_runtime_uses_aux_classify_to_pick_role() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let reviewer = AgentId::new();
    let implementer = AgentId::new();
    let spec = TeamBuilder::new("classified-routing", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::All)
        .member(implementer, "implementer", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store);
    let aux = Arc::new(RecordingAuxProvider::new(Ok(
        serde_json::json!({ "roles": ["reviewer"] }).to_string(),
    )));
    let reviewer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let implementer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let runtime = RoleRoutedRuntime::new(
        spec,
        bus,
        journal,
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_aux_role_classifier(Arc::new(AuxRoleClassifier::new(aux.clone())))
    .with_member_runner(reviewer, reviewer_runner.clone())
    .with_member_runner(implementer, implementer_runner.clone());

    runtime
        .dispatch_goal_classified(
            sender,
            "needs review",
            Recipient::Role("implementer".to_owned()),
        )
        .await
        .unwrap();

    assert_eq!(aux.tasks().await, vec![AuxTask::Classify]);
    assert_eq!(reviewer_runner.roles().await, vec!["reviewer".to_owned()]);
    assert!(implementer_runner.roles().await.is_empty());
}

#[tokio::test]
async fn role_routed_runtime_falls_back_when_aux_classify_returns_no_valid_role() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let reviewer = AgentId::new();
    let implementer = AgentId::new();
    let spec = TeamBuilder::new("classified-routing", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::All)
        .member(implementer, "implementer", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store);
    let aux = Arc::new(RecordingAuxProvider::new(Ok(
        serde_json::json!({ "roles": ["missing"] }).to_string(),
    )));
    let reviewer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let implementer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let runtime = RoleRoutedRuntime::new(
        spec,
        bus,
        journal,
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_aux_role_classifier(Arc::new(AuxRoleClassifier::new(aux.clone())))
    .with_member_runner(reviewer, reviewer_runner.clone())
    .with_member_runner(implementer, implementer_runner.clone());

    runtime
        .dispatch_goal_classified(
            sender,
            "needs implementation",
            Recipient::Role("implementer".to_owned()),
        )
        .await
        .unwrap();

    assert_eq!(aux.tasks().await, vec![AuxTask::Classify]);
    assert!(reviewer_runner.roles().await.is_empty());
    assert_eq!(
        implementer_runner.roles().await,
        vec!["implementer".to_owned()]
    );
}

#[tokio::test]
async fn role_routing_table_uses_highest_priority_matching_rule() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let reviewer = AgentId::new();
    let implementer = AgentId::new();
    let spec = TeamBuilder::new("table-routing", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::All)
        .member(implementer, "implementer", ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store);
    let reviewer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let implementer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let table = RoleRoutingTable::new(
        vec![
            RoleRoutingRule {
                rule_id: "implement".to_owned(),
                priority: 1,
                pattern: RoutingPattern::KeywordAny {
                    keywords: vec!["needs".to_owned()],
                    roles: vec!["implementer".to_owned()],
                },
            },
            RoleRoutingRule {
                rule_id: "review".to_owned(),
                priority: 10,
                pattern: RoutingPattern::KeywordAny {
                    keywords: vec!["review".to_owned()],
                    roles: vec!["reviewer".to_owned()],
                },
            },
        ],
        RouteFallback::Broadcast,
        Vec::new(),
    )
    .unwrap();
    let runtime = RoleRoutedRuntime::new(
        spec,
        bus,
        journal,
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_role_routing_table(table)
    .with_member_runner(reviewer, reviewer_runner.clone())
    .with_member_runner(implementer, implementer_runner.clone());

    runtime
        .dispatch_goal_classified(
            sender,
            "needs review",
            Recipient::Role("implementer".to_owned()),
        )
        .await
        .unwrap();

    assert_eq!(reviewer_runner.roles().await, vec!["reviewer".to_owned()]);
    assert!(implementer_runner.roles().await.is_empty());
}

#[tokio::test]
async fn role_routing_table_classifier_timeout_uses_fallback_without_failing_dispatch() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session_id = SessionId::new();
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id,
    };
    let sender = AgentId::new();
    let coordinator = AgentId::new();
    let reviewer = AgentId::new();
    let mut spec = TeamBuilder::new("classifier-timeout", Topology::RoleRouted)
        .member(sender, "sender", ContextVisibility::All)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .member(reviewer, "reviewer", ContextVisibility::All)
        .build();
    spec.topology_config.coordinator = Some(coordinator);
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store);
    let coordinator_runner = Arc::new(RecordingTeamMemberRunner::default());
    let reviewer_runner = Arc::new(RecordingTeamMemberRunner::default());
    let classifier = Arc::new(TestRoleClassifier {
        id: "slow".to_owned(),
        timeout: Duration::from_millis(10),
        delay: Duration::from_millis(100),
        result: Ok(ClassifierVerdict {
            roles: vec!["reviewer".to_owned()],
            confidence: 1.0,
        }),
    });
    let table = RoleRoutingTable::new(
        vec![RoleRoutingRule {
            rule_id: "classifier".to_owned(),
            priority: 1,
            pattern: RoutingPattern::Classifier {
                classifier_id: "slow".to_owned(),
            },
        }],
        RouteFallback::SendToCoordinator,
        vec![classifier],
    )
    .unwrap();
    let table_handle = table.clone();
    let runtime = RoleRoutedRuntime::new(
        spec,
        bus,
        journal,
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        Arc::new(InMemoryBlobStore::default()),
    )
    .with_role_routing_table(table)
    .with_member_runner(coordinator, coordinator_runner.clone())
    .with_member_runner(reviewer, reviewer_runner.clone());

    runtime
        .dispatch_goal_classified(
            sender,
            "classify this",
            Recipient::Role("reviewer".to_owned()),
        )
        .await
        .unwrap();

    assert_eq!(
        coordinator_runner.roles().await,
        vec!["coordinator".to_owned()]
    );
    assert!(reviewer_runner.roles().await.is_empty());
    assert_eq!(table_handle.classifier_timeout_total(), 1);
}

#[test]
fn role_routing_table_rejects_duplicate_classifier_ids() {
    let first = Arc::new(TestRoleClassifier::ready("dup", "reviewer"));
    let second = Arc::new(TestRoleClassifier::ready("dup", "implementer"));
    let error = RoleRoutingTable::new(Vec::new(), RouteFallback::Broadcast, vec![first, second])
        .expect_err("duplicate classifier ids must fail assembly");

    assert!(
        matches!(error, TeamError::Internal(message) if message.contains("duplicate role classifier id"))
    );
}

struct RecordingAuxProvider {
    result: Mutex<Result<String, ModelError>>,
    tasks: Mutex<Vec<AuxTask>>,
}

struct TestRoleClassifier {
    id: String,
    timeout: Duration,
    delay: Duration,
    result: Result<ClassifierVerdict, ClassifierError>,
}

impl TestRoleClassifier {
    fn ready(id: &str, role: &str) -> Self {
        Self {
            id: id.to_owned(),
            timeout: Duration::from_secs(1),
            delay: Duration::ZERO,
            result: Ok(ClassifierVerdict {
                roles: vec![role.to_owned()],
                confidence: 1.0,
            }),
        }
    }
}

#[async_trait]
impl RoleMessageClassifier for TestRoleClassifier {
    fn classifier_id(&self) -> &str {
        &self.id
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    async fn classify(
        &self,
        _message: &harness_team::AgentMessage,
        _team: &harness_team::TeamSpec,
    ) -> Result<ClassifierVerdict, ClassifierError> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.result.clone()
    }
}

impl RecordingAuxProvider {
    fn new(result: Result<String, ModelError>) -> Self {
        Self {
            result: Mutex::new(result),
            tasks: Mutex::new(Vec::new()),
        }
    }

    async fn tasks(&self) -> Vec<AuxTask> {
        self.tasks.lock().await.clone()
    }
}

#[async_trait]
impl AuxModelProvider for RecordingAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::new(DummyModelProvider)
    }

    fn aux_options(&self) -> AuxOptions {
        AuxOptions {
            fail_open: true,
            ..AuxOptions::default()
        }
    }

    async fn call_aux(&self, task: AuxTask, _req: ModelRequest) -> Result<String, ModelError> {
        self.tasks.lock().await.push(task);
        self.result.lock().await.clone()
    }
}

struct DummyModelProvider;

#[async_trait]
impl ModelProvider for DummyModelProvider {
    fn provider_id(&self) -> &str {
        "dummy"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "dummy".to_owned(),
            model_id: "dummy-aux".to_owned(),
            display_name: "Dummy Aux".to_owned(),
            context_window: 1_000,
            max_output_tokens: 100,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures::stream::empty()))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Default)]
struct RecordingTeamMemberRunner {
    roles: Mutex<Vec<String>>,
}

impl RecordingTeamMemberRunner {
    async fn roles(&self) -> Vec<String> {
        self.roles.lock().await.clone()
    }
}

#[async_trait]
impl TeamMemberRunner for RecordingTeamMemberRunner {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, TeamError> {
        self.roles.lock().await.push(request.role);
        Ok(TeamMemberRunOutcome {
            body: "ok".to_owned(),
            usage: UsageSnapshot::default(),
        })
    }
}
