#![cfg(feature = "testing")]

mod runtime_assembly_support;
#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
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
#[derive(Default)]
struct CapturingAgentTeamStarter {
    request: Mutex<Option<harness_contracts::AgentTeamToolStartRequest>>,
}

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
impl harness_contracts::AgentTeamStarterCap for CapturingAgentTeamStarter {
    fn start_agent_team(
        &self,
        request: harness_contracts::AgentTeamToolStartRequest,
    ) -> futures::future::BoxFuture<
        'static,
        Result<harness_contracts::AgentTeamToolStartResponse, harness_contracts::ToolError>,
    > {
        *self.request.lock().expect("request lock") = Some(request.clone());
        Box::pin(async move {
            Ok(harness_contracts::AgentTeamToolStartResponse {
                team_id: TeamId::new(),
                conversation_id: request.conversation_id,
                parent_run_id: request.parent_run_id,
                status: "started".to_owned(),
            })
        })
    }
}

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
fn allowed_agent_policy() -> harness_contracts::AgentToolPolicy {
    harness_contracts::AgentToolPolicy {
        subagents: harness_contracts::AgentUsePolicy::Allowed,
        agent_team: harness_contracts::AgentUsePolicy::Allowed,
        team_config: Some(harness_contracts::AgentTeamRunConfig {
            topology: harness_contracts::AgentTeamTopology::CoordinatorWorker,
            lead_profile_id: "reviewer".to_owned(),
            member_profile_ids: vec!["worker".to_owned()],
            max_turns_per_goal: 3,
            shared_memory_policy: harness_contracts::AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        background_agents: harness_contracts::AgentUsePolicy::Off,
        workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 4,
        max_team_members: 4,
    }
}

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
#[test]
fn agent_team_tool_requires_allowed_policy_and_injected_starter() {
    block_on(async {
        for (name, install_starter, allow_team, expected) in [
            ("missing-starter", false, true, false),
            ("policy-off", true, false, false),
            ("enabled", true, true, true),
        ] {
            let workspace = unique_workspace(&format!("sdk-agent-team-{name}"));
            std::fs::create_dir_all(&workspace).unwrap();
            let model = Arc::new(TestModelProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("ready".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]));
            let mut builder = Harness::builder()
                .with_workspace_root(&workspace)
                .with_model_arc(model.clone())
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(NoopSandbox::new());
            if install_starter {
                builder = builder.with_capability::<dyn harness_contracts::AgentTeamStarterCap>(
                    ToolCapability::Custom("jyowo.agent_team.starter".to_owned()),
                    Arc::new(CapturingAgentTeamStarter::default()),
                );
            }
            let harness = builder.build().await.expect("harness should build");
            let options = SessionOptions::new(&workspace);
            harness
                .open_or_create_conversation_session(options.clone())
                .await
                .expect("conversation session should open");
            let mut policy = allowed_agent_policy();
            if !allow_team {
                policy.agent_team = harness_contracts::AgentUsePolicy::Off;
                policy.team_config = None;
            }
            harness
                .submit_conversation_turn(conversation_turn_request(
                    options,
                    ConversationTurnInput::ask("review"),
                    None,
                    None,
                    Some(policy),
                ))
                .await
                .expect("turn should run");

            let requests = model.requests().await;
            let tool_names: Vec<_> = requests[0]
                .tools
                .as_ref()
                .expect("run should expose tools")
                .iter()
                .map(|tool| tool.name.as_str())
                .collect();
            assert_eq!(tool_names.contains(&"agent_team"), expected, "{name}");
        }
    });
}

#[cfg(all(feature = "agents-subagent", feature = "agents-team"))]
#[test]
fn agent_team_tool_forwards_run_policy_and_session_snapshot() {
    block_on(async {
        let workspace = unique_workspace("sdk-agent-team-forwarding");
        std::fs::create_dir_all(&workspace).unwrap();
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::ToolUseComplete {
                            id: tool_use_id,
                            name: "agent_team".to_owned(),
                            input: json!({
                                "goal": "review daemon boundary",
                                "topology": "coordinator_worker",
                                "maxTurnsPerGoal": 3,
                            }),
                        },
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![ModelStreamEvent::MessageStop],
            ],
        ));
        let starter = Arc::new(CapturingAgentTeamStarter::default());
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_capability::<dyn harness_contracts::AgentTeamStarterCap>(
                ToolCapability::Custom("jyowo.agent_team.starter".to_owned()),
                starter.clone(),
            )
            .build()
            .await
            .expect("harness should build");
        let options = SessionOptions::new(&workspace)
            .with_permission_mode(PermissionMode::BypassPermissions)
            .with_interactivity(InteractivityLevel::FullyInteractive);
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("conversation session should open");
        let policy = allowed_agent_policy();
        let receipt = harness
            .submit_conversation_turn(conversation_turn_request(
                options.clone(),
                ConversationTurnInput::ask("review"),
                Some(PermissionMode::BypassPermissions),
                None,
                Some(policy.clone()),
            ))
            .await
            .expect("turn should run");

        let request = starter
            .request
            .lock()
            .expect("request lock")
            .clone()
            .expect("starter request");
        assert_eq!(request.parent_run_id, receipt.run_id);
        assert_eq!(request.tool_use_id, tool_use_id);
        assert_eq!(request.goal, "review daemon boundary");
        assert_eq!(request.agent_tool_policy, policy);
        assert_eq!(request.session.session_id, options.session_id);
        assert_eq!(
            request.session.permission_mode,
            PermissionMode::BypassPermissions
        );
        assert_eq!(
            request.session.interactivity,
            InteractivityLevel::FullyInteractive
        );
    });
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
            .with_workspace_root(&workspace)
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
