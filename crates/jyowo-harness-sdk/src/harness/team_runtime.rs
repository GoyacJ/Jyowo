use super::*;

#[cfg(feature = "agents-team")]
impl Harness {
    pub async fn create_team(
        &self,
        builder: harness_team::TeamBuilder,
    ) -> Result<crate::team::Team, HarnessError> {
        let spec = builder.build();
        spec.validate()
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        if spec.topology == harness_team::Topology::Custom {
            return Err(HarnessError::Other(
                "custom team topology is not executable through the SDK facade".to_owned(),
            ));
        }
        let tenant_id = self.inner.options.tenant_policy.id;
        let journal_session_id = harness_contracts::SessionId::new();
        let journal = harness_team::TeamJournalContext {
            tenant_id,
            session_id: journal_session_id,
        };
        let event_store = Arc::clone(&self.inner.event_store);
        let blob_store: Arc<dyn BlobStore> = self.inner.blob_store.as_ref().map_or_else(
            || Arc::new(harness_journal::InMemoryBlobStore::default()) as Arc<dyn BlobStore>,
            Arc::clone,
        );
        self.emit_team_created(&spec, journal, Arc::clone(&blob_store))
            .await?;
        let bus = harness_team::MessageBus::journaled(
            spec.team_id,
            spec.message_bus.buffer_size,
            journal,
            Arc::clone(&event_store),
        );
        let runtime = harness_team::Team::new(spec.clone(), bus, journal, event_store, blob_store);
        let execution = self
            .team_execution_runtime(runtime.clone(), &spec, tenant_id, journal_session_id)
            .await?;
        Ok(crate::team::Team::from_runtime(
            runtime,
            execution,
            spec,
            tenant_id,
            journal_session_id,
        ))
    }

    #[cfg(feature = "agents-team")]
    async fn team_execution_runtime(
        &self,
        runtime: harness_team::Team,
        spec: &harness_team::TeamSpec,
        tenant_id: TenantId,
        journal_session_id: harness_contracts::SessionId,
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

    #[cfg(feature = "agents-team")]
    async fn team_member_runner(
        &self,
        member: &harness_team::TeamMember,
        team_id: harness_contracts::TeamId,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<Arc<dyn harness_team::TeamMemberRunner>, HarnessError> {
        let mut options = SessionOptions::new(self.inner.options.workspace_root.clone())
            .with_tenant_id(tenant_id)
            .with_session_id(session_id)
            .with_team_id(team_id)
            .with_permission_mode(member.engine_config.permission_mode)
            .with_interactivity(member.engine_config.interactivity)
            .with_max_iterations(member.engine_config.max_iterations);
        if let Some(model_ref) = &member.engine_config.model_ref {
            options = options.with_model_id(model_ref.model_id.clone());
        }
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        self.enforce_tenant(&options)?;
        let prompt_inputs = self.load_effective_prompt_inputs(&options)?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let session_engine = self
            .engine_for_session(&options, &prompt_inputs, memory_manager, None)
            .await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let session_engine = self
            .engine_for_session(&options, &prompt_inputs, None)
            .await?;
        Ok(Arc::new(crate::agents_team::EngineTeamMemberRunner::new(
            Arc::new(session_engine.engine),
        )))
    }

    #[cfg(feature = "agents-team")]
    async fn emit_team_created(
        &self,
        spec: &harness_team::TeamSpec,
        journal: harness_team::TeamJournalContext,
        blob_store: Arc<dyn BlobStore>,
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
                    SessionOptions::new(PathBuf::from("."))
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
                AppendMetadata::default(),
                &events,
            )
            .await
            .map(|_| ())
            .map_err(HarnessError::Journal)
    }
}
