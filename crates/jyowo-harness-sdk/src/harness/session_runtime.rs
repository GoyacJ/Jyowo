use super::*;

pub(super) struct SdkSessionState {
    pub(super) projection: SessionProjection,
}

pub(super) struct SessionEngine {
    pub(super) engine: Engine,
    pub(super) runtime_prompt_context_hash: [u8; 32],
}

impl Harness {
    pub async fn create_session(&self, options: SessionOptions) -> Result<Session, HarnessError> {
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        self.enforce_tenant(&options)?;
        let limit_permit = self.inner.session_limits.try_acquire()?;
        #[cfg(feature = "memory-external-slot")]
        self.activate_plugins(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        let pending_session_events = Arc::new(PendingSessionEvents::default());
        let prompt_inputs = self.load_effective_prompt_inputs(&options)?;
        let prompt_inputs_hash = effective_prompt_inputs_hash(&prompt_inputs);
        #[cfg(feature = "memory-external-slot")]
        let session_engine = self
            .engine_for_session(
                &options,
                &prompt_inputs,
                memory_manager.clone(),
                Some(Arc::clone(&pending_session_events)),
                #[cfg(feature = "agents-subagent")]
                None,
                #[cfg(feature = "agents-subagent")]
                None,
            )
            .await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let session_engine = self
            .engine_for_session(
                &options,
                &prompt_inputs,
                Some(Arc::clone(&pending_session_events)),
                #[cfg(feature = "agents-subagent")]
                None,
                #[cfg(feature = "agents-subagent")]
                None,
            )
            .await?;
        let tenant_id = options.tenant_id;
        let session_id = options.session_id;
        let event_store: Arc<dyn EventStore> = Arc::new(LifecycleHookEventStore {
            inner: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            #[cfg(feature = "memory-external-slot")]
            user_id: options.user_id.clone(),
            #[cfg(feature = "memory-external-slot")]
            team_id: options.team_id,
            workspace_root: options.workspace_root.clone(),
            redactor: self.hook_redactor(),
            session_limits: Arc::clone(&self.inner.session_limits),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            summary_state: parking_lot::Mutex::new(MemorySessionSummaryState::default()),
            #[cfg(feature = "memory-external-slot")]
            memory_manager,
        });

        let session = Session::builder()
            .with_options(options)
            .with_effective_prompt_inputs_hash(prompt_inputs_hash)
            .with_runtime_prompt_context_hash(session_engine.runtime_prompt_context_hash)
            .with_event_store(event_store)
            .with_turn_runner(Arc::new(EngineSessionTurnRunner {
                engine: session_engine.engine,
                active_conversation_runs: Arc::clone(&self.inner.active_conversation_runs),
                process_registry: self.run_scoped_process_registry(),
                skill_registry: Some(self.inner.skill_registry.clone()),
                skill_metrics_sink: self.skill_metrics_sink(),
                skill_config_snapshot: self.inner.skill_config_snapshot.clone(),
            }))
            .with_skill_reload_cap(Arc::new(SdkSkillReloadCap {
                inner: Arc::clone(&self.inner),
            }))
            .build()
            .await
            .map_err(HarnessError::from)?;
        let pending_events = pending_session_events.drain();
        if !pending_events.is_empty() {
            self.inner
                .event_store
                .append(tenant_id, session_id, &pending_events)
                .await
                .map_err(HarnessError::Journal)?;
        }
        limit_permit.disarm();
        Ok(session)
    }

    pub(super) fn effective_sdk_session_options(
        &self,
        options: SessionOptions,
    ) -> Result<SessionOptions, HarnessError> {
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        self.enforce_tenant(&options)?;
        Ok(options)
    }

    pub(super) fn enforce_tenant(&self, options: &SessionOptions) -> Result<(), HarnessError> {
        if options.tenant_id != self.inner.options.tenant_policy.id
            && !self.inner.options.tenant_policy.allow_scoped_tenants
        {
            return Err(HarnessError::InvalidTenant(options.tenant_id));
        }
        Ok(())
    }

    pub(super) fn enforce_provider_allowed(&self, provider_id: &str) -> Result<(), HarnessError> {
        if let Some(allowed) = &self.inner.options.tenant_policy.allowed_providers {
            if !allowed.contains(provider_id) {
                return Err(HarnessError::PermissionDenied(format!(
                    "provider `{provider_id}` is not allowed by tenant policy"
                )));
            }
        }
        Ok(())
    }

    pub(super) async fn read_sdk_session_state(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<SdkSessionState>, HarnessError> {
        let envelopes = self
            .inner
            .event_store
            .read_envelopes(
                options.tenant_id,
                options.session_id,
                ReplayCursor::FromStart,
            )
            .await
            .map_err(HarnessError::Journal)?
            .collect::<Vec<_>>()
            .await;
        if envelopes.is_empty() {
            return Ok(None);
        }
        self.enforce_sdk_session_options_hash(options, &envelopes)?;
        let projection =
            SessionProjection::replay(envelopes.clone()).map_err(HarnessError::Session)?;
        Ok(Some(SdkSessionState { projection }))
    }

    pub(super) async fn is_conversation_session_stream(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<bool, HarnessError> {
        let envelopes = self
            .inner
            .event_store
            .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
            .await
            .map_err(HarnessError::Journal)?
            .take(1)
            .collect::<Vec<_>>()
            .await;
        let Some(envelope) = envelopes.first() else {
            return Ok(false);
        };
        let Event::SessionCreated(created) = &envelope.payload else {
            return Ok(false);
        };
        Ok(created.tenant_id == tenant_id && created.session_id == session_id)
    }

    #[cfg(feature = "sqlite-store")]
    pub(super) async fn is_conversation_session_stream_page(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<bool, HarnessError> {
        let page = self
            .inner
            .event_store
            .page_session_envelopes(tenant_id, session_id, None, 1)
            .await
            .map_err(HarnessError::Journal)?;
        let Some(envelope) = page.envelopes.first() else {
            return Ok(false);
        };
        let Event::SessionCreated(created) = &envelope.payload else {
            return Ok(false);
        };
        Ok(created.tenant_id == tenant_id && created.session_id == session_id)
    }

    pub(super) fn enforce_sdk_session_options_hash(
        &self,
        options: &SessionOptions,
        envelopes: &[EventEnvelope],
    ) -> Result<(), HarnessError> {
        let Some(Event::SessionCreated(created)) =
            envelopes.first().map(|envelope| &envelope.payload)
        else {
            return Err(HarnessError::Session(SessionError::Message(
                "session event stream does not start with SessionCreated".to_owned(),
            )));
        };
        let mut canonical = options.clone();
        canonical.workspace_root = canonical.workspace_root.canonicalize().map_err(|error| {
            HarnessError::Session(SessionError::Message(format!(
                "workspace_root invalid: {error}"
            )))
        })?;
        if !self.conversation_session_options_hash_matches(&canonical, created.options_hash) {
            return Err(HarnessError::PermissionDenied(
                "conversation session options do not match the existing session".to_owned(),
            ));
        }
        let session_created_options_hash = created.options_hash;
        for envelope in envelopes.iter().skip(1) {
            let Event::SessionCreated(created) = &envelope.payload else {
                continue;
            };
            if created.tenant_id != options.tenant_id
                || created.session_id != options.session_id
                || created.options_hash != session_created_options_hash
            {
                return Err(HarnessError::PermissionDenied(
                    "conversation session stream contains a mismatched SessionCreated event"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn matching_session_options_hash_variant(
        &self,
        options: &SessionOptions,
        actual: [u8; 32],
    ) -> Option<SessionOptions> {
        if session_options_hash(options) == actual
            || legacy_session_options_hash_with_permission_mode(options) == actual
        {
            return Some(options.clone());
        }

        if legacy_session_options_hash_without_runtime_context(options) == actual {
            return Some(options.clone());
        }

        None
    }

    fn conversation_session_options_hash_matches(
        &self,
        options: &SessionOptions,
        actual: [u8; 32],
    ) -> bool {
        if self
            .matching_session_options_hash_variant(options, actual)
            .is_some()
        {
            return true;
        }

        let mut model_ids = vec![None, options.model_id.clone()];
        let mut protocols = vec![
            None,
            options.protocol,
            Some(ModelProtocol::ChatCompletions),
            Some(ModelProtocol::Responses),
            Some(ModelProtocol::Messages),
            Some(ModelProtocol::GenerateContent),
        ];
        for descriptor in self.inner.model.supported_models() {
            model_ids.push(Some(descriptor.model_id));
            protocols.push(Some(descriptor.protocol));
        }
        model_ids.sort();
        model_ids.dedup();
        let mut deduped_protocols = Vec::new();
        for protocol in protocols {
            if !deduped_protocols.contains(&protocol) {
                deduped_protocols.push(protocol);
            }
        }

        for model_id in model_ids {
            for protocol in &deduped_protocols {
                let mut variant = options.clone();
                variant.model_id = model_id.clone();
                variant.protocol = *protocol;
                if session_options_hash(&variant) == actual
                    || legacy_session_options_hash_with_permission_mode(&variant) == actual
                    || legacy_session_options_hash_without_runtime_context(&variant) == actual
                {
                    return true;
                }
            }
        }

        false
    }

    pub(super) async fn resume_sdk_session_from_projection(
        &self,
        options: SessionOptions,
        projection: SessionProjection,
        #[cfg(feature = "agents-subagent")] agent_run_options: Option<
            &harness_contracts::AgentRunOptions,
        >,
    ) -> Result<Session, HarnessError> {
        let limit_permit = self.inner.session_limits.try_acquire()?;
        let prompt_inputs = self.load_effective_prompt_inputs(&options)?;
        let prompt_inputs_hash = effective_prompt_inputs_hash(&prompt_inputs);
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let session_engine = self
            .engine_for_session(
                &options,
                &prompt_inputs,
                memory_manager.clone(),
                None,
                #[cfg(feature = "agents-subagent")]
                agent_run_options,
                #[cfg(feature = "agents-subagent")]
                None,
            )
            .await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let session_engine = self
            .engine_for_session(
                &options,
                &prompt_inputs,
                None,
                #[cfg(feature = "agents-subagent")]
                agent_run_options,
                #[cfg(feature = "agents-subagent")]
                None,
            )
            .await?;
        let event_store: Arc<dyn EventStore> = Arc::new(LifecycleHookEventStore {
            inner: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            #[cfg(feature = "memory-external-slot")]
            user_id: options.user_id.clone(),
            #[cfg(feature = "memory-external-slot")]
            team_id: options.team_id,
            workspace_root: options.workspace_root.clone(),
            redactor: self.hook_redactor(),
            session_limits: Arc::clone(&self.inner.session_limits),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            summary_state: parking_lot::Mutex::new(MemorySessionSummaryState::default()),
            #[cfg(feature = "memory-external-slot")]
            memory_manager,
        });
        let session = Session::builder()
            .with_options(options)
            .with_effective_prompt_inputs_hash(prompt_inputs_hash)
            .with_runtime_prompt_context_hash(session_engine.runtime_prompt_context_hash)
            .with_event_store(event_store)
            .with_turn_runner(Arc::new(EngineSessionTurnRunner {
                engine: session_engine.engine,
                active_conversation_runs: Arc::clone(&self.inner.active_conversation_runs),
                process_registry: self.run_scoped_process_registry(),
                skill_registry: Some(self.inner.skill_registry.clone()),
                skill_metrics_sink: self.skill_metrics_sink(),
                skill_config_snapshot: self.inner.skill_config_snapshot.clone(),
            }))
            .with_skill_reload_cap(Arc::new(SdkSkillReloadCap {
                inner: Arc::clone(&self.inner),
            }))
            .with_projection(projection)
            .build()
            .await
            .map_err(HarnessError::from)?;
        limit_permit.disarm();
        Ok(session)
    }

    pub(super) fn effective_session_options(
        &self,
        explicit: SessionOptions,
    ) -> Result<SessionOptions, HarnessError> {
        let mut options = self.inner.options.default_session_options.clone();
        options.session_id = explicit.session_id;

        if let Some(workspace_id) = explicit.workspace_ref {
            let workspace = self
                .inner
                .workspace_registry
                .get(workspace_id)
                .ok_or_else(|| {
                    HarnessError::Other(format!("workspace not found: {workspace_id}"))
                })?;
            if explicit.tenant_id != TenantId::SINGLE && explicit.tenant_id != workspace.tenant_id {
                return Err(HarnessError::TenantMismatch);
            }
            options.workspace_ref = Some(workspace.id);
            options.tenant_id = workspace.tenant_id;
            options.workspace_root = workspace.root_path.clone();
            options.workspace_bootstrap = Some(workspace.bootstrap());
            if let Some(defaults) = &workspace.default_session_options {
                apply_non_default_session_options(&mut options, defaults);
            }
        }

        apply_explicit_session_options(&mut options, &explicit);
        Ok(options)
    }

    pub(super) fn load_effective_prompt_inputs(
        &self,
        options: &SessionOptions,
    ) -> Result<EffectiveSystemPromptInputs, HarnessError> {
        let Some(bootstrap) = options.workspace_bootstrap.clone() else {
            return Ok(EffectiveSystemPromptInputs::default());
        };
        let mut workspace_sections = Vec::new();
        for file in &bootstrap.files {
            let path = resolve_bootstrap_path(&bootstrap.workspace_root, &file.relative_path)?;
            match std::fs::read_to_string(&path) {
                Ok(content) if !content.trim().is_empty() => {
                    if let Some(section) = workspace_instruction_section(
                        &bootstrap_source_label(&file.relative_path),
                        &content,
                    ) {
                        workspace_sections.push(section);
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !file.required => {}
                Err(error) => {
                    return Err(HarnessError::Other(format!(
                        "load workspace bootstrap `{}` failed: {error}",
                        path.display()
                    )));
                }
            }
        }
        let workspace_addendum = bootstrap
            .system_prompt_addendum
            .filter(|addendum| !addendum.trim().is_empty());
        Ok(EffectiveSystemPromptInputs {
            workspace_sections,
            workspace_addendum,
            ..EffectiveSystemPromptInputs::default()
        })
    }

    #[cfg(feature = "memory-external-slot")]
    pub(super) async fn engine_for_session(
        &self,
        options: &SessionOptions,
        prompt_inputs: &EffectiveSystemPromptInputs,
        memory_manager: Option<Arc<harness_memory::MemoryManager>>,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
        #[cfg(feature = "agents-subagent")] agent_run_options: Option<
            &harness_contracts::AgentRunOptions,
        >,
        #[cfg(feature = "agents-subagent")] subagent_team_attribution: Option<
            harness_agent_runtime::SubagentTeamAttribution,
        >,
    ) -> Result<SessionEngine, HarnessError> {
        self.activate_plugins(options).await?;
        let context = self.context_engine(options, memory_manager).await?;
        self.engine_for_session_with_context(
            options,
            prompt_inputs,
            context,
            pending_session_events,
            #[cfg(feature = "agents-subagent")]
            agent_run_options,
            #[cfg(feature = "agents-subagent")]
            subagent_team_attribution,
        )
        .await
    }

    #[cfg(not(feature = "memory-external-slot"))]
    pub(super) async fn engine_for_session(
        &self,
        options: &SessionOptions,
        prompt_inputs: &EffectiveSystemPromptInputs,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
        #[cfg(feature = "agents-subagent")] agent_run_options: Option<
            &harness_contracts::AgentRunOptions,
        >,
        #[cfg(feature = "agents-subagent")] subagent_team_attribution: Option<
            harness_agent_runtime::SubagentTeamAttribution,
        >,
    ) -> Result<SessionEngine, HarnessError> {
        self.activate_plugins(options).await?;
        let context = self.context_engine(options).await?;
        self.engine_for_session_with_context(
            options,
            prompt_inputs,
            context,
            pending_session_events,
            #[cfg(feature = "agents-subagent")]
            agent_run_options,
            #[cfg(feature = "agents-subagent")]
            subagent_team_attribution,
        )
        .await
    }

    async fn engine_for_session_with_context(
        &self,
        options: &SessionOptions,
        prompt_inputs: &EffectiveSystemPromptInputs,
        context: ContextEngine,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
        #[cfg(feature = "agents-subagent")] agent_run_options: Option<
            &harness_contracts::AgentRunOptions,
        >,
        #[cfg(feature = "agents-subagent")] subagent_team_attribution: Option<
            harness_agent_runtime::SubagentTeamAttribution,
        >,
    ) -> Result<SessionEngine, HarnessError> {
        #[cfg(feature = "agents-subagent")]
        {
            let worktrees_dir = harness_agent_runtime::WorkspaceIsolationManager::worktrees_dir(
                &options.workspace_root,
            );
            std::fs::create_dir_all(worktrees_dir).map_err(|error| {
                HarnessError::Other(format!(
                    "failed to prepare agent worktrees directory: {error}"
                ))
            })?;
        }
        let mut cap_registry = (*self.inner.cap_registry).clone();
        if let Some(blob_store) = &self.inner.blob_store {
            cap_registry.install::<dyn harness_contracts::BlobReaderCap>(
                ToolCapability::BlobReader,
                Arc::new(BlobReaderCapAdapter::new(Arc::clone(blob_store))),
            );
            cap_registry.install::<dyn harness_contracts::BlobWriterCap>(
                ToolCapability::BlobWriter,
                Arc::new(BlobWriterCapAdapter::new(Arc::clone(blob_store))),
            );
            cap_registry.install::<dyn harness_contracts::OffloadedBlobAuthorizerCap>(
                ToolCapability::OffloadedBlobAuthorizer,
                Arc::new(EventStoreOffloadedBlobAuthorizer::new(Arc::clone(
                    &self.inner.event_store,
                ))),
            );
        }
        if let Some(skill_registry) = self
            .skill_registry_service(options, pending_session_events)
            .await?
        {
            cap_registry.install::<dyn harness_contracts::SkillRegistryCap>(
                ToolCapability::SkillRegistry,
                Arc::new(skill_registry),
            );
        }
        self.inject_mcp_tools().await?;
        let model_id = options
            .model_id
            .clone()
            .unwrap_or_else(|| self.inner.options.model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.inner.model.as_ref(), &model_id)?;
        let protocol = options.protocol.unwrap_or(model_snapshot.protocol);
        self.enforce_provider_allowed(&model_snapshot.provider_id)?;
        let context = context.clone_with_budget(context_budget_for_model(
            &model_snapshot,
            options.context_compression_trigger_ratio,
        ));
        if !cap_registry.contains(&ToolCapability::ContextPatchSink) {
            cap_registry.install::<dyn ContextPatchSinkCap>(
                ToolCapability::ContextPatchSink,
                Arc::new(context.clone()),
            );
        }
        let model_profile = ToolPoolModelProfile {
            provider: harness_contracts::ModelProvider(model_snapshot.provider_id.clone()),
            max_context_tokens: (model_snapshot.context_window > 0)
                .then_some(model_snapshot.context_window),
        };
        let tool_registry_snapshot = self.inner.tool_registry.snapshot();
        let mut tool_filter = filter_unavailable_tools(&tool_registry_snapshot, &cap_registry);
        filter_unrouted_service_tools(
            &mut tool_filter,
            &tool_registry_snapshot,
            &*self.inner.provider_capability_routes.read(),
        );
        apply_tenant_tool_filter(&mut tool_filter, &self.inner.options.tenant_policy);
        tool_filter.intersect_with(ToolPoolFilter::from_profile(&options.tool_profile));
        let schema_context = SchemaResolverContext {
            run_id: RunId::new(),
            session_id: options.session_id,
            tenant_id: options.tenant_id,
        };
        let tools = ToolPool::assemble(
            &tool_registry_snapshot,
            &tool_filter,
            &options.tool_search,
            &model_profile,
            &schema_context,
        )
        .await
        .map_err(HarnessError::Tool)?;
        #[cfg(feature = "tool-search")]
        let mut tools = tools;
        #[cfg(feature = "tool-search")]
        self.install_tool_search_runtime(options, &mut tools, &mut cap_registry, &model_snapshot);

        #[cfg(feature = "agents-subagent")]
        let harness_has_subagent_runner = self
            .inner
            .cap_registry
            .contains(&ToolCapability::SubagentRunner);
        #[cfg(feature = "agents-subagent")]
        let mut subagent_assembly = None;
        #[cfg(feature = "agents-subagent")]
        if let Some(run_options) = agent_run_options {
            if harness_agent_runtime::should_install_subagent_runner(run_options) {
                subagent_assembly = Some(super::tool_pool::install_subagent_runner_for_run(
                    &mut cap_registry,
                    run_options,
                    self.conversation_deletion_guarded_event_store(),
                    &options.workspace_root,
                    subagent_team_attribution.clone(),
                ));
            }
        }
        #[cfg(feature = "agents-subagent")]
        let subagent_tool_enabled = super::tool_pool::subagent_tool_should_be_enabled(
            harness_has_subagent_runner,
            agent_run_options,
        );
        #[cfg(not(feature = "agents-subagent"))]
        let subagent_tool_enabled = false;

        let runtime_context = build_runtime_prompt_context(
            options,
            &model_snapshot,
            &model_id,
            protocol,
            subagent_tool_enabled,
            #[cfg(feature = "memory-builtin")]
            self.inner.builtin_memory.is_some(),
            #[cfg(not(feature = "memory-builtin"))]
            false,
            true,
        );
        let runtime_prompt_context_hash = runtime_prompt_context_hash(&runtime_context);

        #[cfg(feature = "agents-subagent")]
        let enable_subagent_tool =
            subagent_tool_enabled && cap_registry.contains(&ToolCapability::SubagentRunner);

        let mut builder = Engine::builder()
            .with_event_store(self.conversation_deletion_guarded_event_store())
            .with_context(context)
            .with_hooks(HookDispatcher::new(self.inner.hook_registry.snapshot()))
            .with_model(Arc::clone(&self.inner.model))
            .with_tools(tools)
            .with_permission_broker(Arc::clone(&self.inner.permission_broker))
            .with_workspace_root(&options.workspace_root)
            .with_model_id(model_id)
            .with_model_snapshot(model_snapshot)
            .with_model_extra(options.model_extra.clone())
            .with_protocol(protocol)
            .with_system_prompt(
                self.session_system_prompt(options, runtime_context, prompt_inputs)
                    .await?,
            )
            .with_sandbox(Arc::clone(&self.inner.sandbox))
            .with_cap_registry(Arc::new(cap_registry));
        if options.max_iterations > 0 {
            builder = builder.with_max_iterations(options.max_iterations);
        }
        #[cfg(feature = "agents-subagent")]
        if enable_subagent_tool {
            builder = builder.with_subagent_tool();
        }
        if let Some(blob_store) = &self.inner.blob_store {
            builder = builder.with_blob_store(Arc::clone(blob_store));
        }
        if let Some(tracer) = &self.inner.tracer {
            builder = builder.with_tracer(Arc::clone(tracer));
        }
        if let Some(observer) = &self.inner.observer {
            builder = builder.with_observer(Arc::clone(observer));
        }
        builder = builder.with_model_middlewares(self.inner.model_middlewares.clone());
        let engine = builder.build().map_err(HarnessError::from)?;
        #[cfg(feature = "agents-subagent")]
        if let Some(assembly) = &subagent_assembly {
            assembly
                .engine_factory
                .bind_engine(engine.clone())
                .map_err(|()| {
                    HarnessError::Other(
                        "subagent engine factory already bound to parent engine".to_owned(),
                    )
                })?;
        }
        Ok(SessionEngine {
            engine,
            runtime_prompt_context_hash,
        })
    }

    async fn context_engine(
        &self,
        _options: &SessionOptions,
        #[cfg(feature = "memory-external-slot")] memory_manager: Option<
            Arc<harness_memory::MemoryManager>,
        >,
    ) -> Result<ContextEngine, HarnessError> {
        let mut builder =
            ContextEngine::builder().with_default_compaction(self.inner.blob_store.clone());
        if let Some(aux_model) = &self.inner.aux_model {
            builder = builder.with_aux_provider(Arc::clone(aux_model));
        }
        if let Some(metrics_sink) = self.model_metrics_sink() {
            builder = builder.with_model_metrics_sink(metrics_sink);
        }
        #[cfg(feature = "memory-external-slot")]
        if let Some(memory_manager) = memory_manager {
            builder = builder.with_memory_manager(memory_manager);
        }
        builder.build().map_err(HarnessError::Context)
    }

    async fn session_system_prompt(
        &self,
        options: &SessionOptions,
        runtime_context: RuntimePromptContext,
        prompt_inputs: &EffectiveSystemPromptInputs,
    ) -> Result<Option<String>, HarnessError> {
        let mut inputs = prompt_inputs.clone();
        inputs.session_addendum = options.system_prompt_addendum.clone();
        inputs.builtin_memory_inner = self.builtin_system_prompt(options).await?;
        let rendered = SystemPromptBuilder::new()
            .with_runtime_context(runtime_context)
            .push_inputs(inputs)
            .render();
        Ok(Some(rendered))
    }

    #[cfg(not(feature = "memory-builtin"))]
    async fn builtin_system_prompt(
        &self,
        _options: &SessionOptions,
    ) -> Result<Option<String>, HarnessError> {
        Ok(None)
    }
}

pub(super) fn sdk_session_not_found(session_id: SessionId) -> HarnessError {
    HarnessError::Session(SessionError::Message(format!(
        "session not found: {session_id}"
    )))
}

pub(super) fn snapshot_for_supported_model(
    model: &dyn ModelProvider,
    model_id: &str,
) -> Result<ModelRuntimeSnapshot, HarnessError> {
    let provider_id = model.provider_id().to_owned();
    let descriptor = model
        .supported_models()
        .into_iter()
        .find(|descriptor| descriptor.provider_id == provider_id && descriptor.model_id == model_id)
        .ok_or_else(|| {
            HarnessError::Engine(harness_contracts::EngineError::Message(format!(
                "unsupported model id for provider {provider_id}: {model_id}"
            )))
        })?;
    Ok(ModelRuntimeSnapshot {
        provider_id: descriptor.provider_id,
        model_id: descriptor.model_id,
        protocol: descriptor.protocol,
        context_window: descriptor.context_window,
        conversation_capability: descriptor.conversation_capability,
        lifecycle: descriptor.lifecycle,
        pricing: descriptor.pricing,
    })
}

fn context_budget_for_model(
    model_snapshot: &ModelRuntimeSnapshot,
    context_compression_trigger_ratio: f32,
) -> TokenBudget {
    let mut budget = TokenBudget::default();
    let context_window = u64::from(model_snapshot.context_window);
    budget.soft_budget_ratio = context_compression_trigger_ratio.clamp(0.5, 0.95);
    budget.hard_budget_ratio = 0.95;

    if context_window == 0 {
        return budget;
    }

    let declared_output_tokens =
        u64::from(model_snapshot.conversation_capability.max_output_tokens);
    let reserved_output_tokens = if declared_output_tokens > 0 {
        declared_output_tokens.min(context_window / 2)
    } else {
        4_096_u64.min(context_window / 4)
    };
    budget.max_tokens_per_turn = context_window.saturating_sub(reserved_output_tokens).max(1);
    budget
}

fn apply_non_default_session_options(options: &mut SessionOptions, defaults: &SessionOptions) {
    if defaults.workspace_root != PathBuf::from(".") {
        options.workspace_root = defaults.workspace_root.clone();
    }
    if defaults.workspace_bootstrap.is_some() {
        options.workspace_bootstrap = defaults.workspace_bootstrap.clone();
    }
    if defaults.tool_search != ToolSearchMode::default() {
        options.tool_search = defaults.tool_search.clone();
    }
    if defaults.model_id.is_some() {
        options.model_id = defaults.model_id.clone();
    }
    if defaults.protocol.is_some() {
        options.protocol = defaults.protocol;
    }
    if defaults.model_extra != Value::Null {
        options.model_extra = defaults.model_extra.clone();
    }
    if defaults.permission_mode != PermissionMode::Default {
        options.permission_mode = defaults.permission_mode;
    }
    if defaults.interactivity != InteractivityLevel::NoInteractive {
        options.interactivity = defaults.interactivity;
    }
    if defaults.user_id.is_some() {
        options.user_id = defaults.user_id.clone();
    }
    if defaults.team_id.is_some() {
        options.team_id = defaults.team_id;
    }
    if defaults.system_prompt_addendum.is_some() {
        options.system_prompt_addendum = defaults.system_prompt_addendum.clone();
    }
    if defaults.max_iterations > 0 {
        options.max_iterations = defaults.max_iterations;
    }
    if defaults.context_compression_trigger_ratio != 0.8 {
        options.context_compression_trigger_ratio = defaults.context_compression_trigger_ratio;
    }
}

fn apply_explicit_session_options(options: &mut SessionOptions, explicit: &SessionOptions) {
    if explicit.workspace_ref.is_some() {
        options.workspace_ref = explicit.workspace_ref;
    }
    if explicit.workspace_root != PathBuf::from(".") {
        options.workspace_root = explicit.workspace_root.clone();
    }
    if explicit.workspace_bootstrap.is_some() {
        options.workspace_bootstrap = explicit.workspace_bootstrap.clone();
    }
    if explicit.tenant_id != TenantId::SINGLE {
        options.tenant_id = explicit.tenant_id;
    }
    if explicit.tool_search != ToolSearchMode::default() {
        options.tool_search = explicit.tool_search.clone();
    }
    if explicit.model_id.is_some() {
        options.model_id = explicit.model_id.clone();
    }
    if explicit.protocol.is_some() {
        options.protocol = explicit.protocol;
    }
    if explicit.model_extra != Value::Null {
        options.model_extra = explicit.model_extra.clone();
    }
    if explicit.permission_mode != PermissionMode::Default {
        options.permission_mode = explicit.permission_mode;
    }
    if explicit.interactivity != InteractivityLevel::NoInteractive {
        options.interactivity = explicit.interactivity;
    }
    if explicit.user_id.is_some() {
        options.user_id = explicit.user_id.clone();
    }
    if explicit.team_id.is_some() {
        options.team_id = explicit.team_id;
    }
    if explicit.system_prompt_addendum.is_some() {
        options.system_prompt_addendum = explicit.system_prompt_addendum.clone();
    }
    if explicit.max_iterations > 0 {
        options.max_iterations = explicit.max_iterations;
    }
    if explicit.context_compression_trigger_ratio != 0.8 {
        options.context_compression_trigger_ratio = explicit.context_compression_trigger_ratio;
    }
    options.session_id = explicit.session_id;
}

fn bootstrap_source_label(relative_path: &Path) -> String {
    relative_path.to_string_lossy().replace('\\', "/")
}

fn resolve_bootstrap_path(root: &Path, relative_path: &Path) -> Result<PathBuf, HarnessError> {
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(HarnessError::PermissionDenied(format!(
            "workspace bootstrap path must stay inside workspace: {}",
            relative_path.display()
        )));
    }
    Ok(root.join(relative_path))
}

#[async_trait]
impl SessionTurnRunner for EngineSessionTurnRunner {
    async fn run_turn(
        &self,
        ctx: SessionTurnContext,
        prompt: String,
    ) -> Result<Vec<Event>, SessionError> {
        let input = TurnInput {
            message: Message {
                id: ctx.message_id,
                role: MessageRole::User,
                parts: vec![MessagePart::Text(prompt)],
                created_at: harness_contracts::now(),
            },
            metadata: conversation_turn_metadata(
                ctx.turn_index,
                ctx.client_message_id.clone(),
                ctx.attachments.clone(),
            ),
        };
        let cancellation = CancellationToken::new();
        let _active_run = ActiveConversationRunGuard::register(
            Arc::clone(&self.active_conversation_runs),
            ctx.tenant_id,
            ctx.session_id,
            ctx.run_id,
            cancellation.clone(),
            self.process_registry.clone(),
        );
        let run_ctx = RunContext::new(ctx.tenant_id, ctx.session_id, ctx.run_id)
            .with_cancellation(cancellation)
            .with_optional_user_id(ctx.user_id.clone())
            .with_optional_team_id(ctx.team_id)
            .with_permission_mode(ctx.permission_mode)
            .with_permission_actor_source(ctx.permission_actor_source.clone())
            .with_interactivity(ctx.interactivity)
            .with_config_snapshot(
                ctx.config_snapshot_id,
                ctx.effective_config_hash,
                ctx.started_from_scope_set,
            )
            .with_context_seed(ctx.context_seed.clone());
        let engine = self.engine_with_turn_skill_snapshot()?;
        #[cfg(feature = "steering-queue")]
        let mut engine = engine;
        if let Some(delta) = ctx.pending_deferred_tools_delta.clone() {
            engine
                .context_engine()
                .push_deferred_tools_delta(ctx.tenant_id, ctx.session_id, delta)
                .map_err(|error| SessionError::Message(error.to_string()))?;
        }
        #[cfg(feature = "steering-queue")]
        if let Some(merge) = ctx.steering_merge.clone() {
            engine = engine
                .into_builder()
                .with_steering_drain(Arc::new(PreDrainedSteeringDrain::new(merge)))
                .build()
                .map_err(|error| SessionError::Message(error.to_string()))?;
        }
        let stream = engine
            .run(
                SessionHandle {
                    tenant_id: ctx.tenant_id,
                    session_id: ctx.session_id,
                },
                input,
                run_ctx,
            )
            .await
            .map_err(|error| SessionError::Message(error.to_string()))?;
        let events = stream.collect().await;
        self.cleanup_run_processes(ctx.tenant_id, ctx.session_id, ctx.run_id)
            .await?;
        Ok(events)
    }

    async fn push_context_patch(&self, request: ContextPatchRequest) -> Result<(), SessionError> {
        self.engine
            .context_engine()
            .push_patch(request)
            .await
            .map_err(|error| SessionError::Message(error.to_string()))
    }
}

fn conversation_turn_metadata(
    turn_index: usize,
    client_message_id: Option<String>,
    attachments: Vec<ConversationAttachmentReference>,
) -> serde_json::Value {
    let mut metadata = json!({ "turn": turn_index });
    if let Some(client_message_id) = client_message_id.filter(|value| is_uuid_v4_like(value)) {
        metadata["clientMessageId"] = json!(client_message_id);
    }
    if !attachments.is_empty() {
        metadata["attachments"] = json!(attachments);
    }
    metadata
}

fn is_uuid_v4_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }

    for index in [8, 13, 18, 23] {
        if bytes[index] != b'-' {
            return false;
        }
    }
    if bytes[14] != b'4' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
        return false;
    }

    bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 8 | 13 | 18 | 23))
        .all(|(_, byte)| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod conversation_metadata_tests {
    use super::conversation_turn_metadata;

    #[test]
    fn conversation_turn_metadata_keeps_only_uuid_v4_client_message_ids() {
        let uuid_v4 = "00000000-0000-4000-8000-000000000001";
        let uuid_v1 = "00000000-0000-1000-8000-000000000001";

        assert_eq!(
            conversation_turn_metadata(1, Some(uuid_v4.to_owned()), Vec::new())["clientMessageId"],
            uuid_v4
        );
        assert!(
            conversation_turn_metadata(1, Some(uuid_v1.to_owned()), Vec::new())
                .get("clientMessageId")
                .is_none()
        );
    }
}

impl EngineSessionTurnRunner {
    async fn cleanup_run_processes(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
    ) -> Result<(), SessionError> {
        if let Some(registry) = &self.process_registry {
            registry
                .cleanup_run(tenant_id, session_id, run_id)
                .await
                .map_err(|error| SessionError::Message(error.to_string()))?;
        }
        Ok(())
    }

    fn engine_with_turn_skill_snapshot(&self) -> Result<Engine, SessionError> {
        let engine = self.engine.clone();
        let Some(registry) = &self.skill_registry else {
            return Ok(engine);
        };

        let snapshot = registry.snapshot();
        let mut cap_registry = engine.cap_registry().as_ref().clone();
        validate_required_skill_config(&snapshot, &self.skill_config_snapshot)
            .map_err(|error| SessionError::Message(error.to_string()))?;
        let mut renderer = SkillRenderer::new(Arc::new(
            SkillConfigSnapshotResolver::from_registry_snapshot(
                &snapshot,
                self.skill_config_snapshot.clone(),
            ),
        ));
        if let Some(metrics_sink) = &self.skill_metrics_sink {
            renderer = renderer.with_metrics_sink(Arc::clone(metrics_sink));
        }
        let mut service =
            SkillRegistryService::new(registry.clone(), renderer).with_snapshot(snapshot);
        if let Some(metrics_sink) = &self.skill_metrics_sink {
            service = service.with_metrics_sink(Arc::clone(metrics_sink));
        }
        cap_registry.install::<dyn harness_contracts::SkillRegistryCap>(
            ToolCapability::SkillRegistry,
            Arc::new(service),
        );

        engine
            .into_builder()
            .with_cap_registry(Arc::new(cap_registry))
            .build()
            .map_err(|error| SessionError::Message(error.to_string()))
    }
}

#[cfg(test)]
mod run_scoped_process_cleanup_tests {
    use super::*;
    use futures::future::BoxFuture;
    use harness_contracts::ToolError;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn active_run_guard_drop_cleans_run_scoped_processes() {
        let active = Arc::new(parking_lot::Mutex::new(HashMap::new()));
        let registry = Arc::new(RecordingProcessRegistry::default());
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let run_id = RunId::new();

        {
            let _guard = ActiveConversationRunGuard::register(
                Arc::clone(&active),
                tenant_id,
                session_id,
                run_id,
                CancellationToken::new(),
                Some(registry.clone()),
            );
            assert!(active.lock().contains_key(&run_id));
        }

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            registry.notify.notified(),
        )
        .await
        .unwrap();
        assert!(!active.lock().contains_key(&run_id));
        assert_eq!(
            *registry.cleaned.lock(),
            Some((tenant_id, session_id, run_id))
        );
    }

    #[derive(Default)]
    struct RecordingProcessRegistry {
        cleaned: parking_lot::Mutex<Option<(TenantId, SessionId, RunId)>>,
        notify: Notify,
    }

    impl RunScopedProcessRegistryCap for RecordingProcessRegistry {
        fn start_process(
            &self,
            _invocation: harness_contracts::ProcessStartInvocation,
            _redactor: Arc<dyn Redactor>,
        ) -> BoxFuture<'_, Result<harness_contracts::ProcessStartResult, ToolError>> {
            Box::pin(async { Err(ToolError::Message("not implemented".to_owned())) })
        }

        fn read_process(
            &self,
            _invocation: harness_contracts::ProcessReadInvocation,
            _redactor: Arc<dyn Redactor>,
        ) -> BoxFuture<'_, Result<harness_contracts::ProcessReadResult, ToolError>> {
            Box::pin(async { Err(ToolError::Message("not implemented".to_owned())) })
        }

        fn stop_process(
            &self,
            _invocation: harness_contracts::ProcessStopInvocation,
        ) -> BoxFuture<'_, Result<harness_contracts::ProcessStopResult, ToolError>> {
            Box::pin(async { Err(ToolError::Message("not implemented".to_owned())) })
        }

        fn cleanup_run(
            &self,
            tenant_id: TenantId,
            session_id: SessionId,
            run_id: RunId,
        ) -> BoxFuture<'_, Result<(), ToolError>> {
            Box::pin(async move {
                *self.cleaned.lock() = Some((tenant_id, session_id, run_id));
                self.notify.notify_waiters();
                Ok(())
            })
        }
    }
}

#[cfg(feature = "steering-queue")]
struct PreDrainedSteeringDrain {
    merge: parking_lot::Mutex<Option<harness_session::SynthesizedUserMessage>>,
}

#[cfg(feature = "steering-queue")]
impl PreDrainedSteeringDrain {
    fn new(merge: harness_session::SynthesizedUserMessage) -> Self {
        Self {
            merge: parking_lot::Mutex::new(Some(merge)),
        }
    }
}

#[cfg(feature = "steering-queue")]
#[async_trait]
impl SteeringDrain for PreDrainedSteeringDrain {
    async fn drain_and_merge(
        &self,
        _session: &SessionHandle,
        _run_id: RunId,
        _merged_into_message_id: MessageId,
    ) -> Result<Option<SteeringMerge>, harness_contracts::EngineError> {
        let merge = self.merge.lock().take();
        Ok(merge.map(|message| SteeringMerge {
            body: message.body,
            applied_event: message.applied_event,
            already_persisted: true,
        }))
    }
}
