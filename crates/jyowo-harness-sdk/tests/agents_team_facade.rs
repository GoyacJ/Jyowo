#![cfg(all(feature = "agents-team", feature = "testing"))]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{AgentId, Event, Recipient, SessionId};
use harness_journal::{EventStore, ReplayCursor};
use harness_team::{ContextVisibility, TeamBuilder, TeamMemberEngineConfig, Topology};
use jyowo_harness_sdk::{testing, Harness};

#[tokio::test]
async fn sdk_create_team_exposes_runtime_facade_and_journals_lifecycle() {
    let store = Arc::new(testing::InMemoryEventStore::new(Arc::new(
        testing::NoopRedactor,
    )));
    let event_store: Arc<dyn EventStore> = store.clone();
    let owner = AgentId::new();
    let worker = AgentId::new();
    let late = AgentId::new();
    let workspace = unique_workspace("sdk-team-facade");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model(testing::TestModelProvider::default())
        .with_store_arc(event_store)
        .with_sandbox(testing::NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let team = harness
        .create_team(
            TeamBuilder::new("sdk-team", Topology::PeerToPeer)
                .member(owner, "owner", ContextVisibility::All)
                .member(worker, "worker", ContextVisibility::All),
        )
        .await
        .expect("sdk team should be created");

    let message = team
        .dispatch(owner, Recipient::Broadcast, "ship")
        .await
        .expect("dispatch should post through runtime team");
    assert_eq!(message.from, owner);

    team.pause();
    let paused = team
        .dispatch(owner, Recipient::Broadcast, "blocked")
        .await
        .expect_err("paused team should reject dispatch");
    assert!(paused.to_string().contains("paused"));
    team.resume();

    team.pause_member(worker).await;
    assert!(team.is_member_paused(worker).await);
    team.resume_member(worker).await;
    assert!(!team.is_member_paused(worker).await);

    team.add_member(harness_team::TeamMember {
        agent_id: late,
        role: "late".to_owned(),
        visibility: ContextVisibility::All,
        engine_config: TeamMemberEngineConfig::default(),
    })
    .await
    .expect("add member should journal join");
    team.remove_member(late)
        .await
        .expect("remove member should journal leave");
    team.remove_member(late)
        .await
        .expect("unknown member removal should be idempotent");

    team.terminate(harness_contracts::TeamTerminationReason::Cancelled)
        .await
        .expect("terminate should journal termination");
    let terminated = team
        .dispatch(owner, Recipient::Broadcast, "after")
        .await
        .expect_err("terminated team should reject dispatch");
    assert!(terminated.to_string().contains("terminated"));

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
    assert!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TeamMemberJoined(_)))
            .count()
            >= 3
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::AgentMessageSent(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamMemberLeft(left) if left.agent_id == late)));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::TeamTerminated(_))));
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}
