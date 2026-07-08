#[cfg(not(all(feature = "agents-team", feature = "testing")))]
#[test]
fn sdk_team_flow_default_build_documents_feature_gate() {
    assert!(!cfg!(all(feature = "agents-team", feature = "testing")));
}

#[cfg(all(feature = "agents-team", feature = "testing"))]
use std::sync::Arc;

#[cfg(all(feature = "agents-team", feature = "testing"))]
use futures::StreamExt;
#[cfg(all(feature = "agents-team", feature = "testing"))]
use harness_contracts::{AgentId, Event, Recipient, TeamTerminationReason, TenantId};
#[cfg(all(feature = "agents-team", feature = "testing"))]
use harness_journal::{EventStore, ReplayCursor};
#[cfg(all(feature = "agents-team", feature = "testing"))]
use harness_model::ModelProvider;
#[cfg(all(feature = "agents-team", feature = "testing"))]
use harness_team::{ContextVisibility, TeamMember, TeamMemberEngineConfig, Topology};
#[cfg(all(feature = "agents-team", feature = "testing"))]
use jyowo_harness_sdk::{prelude::*, testing::*};

#[cfg(all(feature = "agents-team", feature = "testing"))]
#[tokio::test]
async fn sdk_team_flow_dispatches_controls_members_and_terminates() {
    let workspace = unique_workspace("sdk-team-flow");
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(TestModelProvider::default());
    let model_provider: Arc<dyn ModelProvider> = model.clone();
    let owner = AgentId::new();
    let worker = AgentId::new();
    let late = AgentId::new();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model_provider)
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let team = harness
        .create_team(
            TeamBuilder::new("business-team", Topology::PeerToPeer)
                .member(owner, "owner", ContextVisibility::All)
                .member(worker, "worker", ContextVisibility::All),
        )
        .await
        .expect("team should be created through SDK");

    let message = team
        .dispatch(owner, Recipient::Broadcast, "ship the workflow")
        .await
        .expect("team should dispatch messages");
    assert_eq!(message.from, owner);

    let report = team
        .dispatch_goal_from(owner, "run the workflow")
        .await
        .expect("team should execute a goal through SDK member runners");
    assert_eq!(report.final_state["responses"], 1);
    assert!(report.members_usage.contains_key(&worker));
    assert_eq!(model.requests().await.len(), 1);

    team.pause();
    assert!(team.is_paused());
    assert!(team
        .dispatch(owner, Recipient::Broadcast, "blocked")
        .await
        .expect_err("paused team should reject dispatch")
        .to_string()
        .contains("paused"));
    team.resume();
    assert!(!team.is_paused());

    team.pause_member(worker).await;
    assert!(team.is_member_paused(worker).await);
    team.resume_member(worker).await;
    assert!(!team.is_member_paused(worker).await);

    team.add_member(TeamMember {
        agent_id: late,
        role: "late".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: TeamMemberEngineConfig::default(),
    })
    .await
    .expect("member should be added");
    assert!(team
        .members()
        .await
        .iter()
        .any(|member| member.agent_id == late));
    team.remove_member(late)
        .await
        .expect("member should be removed");
    assert!(!team
        .members()
        .await
        .iter()
        .any(|member| member.agent_id == late));

    team.terminate(TeamTerminationReason::Cancelled)
        .await
        .expect("team should terminate");
    assert!(team
        .dispatch(owner, Recipient::Broadcast, "after terminate")
        .await
        .expect_err("terminated team should reject dispatch")
        .to_string()
        .contains("terminated"));

    let events: Vec<_> = store
        .read(
            team.tenant_id(),
            team.journal_session_id(),
            ReplayCursor::FromStart,
        )
        .await
        .expect("team journal should be readable")
        .collect()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamCreated(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::AgentMessageSent(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamTurnCompleted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamMemberJoined(joined) if joined.agent_id == late)));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamMemberLeft(left) if left.agent_id == late)));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamTerminated(_))));

    let all_events = store
        .query_after(TenantId::SINGLE, None, 256)
        .await
        .expect("tenant event stream should be queryable");
    assert!(all_events
        .iter()
        .any(|event| matches!(event.payload, Event::RunStarted(_))));
    assert!(all_events
        .iter()
        .any(|event| matches!(event.payload, Event::RunEnded(_))));
}

#[cfg(all(feature = "agents-team", feature = "testing"))]
fn unique_workspace(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", SessionId::new()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}
