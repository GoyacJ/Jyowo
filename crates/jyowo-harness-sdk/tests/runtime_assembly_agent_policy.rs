#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
struct NoopBackgroundAgentStarter;

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
impl harness_contracts::BackgroundAgentStarterCap for NoopBackgroundAgentStarter {
    fn start_background_agent(
        &self,
        _request: harness_contracts::BackgroundAgentToolStartRequest,
    ) -> futures::future::BoxFuture<
        'static,
        Result<harness_contracts::BackgroundAgentToolStartResponse, harness_contracts::ToolError>,
    > {
        Box::pin(async {
            Err(harness_contracts::ToolError::Internal(
                "unexpected background starter execution".to_owned(),
            ))
        })
    }
}

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
#[test]
fn tenant_allowed_tools_filter_applies_to_runtime_agent_tools() {
    block_on(async {
        let workspace = unique_workspace("sdk-agent-runtime-tools-tenant-allowlist");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("ready".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));
        let allowed_tools = ["agent".to_owned()].into_iter().collect();

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_tenant_policy(TenantPolicy {
                allowed_tools: Some(allowed_tools),
                ..TenantPolicy::default()
            })
            .with_capability::<dyn harness_contracts::BackgroundAgentStarterCap>(
                ToolCapability::Custom("jyowo.background_agent.starter".to_owned()),
                Arc::new(NoopBackgroundAgentStarter),
            )
            .build()
            .await
            .expect("harness should build");

        let options = SessionOptions::new(&workspace);
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("conversation session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("review in parallel"),
                None,
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
                    background_agents: harness_contracts::AgentUsePolicy::Allowed,
                    workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
                    max_depth: 2,
                    max_concurrent_subagents: 2,
                    max_team_members: 4,
                }),
            ))
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("run should expose allowed tools")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert!(tool_names.contains(&"agent"));
        assert!(!tool_names.contains(&"agent_team"));
        assert!(!tool_names.contains(&"background_agent"));
    });
}
