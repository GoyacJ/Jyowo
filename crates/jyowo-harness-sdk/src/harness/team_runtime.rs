use super::*;

#[cfg(feature = "agents-team")]
pub struct RunScopedTeamStartupRequest {
    pub agent_tool_policy: harness_contracts::AgentToolPolicy,
    pub profiles: Vec<harness_contracts::AgentProfile>,
    pub run_id: RunId,
    pub conversation_session_id: SessionId,
    pub goal: String,
    pub workspace_root: PathBuf,
    pub workspace_bootstrap: Option<WorkspaceBootstrap>,
}

#[cfg(feature = "agents-team")]
struct SdkRunScopedTeamHost {
    harness: Harness,
    workspace_bootstrap: Option<WorkspaceBootstrap>,
}

#[cfg(feature = "agents-team")]
#[async_trait]
impl harness_agent_runtime::RunScopedTeamHost for SdkRunScopedTeamHost {
    type Team = Arc<crate::team::Team>;

    async fn create_run_scoped_team(
        &self,
        request: harness_agent_runtime::RunScopedTeamCreateRequest,
    ) -> Result<Self::Team, String> {
        self.harness
            .create_team_from_spec(
                request.spec,
                Some(request.conversation_session_id),
                Some(&request.agent_tool_policy),
                Some(request.workspace_root),
                self.workspace_bootstrap.clone(),
                Some(request.member_profile_ids),
                Some(request.run_id),
            )
            .await
            .map(Arc::new)
            .map_err(|error| error.to_string())
    }

    async fn register_active_run_team(
        &self,
        run_id: RunId,
        team: Self::Team,
    ) -> Result<(), String> {
        self.harness.register_active_run_team(run_id, team);
        Ok(())
    }

    async fn emit_team_task_updated(
        &self,
        session_id: SessionId,
        prepared: &harness_agent_runtime::PreparedRunScopedTeam,
        status: &str,
    ) -> Result<(), String> {
        self.harness
            .emit_team_task_updated(session_id, prepared, status)
            .await
            .map_err(|error| error.to_string())
    }

    async fn dispatch_run_scoped_team_goal(
        &self,
        run_id: RunId,
        team: Self::Team,
        prepared: harness_agent_runtime::PreparedRunScopedTeam,
        goal: String,
    ) -> Result<(), String> {
        self.harness
            .spawn_run_scoped_team_goal_dispatch(run_id, team, prepared, goal)
            .await;
        Ok(())
    }
}

#[cfg(all(test, feature = "agents-team"))]
mod tests {
    use super::*;

    fn profile(id: &str, role: &str) -> harness_contracts::AgentProfile {
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

    #[test]
    fn runtime_assembly_uses_agent_runtime_prepared_team_payload() {
        let options = harness_contracts::AgentToolPolicy {
            subagents: harness_contracts::AgentUsePolicy::Allowed,
            agent_team: harness_contracts::AgentUsePolicy::Allowed,
            team_config: Some(harness_contracts::AgentTeamRunConfig {
                topology: harness_contracts::AgentTeamTopology::CoordinatorWorker,
                lead_profile_id: "lead".to_owned(),
                member_profile_ids: vec!["worker".to_owned()],
                max_turns_per_goal: 2,
                shared_memory_policy: harness_contracts::AgentTeamSharedMemoryPolicy::SummariesOnly,
            }),
            background_agents: harness_contracts::AgentUsePolicy::Off,
            workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
            max_depth: 2,
            max_concurrent_subagents: 2,
            max_team_members: 2,
        };
        let request = RunScopedTeamStartupRequest {
            agent_tool_policy: options,
            profiles: vec![profile("lead", "lead"), profile("worker", "worker")],
            run_id: RunId::new(),
            conversation_session_id: SessionId::new(),
            goal: "inspect".to_owned(),
            workspace_root: PathBuf::from("."),
            workspace_bootstrap: None,
        };

        let prepared = harness_agent_runtime::prepare_run_scoped_team(
            &request.agent_tool_policy,
            &request.profiles,
            &request.run_id.to_string(),
            &request.conversation_session_id.to_string(),
            &request.goal,
        )
        .expect("prepared team");
        let spec = harness_agent_runtime::build_team_spec(&prepared);

        assert_eq!(spec.team_id, prepared.team_id);
        assert_eq!(spec.topology, harness_team::Topology::CoordinatorWorker);
        assert_eq!(
            spec.topology_config.coordinator,
            Some(prepared.lead.agent_id)
        );
        assert_eq!(
            spec.topology_config.workers,
            vec![prepared.members[0].agent_id]
        );
    }
}

#[cfg(feature = "agents-team")]
impl Harness {
    pub async fn create_team(
        &self,
        builder: harness_team::TeamBuilder,
    ) -> Result<crate::team::Team, HarnessError> {
        self.create_team_from_spec(builder.build(), None, None, None, None, None, None)
            .await
    }

    pub async fn create_team_from_spec(
        &self,
        spec: harness_team::TeamSpec,
        journal_session_id: Option<SessionId>,
        agent_tool_policy: Option<&harness_contracts::AgentToolPolicy>,
        workspace_root: Option<PathBuf>,
        workspace_bootstrap: Option<WorkspaceBootstrap>,
        member_profile_ids: Option<HashMap<AgentId, String>>,
        journal_run_id: Option<RunId>,
    ) -> Result<crate::team::Team, HarnessError> {
        spec.validate()
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        if spec.topology == harness_team::Topology::Custom {
            return Err(HarnessError::Other(
                "custom team topology is not executable through the SDK facade".to_owned(),
            ));
        }
        let tenant_id = self.inner.options.tenant_policy.id;
        let journal_session_id = journal_session_id.unwrap_or_else(SessionId::new);
        let journal = harness_team::TeamJournalContext {
            tenant_id,
            session_id: journal_session_id,
        };
        let event_store = Arc::clone(&self.inner.event_store);
        let blob_store: Arc<dyn BlobStore> = self.inner.blob_store.as_ref().map_or_else(
            || Arc::new(harness_journal::InMemoryBlobStore::default()) as Arc<dyn BlobStore>,
            Arc::clone,
        );
        let workspace_root =
            workspace_root.unwrap_or_else(|| self.inner.options.workspace_root.clone());
        self.emit_team_created(
            &spec,
            journal,
            Arc::clone(&blob_store),
            &workspace_root,
            journal_run_id,
        )
        .await?;
        let bus = harness_team::MessageBus::journaled(
            spec.team_id,
            spec.message_bus.buffer_size,
            journal,
            Arc::clone(&event_store),
        );
        let runtime = harness_team::Team::new_with_workspace_root(
            spec.clone(),
            bus,
            journal,
            event_store,
            blob_store,
            workspace_root.clone(),
        );
        let execution = self
            .team_execution_runtime(
                runtime.clone(),
                &spec,
                tenant_id,
                journal_session_id,
                agent_tool_policy,
                &workspace_root,
                workspace_bootstrap.as_ref(),
                member_profile_ids.as_ref(),
            )
            .await?;
        Ok(crate::team::Team::from_runtime(
            runtime,
            execution,
            spec,
            tenant_id,
            journal_session_id,
        ))
    }

    pub async fn start_run_scoped_team(
        &self,
        request: RunScopedTeamStartupRequest,
    ) -> Result<Arc<crate::team::Team>, HarnessError> {
        let store = harness_agent_runtime::open_team_runtime_store(&request.workspace_root)
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        let host = SdkRunScopedTeamHost {
            harness: self.clone(),
            workspace_bootstrap: request.workspace_bootstrap,
        };
        let team = harness_agent_runtime::RunScopedTeamCoordinator::new(&store)
            .start(
                &host,
                harness_agent_runtime::RunScopedTeamCoordinatorRequest {
                    agent_tool_policy: request.agent_tool_policy,
                    profiles: request.profiles,
                    run_id: request.run_id,
                    conversation_session_id: request.conversation_session_id,
                    goal: request.goal,
                    workspace_root: request.workspace_root,
                },
            )
            .await
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        Ok(team)
    }

    pub(super) fn register_active_run_team(&self, run_id: RunId, team: Arc<crate::team::Team>) {
        self.inner.active_run_teams.lock().insert(run_id, team);
    }

    pub(super) fn has_active_run_team(&self, run_id: RunId) -> bool {
        self.inner.active_run_teams.lock().contains_key(&run_id)
    }

    pub(super) async fn cancel_active_run_team(&self, run_id: RunId) -> Result<(), HarnessError> {
        let team = self.inner.active_run_teams.lock().remove(&run_id);
        if let Some(team) = team {
            team.terminate(TeamTerminationReason::Cancelled)
                .await
                .map_err(|error| HarnessError::Other(error.to_string()))?;
        }
        Ok(())
    }

    pub(super) async fn spawn_run_scoped_team_goal_dispatch(
        &self,
        run_id: RunId,
        team: Arc<crate::team::Team>,
        prepared: harness_agent_runtime::PreparedRunScopedTeam,
        goal: String,
    ) {
        let harness = self.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let result = match prepared.topology {
                    harness_contracts::AgentTeamTopology::CoordinatorWorker => {
                        team.dispatch_goal(goal).await
                    }
                    harness_contracts::AgentTeamTopology::PeerToPeer => {
                        team.dispatch_goal_from(prepared.lead.agent_id, goal).await
                    }
                    harness_contracts::AgentTeamTopology::RoleRouted => {
                        let recipient = prepared
                            .members
                            .first()
                            .map(|member| Recipient::Role(member.profile.role.clone()))
                            .unwrap_or(Recipient::Broadcast);
                        team.dispatch_goal_to(prepared.lead.agent_id, recipient, goal)
                            .await
                    }
                };
                if let Err(error) = result {
                    let _ = harness.cancel_active_run_team(run_id).await;
                    let _ = error;
                } else {
                    harness.inner.active_run_teams.lock().remove(&run_id);
                }
            });
        }
    }

    async fn emit_team_task_updated(
        &self,
        session_id: SessionId,
        prepared: &harness_agent_runtime::PreparedRunScopedTeam,
        status: &str,
    ) -> Result<(), HarnessError> {
        self.inner
            .event_store
            .append_with_metadata(
                self.inner.options.tenant_policy.id,
                session_id,
                AppendMetadata {
                    run_id: Some(RunId::parse(&prepared.run_id).map_err(|error| {
                        HarnessError::Other(format!("invalid run-scoped team id: {error}"))
                    })?),
                    ..AppendMetadata::default()
                },
                &[Event::TeamTaskUpdated(TeamTaskUpdatedEvent {
                    team_id: prepared.team_id,
                    task_id: prepared.initial_task_id.clone(),
                    title: prepared.goal.clone(),
                    status: status.to_owned(),
                    assignee_profile_id: Some(prepared.lead.profile_id.clone()),
                    at: chrono::Utc::now(),
                })],
            )
            .await
            .map(|_| ())
            .map_err(HarnessError::Journal)
    }

    async fn team_execution_runtime(
        &self,
        runtime: harness_team::Team,
        spec: &harness_team::TeamSpec,
        tenant_id: TenantId,
        journal_session_id: harness_contracts::SessionId,
        agent_tool_policy: Option<&harness_contracts::AgentToolPolicy>,
        workspace_root: &Path,
        workspace_bootstrap: Option<&WorkspaceBootstrap>,
        member_profile_ids: Option<&HashMap<AgentId, String>>,
    ) -> Result<crate::team::TeamExecutionRuntime, HarnessError> {
        match spec.topology {
            harness_team::Topology::CoordinatorWorker => {
                let mut execution = harness_team::CoordinatorWorkerRuntime::from_team(runtime);
                for member in &spec.members {
                    execution = execution.with_member_runner(
                        member.agent_id,
                        self.team_member_runner(
                            member,
                            spec.team_id,
                            tenant_id,
                            journal_session_id,
                            agent_tool_policy,
                            workspace_root,
                            workspace_bootstrap,
                            member_profile_ids
                                .and_then(|profiles| profiles.get(&member.agent_id))
                                .map(String::as_str),
                        )
                        .await?,
                    );
                }
                Ok(crate::team::TeamExecutionRuntime::CoordinatorWorker(
                    Arc::new(execution),
                ))
            }
            harness_team::Topology::PeerToPeer => {
                let mut execution = harness_team::PeerToPeerRuntime::from_team(runtime);
                for member in &spec.members {
                    execution = execution.with_member_runner(
                        member.agent_id,
                        self.team_member_runner(
                            member,
                            spec.team_id,
                            tenant_id,
                            journal_session_id,
                            agent_tool_policy,
                            workspace_root,
                            workspace_bootstrap,
                            member_profile_ids
                                .and_then(|profiles| profiles.get(&member.agent_id))
                                .map(String::as_str),
                        )
                        .await?,
                    );
                }
                Ok(crate::team::TeamExecutionRuntime::PeerToPeer(Arc::new(
                    execution,
                )))
            }
            harness_team::Topology::RoleRouted => {
                let mut execution = harness_team::RoleRoutedRuntime::from_team(runtime);
                for member in &spec.members {
                    execution = execution.with_member_runner(
                        member.agent_id,
                        self.team_member_runner(
                            member,
                            spec.team_id,
                            tenant_id,
                            journal_session_id,
                            agent_tool_policy,
                            workspace_root,
                            workspace_bootstrap,
                            member_profile_ids
                                .and_then(|profiles| profiles.get(&member.agent_id))
                                .map(String::as_str),
                        )
                        .await?,
                    );
                }
                Ok(crate::team::TeamExecutionRuntime::RoleRouted(Arc::new(
                    execution,
                )))
            }
            harness_team::Topology::Custom => Err(HarnessError::Other(
                "custom team topology is not executable through the SDK facade".to_owned(),
            )),
        }
    }

    async fn team_member_runner(
        &self,
        member: &harness_team::TeamMember,
        team_id: harness_contracts::TeamId,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        agent_tool_policy: Option<&harness_contracts::AgentToolPolicy>,
        workspace_root: &Path,
        workspace_bootstrap: Option<&WorkspaceBootstrap>,
        team_member_profile_id: Option<&str>,
    ) -> Result<Arc<dyn harness_team::TeamMemberRunner>, HarnessError> {
        let nested_subagent_options = agent_tool_policy.and_then(|options| {
            if harness_agent_runtime::should_install_subagent_runner(options)
                && options.agent_team == harness_contracts::AgentUsePolicy::Allowed
            {
                Some(options)
            } else {
                None
            }
        });
        let mut options = SessionOptions::new(workspace_root.to_path_buf())
            .with_tenant_id(tenant_id)
            .with_session_id(session_id)
            .with_team_id(team_id)
            .with_permission_mode(member.engine_config.permission_mode)
            .with_interactivity(member.engine_config.interactivity)
            .with_max_iterations(member.engine_config.max_iterations);
        options.workspace_bootstrap = workspace_bootstrap.cloned();
        if let Some(model_ref) = &member.engine_config.model_ref {
            options = options.with_model_id(model_ref.model_id.clone());
        }
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        let mut run_options = ConversationRunOptions::from_session_options(&options);
        #[cfg(feature = "agents-subagent")]
        {
            run_options.agent_tool_policy = nested_subagent_options.cloned();
        }
        #[cfg(feature = "agents-subagent")]
        let subagent_team_attribution = nested_subagent_options.and_then(|_| {
            team_member_profile_id.map(|profile_id| {
                harness_agent_runtime::SubagentTeamAttribution {
                    team_id,
                    team_member_profile_id: profile_id.to_owned(),
                }
            })
        });
        self.enforce_tenant(&options)?;
        let prompt_inputs = self.load_effective_prompt_inputs(&options)?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let session_engine = self
            .engine_for_session(
                &options,
                &run_options,
                &prompt_inputs,
                memory_manager,
                None,
                #[cfg(feature = "agents-subagent")]
                subagent_team_attribution.clone(),
            )
            .await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let session_engine = self
            .engine_for_session(
                &options,
                &run_options,
                &prompt_inputs,
                None,
                #[cfg(feature = "agents-subagent")]
                subagent_team_attribution,
            )
            .await?;
        Ok(Arc::new(crate::agents_team::EngineTeamMemberRunner::new(
            Arc::new(session_engine.engine),
        )))
    }

    async fn emit_team_created(
        &self,
        spec: &harness_team::TeamSpec,
        journal: harness_team::TeamJournalContext,
        blob_store: Arc<dyn BlobStore>,
        workspace_root: &Path,
        journal_run_id: Option<RunId>,
    ) -> Result<(), HarnessError> {
        let member_specs = serde_json::to_vec(&spec.members)
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        let member_specs_hash = *blake3::hash(&member_specs).as_bytes();
        let mut events = vec![Event::TeamCreated(TeamCreatedEvent {
            team_id: spec.team_id,
            tenant_id: journal.tenant_id,
            name: spec.name.clone(),
            topology_kind: redaction::topology_kind(spec.topology),
            member_specs_hash,
            created_at: chrono::Utc::now(),
        })];

        for member in &spec.members {
            let session_id = harness_contracts::SessionId::new();
            Session::builder()
                .with_options(
                    SessionOptions::new(workspace_root)
                        .with_tenant_id(journal.tenant_id)
                        .with_session_id(session_id),
                )
                .with_event_store(Arc::clone(&self.inner.event_store))
                .build()
                .await
                .map_err(HarnessError::Session)?;

            let member_bytes = serde_json::to_vec(member)
                .map_err(|error| HarnessError::Other(error.to_string()))?;
            let member_size = member_bytes.len() as u64;
            let spec_hash = *blake3::hash(&member_bytes).as_bytes();
            let spec_snapshot_id = blob_store
                .put(
                    journal.tenant_id,
                    Bytes::from(member_bytes),
                    BlobMeta {
                        content_type: Some("application/json".to_owned()),
                        size: member_size,
                        content_hash: spec_hash,
                        created_at: chrono::Utc::now(),
                        retention: BlobRetention::SessionScoped(session_id),
                    },
                )
                .await
                .map_err(|error| HarnessError::Other(error.to_string()))?;
            events.push(Event::TeamMemberJoined(TeamMemberJoinedEvent {
                team_id: spec.team_id,
                agent_id: member.agent_id,
                role: member.role.clone(),
                session_id,
                visibility: member.visibility.clone(),
                spec_snapshot_id,
                spec_hash,
                joined_at: chrono::Utc::now(),
            }));
        }

        self.inner
            .event_store
            .append_with_metadata(
                journal.tenant_id,
                journal.session_id,
                AppendMetadata {
                    run_id: journal_run_id,
                    ..AppendMetadata::default()
                },
                &events,
            )
            .await
            .map(|_| ())
            .map_err(HarnessError::Journal)
    }
}
