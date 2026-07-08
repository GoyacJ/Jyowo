#![cfg(all(feature = "testing", feature = "agents-team"))]

mod runtime_assembly_support;
use runtime_assembly_support::*;

fn injected_agent_profile(id: &str, role: &str) -> harness_contracts::AgentProfile {
    harness_contracts::AgentProfile {
        id: id.to_owned(),
        scope: harness_contracts::AgentProfileScope::User,
        role: role.to_owned(),
        description: format!("{role} profile"),
        model_config_override: None,
        tool_allowlist: None,
        tool_blocklist: vec![],
        sandbox_inheritance: harness_contracts::AgentProfileSandboxInheritance::InheritParent,
        memory_scope: harness_contracts::AgentProfileMemoryScope::ReadOnly,
        context_mode: harness_contracts::AgentProfileContextMode::Focused,
        max_turns: 4,
        max_depth: 1,
        default_workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
    }
}

#[tokio::test]
async fn runtime_assembly_uses_session_injected_agent_profiles_for_run_scoped_team() {
    let workspace = unique_workspace("sdk-runtime-assembly-agent-team-injected-profiles");
    std::fs::create_dir_all(&workspace).unwrap();
    let model = Arc::new(CapabilityScriptedProvider::new(
        ConversationModelCapability::default(),
        vec![
            agent_team_tool_use_events("Run an injected profile team review", 2),
            vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("member accepted".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ],
            vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("parent accepted".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ],
        ],
    ));
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let event_store: Arc<dyn EventStore> = store.clone();
    let harness = Harness::builder()
        .with_model_arc(model)
        .with_store_arc(event_store)
        .with_sandbox(NoopSandbox::new())
        .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
        .build()
        .await
        .expect("harness should build");
    let session_id = SessionId::new();
    let options = SessionOptions::new(&workspace)
        .with_session_id(session_id)
        .with_permission_mode(PermissionMode::BypassPermissions)
        .with_interactivity(harness_contracts::InteractivityLevel::FullyInteractive)
        .with_agent_profiles(vec![
            injected_agent_profile("reviewer", "Injected Reviewer"),
            injected_agent_profile("worker", "Injected Worker"),
        ]);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("conversation session should open");

    harness
        .submit_conversation_turn(conversation_turn_request(
            options,
            ConversationTurnInput::ask("Run an injected profile team review"),
            Some(PermissionMode::BypassPermissions),
            None,
            Some(harness_contracts::AgentToolPolicy {
                subagents: harness_contracts::AgentUsePolicy::Allowed,
                agent_team: harness_contracts::AgentUsePolicy::Allowed,
                team_config: Some(harness_contracts::AgentTeamRunConfig {
                    topology: harness_contracts::AgentTeamTopology::CoordinatorWorker,
                    lead_profile_id: "reviewer".to_owned(),
                    member_profile_ids: vec!["worker".to_owned()],
                    max_turns_per_goal: 2,
                    shared_memory_policy:
                        harness_contracts::AgentTeamSharedMemoryPolicy::SummariesOnly,
                }),
                background_agents: harness_contracts::AgentUsePolicy::Off,
                workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
                max_depth: 2,
                max_concurrent_subagents: 2,
                max_team_members: 4,
            }),
        ))
        .await
        .expect("team turn should run");

    let member_roles = match tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let events = store
                .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
                .await
                .expect("events read")
                .collect::<Vec<_>>()
                .await;
            let member_roles = events
                .iter()
                .filter_map(|event| match event {
                    Event::TeamMemberJoined(joined) => Some(joined.role.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if !member_roles.is_empty() {
                break member_roles;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    {
        Ok(member_roles) => member_roles,
        Err(error) => {
            let events = store
                .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
                .await
                .expect("events read")
                .collect::<Vec<_>>()
                .await;
            panic!("team member should join: {error}; events: {events:#?}");
        }
    };
    assert_eq!(member_roles, vec!["Injected Reviewer", "Injected Worker"]);
}
