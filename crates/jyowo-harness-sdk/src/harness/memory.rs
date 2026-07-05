use super::*;

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
use harness_contracts::{
    MemoryRedactionSummary, MemoryTakesEffect, MemoryToolRecordView, MemoryToolResponse,
    MemoryToolState, ToolError,
};
#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
use harness_tool::builtin::{
    memory_tool_runtime_capability, MemoryToolDraft, MemoryToolRuntimeAction, MemoryToolRuntimeCap,
    MemoryToolRuntimeRequest, MemoryToolVisibility,
};

impl Harness {
    #[cfg(feature = "memory-provider-registry")]
    pub(super) async fn memory_manager_for_session(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<Arc<harness_memory::MemoryManager>>, HarnessError> {
        let memory_db_path = memory_db_path(options);
        let settings_store = memory_settings_store_for_session(options)?;
        let global_settings = settings_store
            .get_global(options.tenant_id)
            .map_err(|error| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(error))
            })?;
        let recall_policy = harness_memory::RecallPolicy {
            max_records_per_turn: global_settings.max_recall_records_per_turn,
            max_chars_per_turn: global_settings.max_recall_chars_per_turn,
            ..harness_memory::RecallPolicy::default()
        };

        let mut manager = harness_memory::MemoryManager::new()
            .with_policy_engine(harness_memory::MemoryPolicyEngine::new(
                global_settings.clone(),
            ))
            .with_recall_policy(recall_policy)
            .with_durable_trace_collector(&memory_db_path.to_string_lossy())
            .map_err(HarnessError::Memory)?
            .with_event_sink(Arc::new(SdkMemoryEventSink {
                event_store: Arc::clone(&self.inner.event_store),
                tenant_id: options.tenant_id,
                session_id: options.session_id,
            }))
            .with_threat_scanner(Arc::new(harness_memory::MemoryThreatScanner::default()));
        if let Some(metrics_sink) = self.memory_metrics_sink() {
            manager = manager.with_metrics_sink(metrics_sink);
        }
        #[cfg(feature = "memory-consolidation")]
        if let Some(hook) = &self.inner.consolidation_hook {
            manager = manager.with_consolidation_hook(Arc::clone(hook));
        }
        let configured_providers = self.effective_memory_providers();
        if !configured_providers
            .iter()
            .any(|provider| provider.provider_id() == "local")
        {
            let local_provider = Arc::new(
                harness_memory::local::LocalMemoryProvider::open(
                    &memory_db_path.to_string_lossy(),
                    options.tenant_id,
                )
                .map_err(HarnessError::Memory)?,
            );
            manager
                .register_provider(local_provider)
                .map_err(HarnessError::Memory)?;
        }
        for provider in configured_providers {
            manager
                .register_provider(provider)
                .map_err(HarnessError::Memory)?;
        }
        manager
            .initialize_session(&harness_contracts::MemorySessionCtx {
                tenant_id: options.tenant_id,
                session_id: options.session_id,
                workspace_id: None,
                user_id: options.user_id.as_deref(),
                team_id: options.team_id,
            })
            .await
            .map_err(HarnessError::Memory)?;
        Ok(Some(Arc::new(manager)))
    }

    #[cfg(feature = "memory-builtin")]
    pub(super) async fn builtin_system_prompt(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<String>, HarnessError> {
        let Some(config) = &self.inner.builtin_memory else {
            return Ok(None);
        };
        let mut memory = config.for_session(options);
        if let Some(metrics_sink) = self.memory_metrics_sink() {
            memory = memory.with_metrics_sink(metrics_sink);
        }
        let snapshot = memory.read_all().await.map_err(HarnessError::Memory)?;
        let rendered =
            render_builtin_memory_system_prompt(&snapshot, options.tenant_id, options.session_id);
        if !rendered.overflows.is_empty() {
            let events = rendered
                .overflows
                .iter()
                .cloned()
                .map(Event::MemdirOverflow)
                .collect::<Vec<_>>();
            let _ = self
                .inner
                .event_store
                .append(options.tenant_id, options.session_id, &events)
                .await;
            if let Some(metrics_sink) = self.memory_metrics_sink() {
                for overflow in &rendered.overflows {
                    metrics_sink.record(harness_memory::MemoryMetric::MemdirOverflow {
                        file: overflow.file,
                        current_chars: overflow.current_chars,
                        threshold: overflow.threshold,
                    });
                }
            }
        }
        Ok(rendered.inner)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn list_memory_items(
        &self,
        options: SessionOptions,
    ) -> Result<Vec<harness_memory::MemorySummary>, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .list_for_actor(memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .get_for_actor(id, memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn update_memory_item_content(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
        content: impl Into<String>,
        action_plan_id: Option<harness_contracts::ActionPlanId>,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        let content = content.into();
        let (engine, thread) = memory_policy_for_session(&options)?;
        let actor = harness_contracts::MemoryActor::User {
            user_label: options.user_id.clone(),
        };
        let permission = manual_user_memory_permission(action_plan_id);
        let evidence =
            manual_memory_evidence(&options, action_plan_id, "memory-item-update", &content);
        let _ = engine;
        let policy = harness_memory::MemoryOperationPolicy {
            thread,
            actor,
            permission,
            evidence,
        };
        manager
            .update_content_for_actor_with_policy(
                id,
                memory_actor_from_options(&options),
                content,
                None,
                &policy,
            )
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn delete_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
        action_plan_id: Option<harness_contracts::ActionPlanId>,
    ) -> Result<(), HarnessError> {
        self.enforce_tenant(&options)?;
        let (_engine, thread) = memory_policy_for_session(&options)?;
        let actor = harness_contracts::MemoryActor::User {
            user_label: options.user_id.clone(),
        };
        let permission = manual_user_memory_permission(action_plan_id);
        let evidence = manual_memory_evidence(&options, action_plan_id, "memory-item-delete", "");
        let policy = harness_memory::MemoryOperationPolicy {
            thread,
            actor,
            permission,
            evidence,
        };
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .forget_for_actor_with_policy(id, memory_actor_from_options(&options), None, &policy)
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn prepare_memory_export(
        &self,
        options: SessionOptions,
    ) -> Result<harness_memory::MemoryExportPreparation, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .prepare_export_for_actor(memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn emit_memory_export_audit(
        &self,
        options: SessionOptions,
        event: harness_contracts::MemoryExportedEvent,
    ) -> Result<(), HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .emit_export_audit(event)
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn list_memory_candidates(
        &self,
        options: SessionOptions,
        request: harness_contracts::ListMemoryCandidatesRequest,
    ) -> Result<harness_contracts::ListMemoryCandidatesResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let inbox = memory_inbox_for_session(&options)?;
        let mut candidates = inbox
            .list(request.state)
            .map_err(harness_contracts::MemoryError::Message)?;
        if let Some(session_id) = request.session_id {
            candidates.retain(|candidate| candidate_belongs_to_session(candidate, session_id));
        }
        let limit = request.limit.max(1) as usize;
        let candidates = candidates
            .into_iter()
            .take(limit)
            .map(memory_candidate_list_item)
            .collect();
        Ok(harness_contracts::ListMemoryCandidatesResponse {
            candidates,
            next_cursor: None,
        })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetMemorySettingsRequest,
    ) -> Result<harness_contracts::GetMemorySettingsResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let store = memory_settings_store_for_session(&options)?;
        let settings = store.get_global(request.tenant_id).map_err(|error| {
            HarnessError::Memory(harness_contracts::MemoryError::Message(error))
        })?;
        Ok(harness_contracts::GetMemorySettingsResponse { settings })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn update_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::UpdateMemorySettingsRequest,
    ) -> Result<harness_contracts::UpdateMemorySettingsResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let store = memory_settings_store_for_session(&options)?;
        let settings = store
            .update_global(request.tenant_id, request.settings)
            .map_err(|error| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(error))
            })?;
        Ok(harness_contracts::UpdateMemorySettingsResponse { settings })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_thread_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetThreadMemorySettingsRequest,
    ) -> Result<harness_contracts::GetThreadMemorySettingsResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let store = memory_settings_store_for_session(&options)?;
        let settings = store
            .get_thread(request.tenant_id, request.session_id)
            .map_err(|error| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(error))
            })?;
        Ok(harness_contracts::GetThreadMemorySettingsResponse { settings })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn update_thread_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::UpdateThreadMemorySettingsRequest,
    ) -> Result<harness_contracts::UpdateThreadMemorySettingsResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let store = memory_settings_store_for_session(&options)?;
        let settings = store
            .update_thread(request.tenant_id, request.settings)
            .map_err(|error| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(error))
            })?;
        Ok(harness_contracts::UpdateThreadMemorySettingsResponse { settings })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn approve_memory_candidate(
        &self,
        options: SessionOptions,
        request: harness_contracts::ApproveMemoryCandidateRequest,
    ) -> Result<harness_contracts::ApproveMemoryCandidateResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let inbox = memory_inbox_for_session(&options)?;
        let candidate = inbox
            .list(None)
            .map_err(harness_contracts::MemoryError::Message)?
            .into_iter()
            .find(|candidate| candidate.id == request.candidate_id)
            .ok_or_else(|| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(format!(
                    "candidate not found: {}",
                    request.candidate_id
                )))
            })?;
        if !candidate_belongs_to_session(&candidate, options.session_id) {
            return Err(HarnessError::Memory(
                harness_contracts::MemoryError::Message(format!(
                    "candidate does not belong to session: {}",
                    request.candidate_id
                )),
            ));
        }
        if candidate.state != harness_contracts::MemoryCandidateState::Proposed {
            return Err(HarnessError::Memory(
                harness_contracts::MemoryError::Message(format!(
                    "candidate is not proposed: {}",
                    request.candidate_id
                )),
            ));
        }
        let (_engine, thread) = memory_policy_for_session(&options)?;
        let permission = manual_user_memory_permission(request.action_plan_id);
        let actor = harness_contracts::MemoryActor::User {
            user_label: options.user_id.clone(),
        };
        let policy = harness_memory::MemoryOperationPolicy {
            thread,
            actor,
            permission,
            evidence: candidate.evidence.clone(),
        };
        let manager = self.memory_manager_for_browser(&options).await?;
        let memory_id = manager
            .upsert_with_policy(
                memory_record_from_candidate(&candidate),
                candidate.evidence.run_id,
                &policy,
            )
            .await
            .map_err(HarnessError::Memory)?;
        let candidate = match inbox.promote(request.candidate_id) {
            Ok(candidate) => candidate,
            Err(error) => {
                let _ = manager
                    .forget_for_actor_with_policy(
                        memory_id,
                        memory_actor_from_options(&options),
                        candidate.evidence.run_id,
                        &policy,
                    )
                    .await;
                return Err(HarnessError::Memory(
                    harness_contracts::MemoryError::Message(error),
                ));
            }
        };
        Ok(harness_contracts::ApproveMemoryCandidateResponse {
            candidate,
            memory_id,
        })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn reject_memory_candidate(
        &self,
        options: SessionOptions,
        request: harness_contracts::RejectMemoryCandidateRequest,
    ) -> Result<harness_contracts::RejectMemoryCandidateResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let _ = request.reason;
        let inbox = memory_inbox_for_session(&options)?;
        let candidate = inbox
            .list(None)
            .map_err(harness_contracts::MemoryError::Message)?
            .into_iter()
            .find(|candidate| candidate.id == request.candidate_id)
            .ok_or_else(|| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(format!(
                    "candidate not found: {}",
                    request.candidate_id
                )))
            })?;
        if !candidate_belongs_to_session(&candidate, options.session_id) {
            return Err(HarnessError::Memory(
                harness_contracts::MemoryError::Message(format!(
                    "candidate does not belong to session: {}",
                    request.candidate_id
                )),
            ));
        }
        if candidate.state != harness_contracts::MemoryCandidateState::Proposed {
            return Err(HarnessError::Memory(
                harness_contracts::MemoryError::Message(format!(
                    "candidate is not proposed: {}",
                    request.candidate_id
                )),
            ));
        }
        let candidate = inbox
            .reject(request.candidate_id)
            .map_err(harness_contracts::MemoryError::Message)?;
        Ok(harness_contracts::RejectMemoryCandidateResponse { candidate })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn merge_memory_candidate(
        &self,
        options: SessionOptions,
        mut request: harness_contracts::MergeMemoryCandidateRequest,
    ) -> Result<harness_contracts::MergeMemoryCandidateResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        ensure_distinct_memory_candidates(&request.candidate_ids)?;
        let (_engine, thread) = memory_policy_for_session(&options)?;
        let permission = manual_user_memory_permission(request.action_plan_id);
        let actor = harness_contracts::MemoryActor::User {
            user_label: options.user_id.clone(),
        };
        let inbox = memory_inbox_for_session(&options)?;
        let candidates = inbox
            .list(None)
            .map_err(harness_contracts::MemoryError::Message)?;
        let mut selected_candidates = Vec::with_capacity(request.candidate_ids.len());
        for candidate_id in &request.candidate_ids {
            let Some(candidate) = candidates
                .iter()
                .find(|candidate| candidate.id == *candidate_id)
            else {
                return Err(HarnessError::Memory(
                    harness_contracts::MemoryError::Message(format!(
                        "candidate not found: {candidate_id}"
                    )),
                ));
            };
            if !candidate_belongs_to_session(candidate, options.session_id) {
                return Err(HarnessError::Memory(
                    harness_contracts::MemoryError::Message(format!(
                        "candidate does not belong to session: {candidate_id}"
                    )),
                ));
            }
            if candidate.state != harness_contracts::MemoryCandidateState::Proposed {
                return Err(HarnessError::Memory(
                    harness_contracts::MemoryError::Message(format!(
                        "candidate is not proposed: {candidate_id}"
                    )),
                ));
            }
            selected_candidates.push(candidate.clone());
        }
        let evidence =
            merged_candidate_evidence(&selected_candidates, &request.merged_record.content);
        let policy = harness_memory::MemoryOperationPolicy {
            thread,
            actor,
            permission,
            evidence: evidence.clone(),
        };

        let manager = self.memory_manager_for_browser(&options).await?;
        let now = chrono::Utc::now();
        let record = harness_memory::MemoryRecord {
            id: harness_contracts::MemoryId::new(),
            tenant_id: request.tenant_id,
            kind: request.merged_record.kind.clone(),
            visibility: request.merged_record.visibility.clone(),
            content: request.merged_record.content.clone(),
            metadata: harness_memory::MemoryMetadata {
                tags: std::mem::take(&mut request.merged_record.metadata.tags),
                source: evidence.source.clone(),
                evidence: Some(evidence.clone()),
                confidence: request.merged_record.metadata.source_trust.clamp(0.0, 1.0) as f32,
                access_count: 0,
                last_accessed_at: None,
                recall_score: 0.0,
                ttl: request.merged_record.metadata.ttl,
                redacted_segments: 0,
            },
            created_at: now,
            updated_at: now,
        };
        let memory_id = manager
            .upsert_with_policy(record, evidence.run_id, &policy)
            .await
            .map_err(HarnessError::Memory)?;
        for candidate_id in &request.candidate_ids {
            if let Err(error) = inbox.merge(*candidate_id) {
                let _ = manager
                    .forget_for_actor_with_policy(
                        memory_id,
                        memory_actor_from_options(&options),
                        request.evidence.run_id,
                        &policy,
                    )
                    .await;
                return Err(HarnessError::Memory(
                    harness_contracts::MemoryError::Message(error),
                ));
            }
        }
        Ok(harness_contracts::MergeMemoryCandidateResponse {
            candidate_ids: request.candidate_ids,
            memory_id,
        })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn list_memory_recall_traces(
        &self,
        options: SessionOptions,
        request: harness_contracts::ListMemoryRecallTracesRequest,
    ) -> Result<harness_contracts::ListMemoryRecallTracesResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let collector = memory_trace_collector_for_session(&options)?;
        let traces = collector
            .list_summaries(request.tenant_id, request.session_id, request.run_id)
            .into_iter()
            .take(request.limit.max(1) as usize)
            .collect();
        Ok(harness_contracts::ListMemoryRecallTracesResponse {
            traces,
            next_cursor: None,
        })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_memory_recall_trace(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetMemoryRecallTraceRequest,
    ) -> Result<harness_contracts::GetMemoryRecallTraceResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        let collector = memory_trace_collector_for_session(&options)?;
        let trace = collector
            .get(request.tenant_id, request.trace_id)
            .ok_or_else(|| {
                HarnessError::Memory(harness_contracts::MemoryError::Message(format!(
                    "memory recall trace not found: {}",
                    request.trace_id
                )))
            })?;
        Ok(harness_contracts::GetMemoryRecallTraceResponse { trace })
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_model_request_preview(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetModelRequestPreviewRequest,
    ) -> Result<harness_contracts::GetModelRequestPreviewResponse, HarnessError> {
        self.enforce_tenant(&options)?;
        enforce_memory_tenant(&options, request.tenant_id)?;
        if request.session_id != options.session_id {
            return Err(HarnessError::Memory(
                harness_contracts::MemoryError::Message(
                    "preview session does not match runtime session".to_owned(),
                ),
            ));
        }

        let Some(trace_id) = request.trace_id else {
            return Ok(super::memory_preview::build_preview_response(
                request.session_id,
                request.run_id,
                super::memory_preview::ModelRequestPreviewBuilder::new(),
            ));
        };

        let collector = memory_trace_collector_for_session(&options)?;
        let Some(trace) = collector.get(request.tenant_id, trace_id) else {
            return Ok(super::memory_preview::build_preview_response(
                request.session_id,
                request.run_id,
                super::memory_preview::ModelRequestPreviewBuilder::new(),
            ));
        };
        let mut builder = super::memory_preview::ModelRequestPreviewBuilder::new();
        for injected in trace.injected {
            builder = builder.add_section(
                harness_contracts::MemorySource::ExternalRetrieval,
                Some(injected.provider_id),
                vec![injected.memory_id],
                format!(
                    "[redacted memory context: hash={:?}, chars={}]",
                    injected.content_hash, injected.injected_chars
                ),
            );
        }
        Ok(super::memory_preview::build_preview_response(
            request.session_id,
            request.run_id,
            builder,
        ))
    }

    #[cfg(feature = "memory-provider-registry")]
    async fn memory_manager_for_browser(
        &self,
        options: &SessionOptions,
    ) -> Result<Arc<harness_memory::MemoryManager>, HarnessError> {
        self.memory_manager_for_session(options)
            .await?
            .ok_or_else(|| {
                HarnessError::Memory(harness_contracts::MemoryError::ExternalProviderNotConfigured)
            })
    }

    #[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
    pub(super) fn install_memory_tool_runtime_for_session(
        &self,
        options: &SessionOptions,
        cap_registry: &mut CapabilityRegistry,
    ) {
        cap_registry.install::<dyn MemoryToolRuntimeCap>(
            memory_tool_runtime_capability(),
            Arc::new(SdkMemoryToolRuntime {
                harness: self.clone(),
                options: options.clone(),
            }),
        );
    }

    pub(super) fn effective_memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.inner
            .memory_providers
            .last()
            .map(Arc::clone)
            .or_else(|| {
                self.inner
                    .plugin_registry
                    .as_ref()
                    .and_then(|registry| registry.registered_memory_providers().into_iter().next())
            })
    }

    #[cfg(feature = "memory-provider-registry")]
    fn effective_memory_providers(&self) -> Vec<Arc<dyn MemoryProvider>> {
        let mut providers = self.inner.memory_providers.clone();
        for provider in self
            .inner
            .plugin_registry
            .as_ref()
            .map(harness_plugin::PluginRegistry::registered_memory_providers)
            .unwrap_or_default()
        {
            if !providers
                .iter()
                .any(|existing| existing.provider_id() == provider.provider_id())
            {
                providers.push(provider);
            }
        }
        providers
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
struct SdkMemoryToolRuntime {
    harness: Harness,
    options: SessionOptions,
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
#[async_trait]
impl MemoryToolRuntimeCap for SdkMemoryToolRuntime {
    async fn execute(
        &self,
        request: MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let mut options = self.options.clone();
        options.tenant_id = request.tenant_id;
        options.session_id = request.session_id;
        options.workspace_root = request.workspace_root.clone();
        self.harness
            .enforce_tenant(&options)
            .map_err(memory_tool_error)?;

        match &request.action {
            MemoryToolRuntimeAction::Search { .. } => self.execute_search(&options, &request).await,
            MemoryToolRuntimeAction::Read { .. } => self.execute_read(&options, &request).await,
            MemoryToolRuntimeAction::Create { .. } => self.execute_create(&options, &request).await,
            MemoryToolRuntimeAction::Update { .. } => self.execute_update(&options, &request).await,
            MemoryToolRuntimeAction::Delete { .. } => self.execute_delete(&options, &request).await,
            MemoryToolRuntimeAction::List { .. } => self.execute_list(&options, &request).await,
            MemoryToolRuntimeAction::Propose { .. } => {
                self.execute_propose(&options, &request).await
            }
        }
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
impl SdkMemoryToolRuntime {
    async fn execute_search(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::Search {
            query,
            max_records,
            visibility,
        } = &request.action
        else {
            return Err(ToolError::Validation("expected search action".to_owned()));
        };
        let manager = self.memory_manager(options).await?;
        let max_records = (*max_records).clamp(1, 50);
        let (engine, thread) = memory_policy_for_session(options).map_err(memory_tool_error)?;
        let _ = engine;
        let sources = manager
            .recall_with_policy_sources(
                harness_memory::MemoryQuery {
                    text: query.clone(),
                    kind_filter: None,
                    visibility_filter: memory_visibility_filter(options, visibility.as_ref())?,
                    max_records,
                    min_similarity: 0.0,
                    tenant_id: request.tenant_id,
                    session_id: Some(request.session_id),
                    deadline: None,
                },
                &thread,
                &harness_contracts::MemoryActor::Model,
            )
            .await
            .map_err(memory_error)?;
        let memory_ids = sources
            .iter()
            .map(|source| source.record.id)
            .collect::<Vec<_>>();
        let record_views = sources
            .iter()
            .map(|source| memory_tool_record_view(&source.record, &source.provider_id))
            .collect::<Vec<_>>();
        Ok(memory_tool_response(
            "search",
            MemoryToolState::Completed,
            memory_ids,
            Vec::new(),
            record_views,
            request.permission_context.action_plan_id,
            MemoryTakesEffect::CurrentTurn,
        ))
    }

    async fn execute_read(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::Read { memory_id } = &request.action else {
            return Err(ToolError::Validation("expected read action".to_owned()));
        };
        let manager = self.memory_manager(options).await?;
        let source = manager
            .get_for_actor_with_provider(*memory_id, memory_actor_from_options(options))
            .await
            .map_err(memory_error)?;
        Ok(memory_tool_response(
            "read",
            MemoryToolState::Completed,
            vec![*memory_id],
            Vec::new(),
            vec![memory_tool_record_view(&source.record, &source.provider_id)],
            request.permission_context.action_plan_id,
            MemoryTakesEffect::CurrentTurn,
        ))
    }

    async fn execute_create(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::Create { draft } = &request.action else {
            return Err(ToolError::Validation("expected create action".to_owned()));
        };
        let manager = self.memory_manager(options).await?;
        let draft = memory_draft_from_tool(options, draft)?;
        let evidence = memory_evidence_from_tool(request, &draft.content);
        let policy = self
            .memory_operation_policy(
                options,
                harness_contracts::MemoryActor::Model,
                request.permission_context.clone(),
                evidence.clone(),
            )
            .await?;
        let memory_id = manager
            .upsert_with_policy_and_provider_selection(
                memory_record_from_tool_draft(request.tenant_id, &draft),
                Some(request.run_id),
                &policy,
                &request.provider_policy,
            )
            .await
            .map_err(memory_error)?;
        Ok(memory_tool_response(
            "create",
            MemoryToolState::Completed,
            vec![memory_id],
            Vec::new(),
            Vec::new(),
            request.permission_context.action_plan_id,
            MemoryTakesEffect::NextTurn,
        ))
    }

    async fn execute_update(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::Update { memory_id, draft } = &request.action else {
            return Err(ToolError::Validation("expected update action".to_owned()));
        };
        let manager = self.memory_manager(options).await?;
        let draft = memory_draft_from_tool(options, draft)?;
        let evidence = memory_evidence_from_tool(request, &draft.content);
        let policy = self
            .memory_operation_policy(
                options,
                harness_contracts::MemoryActor::Model,
                request.permission_context.clone(),
                evidence,
            )
            .await?;
        let record = manager
            .update_content_for_actor_with_policy(
                *memory_id,
                memory_actor_from_options(options),
                draft.content,
                Some(request.run_id),
                &policy,
            )
            .await
            .map_err(memory_error)?;
        let source = manager
            .get_for_actor_with_provider(record.id, memory_actor_from_options(options))
            .await
            .map_err(memory_error)?;
        Ok(memory_tool_response(
            "update",
            MemoryToolState::Completed,
            vec![*memory_id],
            Vec::new(),
            vec![memory_tool_record_view(&source.record, &source.provider_id)],
            request.permission_context.action_plan_id,
            MemoryTakesEffect::NextTurn,
        ))
    }

    async fn execute_delete(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::Delete { memory_id, reason } = &request.action else {
            return Err(ToolError::Validation("expected delete action".to_owned()));
        };
        let manager = self.memory_manager(options).await?;
        let policy = self
            .memory_operation_policy(
                options,
                harness_contracts::MemoryActor::Model,
                request.permission_context.clone(),
                memory_evidence_from_tool(request, ""),
            )
            .await?;
        manager
            .forget_for_actor_with_policy(
                *memory_id,
                memory_actor_from_options(options),
                Some(request.run_id),
                &policy,
            )
            .await
            .map_err(memory_error)?;
        let _ = reason;
        Ok(memory_tool_response(
            "delete",
            MemoryToolState::Completed,
            vec![*memory_id],
            Vec::new(),
            Vec::new(),
            request.permission_context.action_plan_id,
            MemoryTakesEffect::NextTurn,
        ))
    }

    async fn execute_list(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::List { limit, .. } = &request.action else {
            return Err(ToolError::Validation("expected list action".to_owned()));
        };
        let manager = self.memory_manager(options).await?;
        let limit = (*limit).clamp(1, 100) as usize;
        let actor = memory_actor_from_options(options);
        let mut sources = manager
            .list_for_actor_sources(actor)
            .await
            .map_err(memory_error)?;
        sources.truncate(limit);
        let memory_ids = sources
            .iter()
            .map(|source| source.record.id)
            .collect::<Vec<_>>();
        let record_views = sources
            .iter()
            .map(|source| memory_tool_record_view(&source.record, &source.provider_id))
            .collect::<Vec<_>>();
        Ok(memory_tool_response(
            "list",
            MemoryToolState::Completed,
            memory_ids,
            Vec::new(),
            record_views,
            request.permission_context.action_plan_id,
            MemoryTakesEffect::CurrentTurn,
        ))
    }

    async fn execute_propose(
        &self,
        options: &SessionOptions,
        request: &MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        let MemoryToolRuntimeAction::Propose { draft } = &request.action else {
            return Err(ToolError::Validation("expected propose action".to_owned()));
        };
        let inbox = memory_inbox_for_session(options).map_err(memory_tool_error)?;
        let draft = memory_draft_from_tool(options, draft)?;
        let candidate = inbox
            .propose(
                draft.clone(),
                memory_evidence_from_tool(request, &draft.content),
            )
            .map_err(|error| ToolError::Internal(error.to_string()))?;
        Ok(memory_tool_response(
            "propose",
            MemoryToolState::CandidateCreated,
            Vec::new(),
            vec![candidate.id],
            Vec::new(),
            request.permission_context.action_plan_id,
            MemoryTakesEffect::Never,
        ))
    }

    async fn memory_manager(
        &self,
        options: &SessionOptions,
    ) -> Result<Arc<harness_memory::MemoryManager>, ToolError> {
        self.harness
            .memory_manager_for_browser(options)
            .await
            .map_err(memory_tool_error)
    }

    async fn memory_operation_policy(
        &self,
        options: &SessionOptions,
        actor: harness_contracts::MemoryActor,
        permission: harness_contracts::MemoryPermissionContext,
        evidence: harness_contracts::MemoryEvidence,
    ) -> Result<harness_memory::MemoryOperationPolicy, ToolError> {
        let (_engine, thread) = memory_policy_for_session(options).map_err(memory_tool_error)?;
        Ok(harness_memory::MemoryOperationPolicy {
            thread,
            actor,
            permission,
            evidence,
        })
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_draft_from_tool(
    options: &SessionOptions,
    draft: &MemoryToolDraft,
) -> Result<harness_contracts::MemoryRecordDraft, ToolError> {
    Ok(harness_contracts::MemoryRecordDraft {
        kind: draft.kind.clone(),
        visibility: memory_visibility_from_tool(options, &draft.visibility)?,
        content: draft.content.clone(),
        metadata: draft.metadata.clone(),
        expires_at: None,
    })
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_visibility_from_tool(
    options: &SessionOptions,
    value: &MemoryToolVisibility,
) -> Result<harness_contracts::MemoryVisibility, ToolError> {
    match value {
        MemoryToolVisibility::Tenant => Ok(harness_contracts::MemoryVisibility::Tenant),
        MemoryToolVisibility::User => options
            .user_id
            .clone()
            .map(|user_id| harness_contracts::MemoryVisibility::User { user_id })
            .ok_or_else(|| {
                ToolError::Validation("user visibility requires a session user_id".to_owned())
            }),
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_visibility_filter(
    options: &SessionOptions,
    visibility: Option<&MemoryToolVisibility>,
) -> Result<harness_memory::MemoryVisibilityFilter, ToolError> {
    match visibility {
        Some(value) => Ok(harness_memory::MemoryVisibilityFilter::Exact(
            memory_visibility_from_tool(options, value)?,
        )),
        None => Ok(harness_memory::MemoryVisibilityFilter::EffectiveFor(
            memory_actor_from_options(options),
        )),
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_record_from_tool_draft(
    tenant_id: harness_contracts::TenantId,
    draft: &harness_contracts::MemoryRecordDraft,
) -> harness_memory::MemoryRecord {
    let now = chrono::Utc::now();
    harness_memory::MemoryRecord {
        id: harness_contracts::MemoryId::new(),
        tenant_id,
        kind: draft.kind.clone(),
        visibility: draft.visibility.clone(),
        content: draft.content.clone(),
        metadata: harness_memory::MemoryMetadata {
            tags: draft.metadata.tags.clone(),
            source: harness_contracts::MemorySource::ToolOutput,
            evidence: None,
            confidence: draft.metadata.source_trust.clamp(0.0, 1.0) as f32,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 0.0,
            ttl: draft.metadata.ttl,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_tool_record_view(
    record: &harness_memory::MemoryRecord,
    provider_id: &str,
) -> MemoryToolRecordView {
    MemoryToolRecordView {
        memory_id: record.id,
        provider_id: provider_id.to_owned(),
        kind: record.kind.clone(),
        visibility: record.visibility.clone(),
        redacted_content: Some(redacted_memory_content_preview().to_owned()),
        content_hash: content_hash(&record.content),
        score: None,
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn redacted_memory_content_preview() -> &'static str {
    "[redacted memory content]"
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_tool_response(
    action: &str,
    state: MemoryToolState,
    memory_ids: Vec<harness_contracts::MemoryId>,
    candidate_ids: Vec<harness_contracts::MemoryCandidateId>,
    records: Vec<MemoryToolRecordView>,
    action_plan_id: Option<harness_contracts::ActionPlanId>,
    takes_effect: MemoryTakesEffect,
) -> MemoryToolResponse {
    MemoryToolResponse {
        action: action.to_owned(),
        state,
        memory_ids,
        candidate_ids,
        redaction: MemoryRedactionSummary {
            redacted_count: records.len().min(u32::MAX as usize) as u32,
            dropped_count: 0,
        },
        records,
        next_cursor: None,
        action_plan_id,
        denial: None,
        trace_id: None,
        takes_effect,
    }
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_evidence_from_tool(
    request: &MemoryToolRuntimeRequest,
    content: &str,
) -> harness_contracts::MemoryEvidence {
    harness_contracts::MemoryEvidence {
        source: harness_contracts::MemorySource::ToolOutput,
        origin: harness_contracts::MemoryEvidenceOrigin::BuiltinToolOutput {
            tool_name: "memory".to_owned(),
            tool_use_id: request.tool_use_id,
        },
        content_hash: content_hash(content),
        session_id: Some(request.session_id),
        run_id: Some(request.run_id),
        message_id: None,
        tool_use_id: Some(request.tool_use_id),
    }
}

#[cfg(all(
    test,
    feature = "memory-provider-registry",
    feature = "builtin-toolset"
))]
mod tests {
    use super::*;

    #[test]
    fn memory_tool_response_preserves_action_plan_id() {
        let action_plan_id = harness_contracts::ActionPlanId::new();
        let response = memory_tool_response(
            "create",
            MemoryToolState::Completed,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(action_plan_id),
            MemoryTakesEffect::NextTurn,
        );

        assert_eq!(response.action_plan_id, Some(action_plan_id));
    }

    #[tokio::test]
    #[cfg(feature = "testing")]
    async fn memory_tool_search_preserves_per_record_provider_ids() {
        let workspace = unique_test_workspace("sdk-memory-tool-provider-ids");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = harness_contracts::SessionId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(crate::testing::TestModelProvider::default())
            .with_store(crate::testing::InMemoryEventStore::new(Arc::new(
                harness_contracts::NoopRedactor,
            )))
            .with_sandbox(crate::testing::NoopSandbox::new())
            .with_memory_provider(TestMemoryProvider::new("first", "first memory"))
            .with_memory_provider(TestMemoryProvider::new("second", "second memory"))
            .build()
            .await
            .unwrap();
        let runtime = SdkMemoryToolRuntime {
            harness,
            options: options.clone(),
        };

        let response = runtime
            .execute(MemoryToolRuntimeRequest {
                action: MemoryToolRuntimeAction::Search {
                    query: "memory".to_owned(),
                    max_records: 10,
                    visibility: None,
                },
                permission_context: harness_contracts::MemoryPermissionContext {
                    explicit_user_instruction: false,
                    action_plan_id: None,
                    authorization_ticket_id: None,
                    non_interactive_policy_grant: false,
                },
                provider_policy: harness_contracts::MemoryProviderSelectionPolicy::PolicySelected,
                tenant_id: options.tenant_id,
                session_id,
                run_id: harness_contracts::RunId::new(),
                tool_use_id: harness_contracts::ToolUseId::new(),
                workspace_root: workspace,
            })
            .await
            .unwrap();

        let mut provider_ids = response
            .records
            .into_iter()
            .map(|record| record.provider_id)
            .collect::<Vec<_>>();
        provider_ids.sort();
        assert_eq!(provider_ids, vec!["first", "second"]);
    }

    #[tokio::test]
    #[cfg(feature = "testing")]
    async fn memory_tool_create_respects_required_provider_policy() {
        let workspace = unique_test_workspace("sdk-memory-tool-required-provider");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = harness_contracts::SessionId::new();
        let first = Arc::new(TestMemoryProvider::new("first", "first memory"));
        let second = Arc::new(TestMemoryProvider::new("second", "second memory"));
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(crate::testing::TestModelProvider::default())
            .with_store(crate::testing::InMemoryEventStore::new(Arc::new(
                harness_contracts::NoopRedactor,
            )))
            .with_sandbox(crate::testing::NoopSandbox::new())
            .with_memory_provider_arc(first.clone())
            .with_memory_provider_arc(second.clone())
            .build()
            .await
            .unwrap();
        let runtime = SdkMemoryToolRuntime {
            harness,
            options: options.clone(),
        };

        runtime
            .execute(MemoryToolRuntimeRequest {
                action: MemoryToolRuntimeAction::Create {
                    draft: MemoryToolDraft {
                        kind: harness_contracts::MemoryKind::UserPreference,
                        visibility: MemoryToolVisibility::Tenant,
                        content: "write to required provider".to_owned(),
                        metadata: harness_contracts::MemoryMetadata {
                            ttl: None,
                            tags: Vec::new(),
                            source_trust: 1.0,
                        },
                    },
                },
                permission_context: harness_contracts::MemoryPermissionContext {
                    explicit_user_instruction: false,
                    action_plan_id: Some(harness_contracts::ActionPlanId::new()),
                    authorization_ticket_id: Some(harness_contracts::AuthorizationTicketId::new()),
                    non_interactive_policy_grant: false,
                },
                provider_policy:
                    harness_contracts::MemoryProviderSelectionPolicy::RequireProvider {
                        provider_id: "second".to_owned(),
                    },
                tenant_id: options.tenant_id,
                session_id,
                run_id: harness_contracts::RunId::new(),
                tool_use_id: harness_contracts::ToolUseId::new(),
                workspace_root: workspace,
            })
            .await
            .unwrap();

        assert_eq!(first.upserts(), 0);
        assert_eq!(second.upserts(), 1);
    }

    #[cfg(feature = "testing")]
    fn unique_test_workspace(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "jyowo-{name}-{}-{}",
            std::process::id(),
            harness_contracts::SessionId::new()
        ))
    }

    #[cfg(feature = "testing")]
    struct TestMemoryProvider {
        provider_id: String,
        record: harness_memory::MemoryRecord,
        upserts: std::sync::atomic::AtomicUsize,
    }

    #[cfg(feature = "testing")]
    impl TestMemoryProvider {
        fn new(provider_id: &str, content: &str) -> Self {
            let now = chrono::Utc::now();
            Self {
                provider_id: provider_id.to_owned(),
                record: harness_memory::MemoryRecord {
                    id: harness_contracts::MemoryId::new(),
                    tenant_id: harness_contracts::TenantId::SINGLE,
                    kind: harness_contracts::MemoryKind::UserPreference,
                    visibility: harness_contracts::MemoryVisibility::Tenant,
                    content: content.to_owned(),
                    metadata: harness_memory::MemoryMetadata {
                        tags: Vec::new(),
                        source: harness_contracts::MemorySource::UserInput,
                        evidence: None,
                        confidence: 1.0,
                        access_count: 0,
                        last_accessed_at: None,
                        recall_score: 1.0,
                        ttl: None,
                        redacted_segments: 0,
                    },
                    created_at: now,
                    updated_at: now,
                },
                upserts: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn upserts(&self) -> usize {
            self.upserts.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[cfg(feature = "testing")]
    #[async_trait::async_trait]
    impl harness_memory::MemoryStore for TestMemoryProvider {
        fn provider_id(&self) -> &str {
            &self.provider_id
        }

        async fn recall(
            &self,
            _query: harness_memory::MemoryQuery,
        ) -> Result<Vec<harness_memory::MemoryRecord>, harness_contracts::MemoryError> {
            Ok(vec![self.record.clone()])
        }

        async fn get(
            &self,
            id: harness_contracts::MemoryId,
        ) -> Result<harness_memory::MemoryRecord, harness_contracts::MemoryError> {
            if self.record.id == id {
                Ok(self.record.clone())
            } else {
                Err(harness_contracts::MemoryError::NotFound(id))
            }
        }

        async fn upsert(
            &self,
            record: harness_memory::MemoryRecord,
        ) -> Result<harness_contracts::MemoryId, harness_contracts::MemoryError> {
            self.upserts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(record.id)
        }

        async fn forget(
            &self,
            _id: harness_contracts::MemoryId,
        ) -> Result<(), harness_contracts::MemoryError> {
            Ok(())
        }

        async fn list(
            &self,
            _scope: harness_memory::MemoryListScope,
        ) -> Result<Vec<harness_memory::MemorySummary>, harness_contracts::MemoryError> {
            Ok(vec![harness_memory::MemorySummary {
                id: self.record.id,
                kind: self.record.kind.clone(),
                visibility: self.record.visibility.clone(),
                content_preview: harness_memory::content_preview(&self.record.content),
                metadata: self.record.metadata.clone(),
                updated_at: self.record.updated_at,
            }])
        }
    }

    #[cfg(feature = "testing")]
    impl harness_memory::MemoryLifecycle for TestMemoryProvider {}

    #[cfg(feature = "testing")]
    impl harness_memory::MemoryProvider for TestMemoryProvider {}
}

#[cfg(feature = "memory-provider-registry")]
fn content_hash(content: &str) -> harness_contracts::ContentHash {
    harness_contracts::ContentHash(*blake3::hash(content.as_bytes()).as_bytes())
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_error(error: harness_contracts::MemoryError) -> ToolError {
    ToolError::Internal(error.to_string())
}

#[cfg(all(feature = "memory-provider-registry", feature = "builtin-toolset"))]
fn memory_tool_error(error: HarnessError) -> ToolError {
    ToolError::Internal(error.to_string())
}

#[cfg(feature = "memory-provider-registry")]
fn memory_actor_from_options(options: &SessionOptions) -> harness_contracts::MemoryActorContext {
    harness_contracts::MemoryActorContext {
        tenant_id: options.tenant_id,
        user_id: options.user_id.clone(),
        team_id: options.team_id,
        session_id: Some(options.session_id),
    }
}

#[cfg(feature = "memory-provider-registry")]
fn memory_db_path(options: &SessionOptions) -> std::path::PathBuf {
    options
        .workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("memory")
        .join("memory.sqlite3")
}

#[cfg(feature = "memory-provider-registry")]
fn memory_settings_store_for_session(
    options: &SessionOptions,
) -> Result<harness_memory::settings::MemorySettingsStore, HarnessError> {
    harness_memory::settings::MemorySettingsStore::open(&memory_db_path(options).to_string_lossy())
        .map_err(|error| HarnessError::Memory(harness_contracts::MemoryError::Message(error)))
}

#[cfg(feature = "memory-provider-registry")]
fn memory_policy_for_session(
    options: &SessionOptions,
) -> Result<
    (
        harness_memory::MemoryPolicyEngine,
        harness_contracts::MemoryThreadSettings,
    ),
    HarnessError,
> {
    let store = memory_settings_store_for_session(options)?;
    let global = store
        .get_global(options.tenant_id)
        .map_err(|error| HarnessError::Memory(harness_contracts::MemoryError::Message(error)))?;
    let thread = store
        .get_thread(options.tenant_id, options.session_id)
        .map_err(|error| HarnessError::Memory(harness_contracts::MemoryError::Message(error)))?;
    Ok((harness_memory::MemoryPolicyEngine::new(global), thread))
}

#[cfg(feature = "memory-provider-registry")]
pub(super) fn memory_thread_settings_for_session(
    options: &SessionOptions,
) -> Result<harness_contracts::MemoryThreadSettings, HarnessError> {
    let store = memory_settings_store_for_session(options)?;
    store
        .get_thread(options.tenant_id, options.session_id)
        .map_err(|error| HarnessError::Memory(harness_contracts::MemoryError::Message(error)))
}

#[cfg(feature = "memory-provider-registry")]
fn manual_user_memory_permission(
    action_plan_id: Option<harness_contracts::ActionPlanId>,
) -> harness_contracts::MemoryPermissionContext {
    harness_contracts::MemoryPermissionContext {
        explicit_user_instruction: true,
        action_plan_id,
        authorization_ticket_id: None,
        non_interactive_policy_grant: false,
    }
}

#[cfg(feature = "memory-provider-registry")]
fn ensure_distinct_memory_candidates(
    candidate_ids: &[harness_contracts::MemoryCandidateId],
) -> Result<(), HarnessError> {
    let distinct = candidate_ids
        .iter()
        .map(ToString::to_string)
        .collect::<std::collections::HashSet<_>>();
    if distinct.len() != candidate_ids.len() {
        return Err(HarnessError::Memory(
            harness_contracts::MemoryError::Message("candidate ids must be distinct".to_owned()),
        ));
    }
    Ok(())
}

#[cfg(feature = "memory-provider-registry")]
fn merged_candidate_evidence(
    candidates: &[harness_contracts::MemoryCandidate],
    merged_content: &str,
) -> harness_contracts::MemoryEvidence {
    let Some(first) = candidates.first() else {
        return harness_contracts::MemoryEvidence {
            source: harness_contracts::MemorySource::AgentDerived,
            origin: harness_contracts::MemoryEvidenceOrigin::Imported {
                importer: "memory-candidate-merge".to_owned(),
                import_id: "empty-candidate-set".to_owned(),
            },
            content_hash: content_hash(merged_content),
            session_id: None,
            run_id: None,
            message_id: None,
            tool_use_id: None,
        };
    };
    let mut evidence = first.evidence.clone();
    evidence.content_hash = content_hash(merged_content);
    evidence
}

#[cfg(feature = "memory-provider-registry")]
fn manual_memory_evidence(
    options: &SessionOptions,
    action_plan_id: Option<harness_contracts::ActionPlanId>,
    operation: &str,
    content: &str,
) -> harness_contracts::MemoryEvidence {
    let import_id = action_plan_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| operation.to_owned());
    harness_contracts::MemoryEvidence {
        source: harness_contracts::MemorySource::UserInput,
        origin: harness_contracts::MemoryEvidenceOrigin::Imported {
            importer: operation.to_owned(),
            import_id,
        },
        content_hash: content_hash(content),
        session_id: Some(options.session_id),
        run_id: None,
        message_id: None,
        tool_use_id: None,
    }
}

#[cfg(feature = "memory-provider-registry")]
fn memory_inbox_for_session(
    options: &SessionOptions,
) -> Result<harness_memory::MemoryInbox, HarnessError> {
    harness_memory::MemoryInbox::open(
        &memory_db_path(options).to_string_lossy(),
        options.tenant_id,
    )
    .map_err(|error| HarnessError::Memory(harness_contracts::MemoryError::Message(error)))
}

#[cfg(feature = "memory-provider-registry")]
fn memory_trace_collector_for_session(
    options: &SessionOptions,
) -> Result<harness_memory::MemoryRecallTraceCollector, HarnessError> {
    harness_memory::MemoryRecallTraceCollector::open(&memory_db_path(options).to_string_lossy())
        .map_err(|error| HarnessError::Memory(harness_contracts::MemoryError::Message(error)))
}

#[cfg(feature = "memory-provider-registry")]
fn enforce_memory_tenant(
    options: &SessionOptions,
    tenant_id: harness_contracts::TenantId,
) -> Result<(), HarnessError> {
    if tenant_id == options.tenant_id {
        return Ok(());
    }
    Err(HarnessError::Memory(
        harness_contracts::MemoryError::Message(
            "memory tenant does not match session tenant".to_owned(),
        ),
    ))
}

#[cfg(feature = "memory-provider-registry")]
fn candidate_belongs_to_session(
    candidate: &harness_contracts::MemoryCandidate,
    session_id: harness_contracts::SessionId,
) -> bool {
    candidate.evidence.session_id == Some(session_id)
        || candidate.evidence.origin.session_id() == Some(session_id)
}

#[cfg(feature = "memory-provider-registry")]
fn memory_candidate_list_item(
    candidate: harness_contracts::MemoryCandidate,
) -> harness_contracts::MemoryCandidateListItem {
    harness_contracts::MemoryCandidateListItem {
        id: candidate.id,
        state: candidate.state,
        proposed_record: candidate.proposed_record,
        evidence: candidate.evidence,
        created_at: candidate.created_at,
        expires_at: candidate.expires_at,
    }
}

#[cfg(feature = "memory-provider-registry")]
fn memory_record_from_candidate(
    candidate: &harness_contracts::MemoryCandidate,
) -> harness_memory::MemoryRecord {
    let now = chrono::Utc::now();
    harness_memory::MemoryRecord {
        id: harness_contracts::MemoryId::new(),
        tenant_id: candidate.tenant_id,
        kind: candidate.proposed_record.kind.clone(),
        visibility: candidate.proposed_record.visibility.clone(),
        content: candidate.proposed_record.content.clone(),
        metadata: harness_memory::MemoryMetadata {
            tags: candidate.proposed_record.metadata.tags.clone(),
            source: candidate.evidence.source.clone(),
            evidence: Some(candidate.evidence.clone()),
            confidence: candidate
                .proposed_record
                .metadata
                .source_trust
                .clamp(0.0, 1.0) as f32,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 0.0,
            ttl: candidate.proposed_record.metadata.ttl,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

#[cfg(feature = "memory-provider-registry")]
trait MemoryEvidenceOriginSession {
    fn session_id(&self) -> Option<harness_contracts::SessionId>;
}

#[cfg(feature = "memory-provider-registry")]
impl MemoryEvidenceOriginSession for harness_contracts::MemoryEvidenceOrigin {
    fn session_id(&self) -> Option<harness_contracts::SessionId> {
        match self {
            harness_contracts::MemoryEvidenceOrigin::UserMessage { session_id, .. }
            | harness_contracts::MemoryEvidenceOrigin::AssistantMessage { session_id, .. } => {
                Some(*session_id)
            }
            harness_contracts::MemoryEvidenceOrigin::SubagentOutput {
                parent_session_id, ..
            } => Some(*parent_session_id),
            _ => None,
        }
    }
}

pub(super) fn record_memory_summary_event(state: &mut MemorySessionSummaryState, event: &Event) {
    match event {
        Event::UserMessageAppended(_) => {
            state.turn_count = state.turn_count.saturating_add(1);
        }
        Event::AssistantMessageCompleted(completed) => {
            state.final_assistant_text = message_content_text(&completed.content);
        }
        Event::ToolUseCompleted(_) | Event::ToolUseFailed(_) => {
            state.tool_use_count = state.tool_use_count.saturating_add(1);
        }
        _ => {}
    }
}

fn message_content_text(content: &MessageContent) -> Option<String> {
    match content {
        MessageContent::Text(text) => Some(text.clone()),
        MessageContent::Structured(value) => Some(value.to_string()),
        MessageContent::Multimodal(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
    }
}

#[cfg(feature = "memory-provider-registry")]
pub(super) struct SdkMemoryEventSink {
    pub(super) event_store: Arc<dyn EventStore>,
    pub(super) tenant_id: TenantId,
    pub(super) session_id: harness_contracts::SessionId,
}

#[cfg(feature = "memory-provider-registry")]
#[async_trait]
impl harness_memory::MemoryEventSink for SdkMemoryEventSink {
    async fn emit(&self, event: Event) {
        let _ = self
            .event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await;
    }

    async fn emit_required(&self, event: Event) -> Result<(), harness_contracts::MemoryError> {
        self.event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await
            .map(|_| ())
            .map_err(|error| harness_contracts::MemoryError::Provider {
                provider: "journal".to_owned(),
                source_message: error.to_string(),
            })
    }
}
