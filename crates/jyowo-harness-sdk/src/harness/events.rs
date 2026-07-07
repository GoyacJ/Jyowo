use super::*;

impl Harness {
    pub async fn audit_query(
        &self,
        tenant: TenantId,
        query: AuditQuery,
        caller_trust: TrustLevel,
    ) -> Result<AuditPage, HarnessError> {
        if caller_trust != TrustLevel::AdminTrusted {
            return Err(HarnessError::PermissionDenied(
                "audit query requires admin-trusted caller".to_owned(),
            ));
        }

        EventStoreAudit::new(Arc::clone(&self.inner.event_store))
            .query(tenant, query)
            .await
            .map_err(HarnessError::Journal)
    }

    pub fn event_store(&self) -> Arc<dyn EventStore> {
        Arc::new(ConversationDeletionGuardEventStore {
            inner: Arc::clone(&self.inner.event_store),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            evidence_ref_store: self.inner.evidence_ref_store.clone(),
        })
    }

    pub async fn event_stream(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        cursor: ReplayCursor,
    ) -> Result<harness_journal::EventStream, HarnessError> {
        let redactor = self.hook_redactor();
        let stream = self
            .inner
            .event_store
            .read(tenant_id, session_id, cursor)
            .await
            .map_err(HarnessError::Journal)?
            .map(move |event| redact_business_event_for_display(event, redactor.as_ref()));
        Ok(Box::pin(stream))
    }
}

pub(super) struct LifecycleHookEventStore {
    pub(super) inner: Arc<dyn EventStore>,
    pub(super) hooks: HookDispatcher,
    pub(super) tenant_id: TenantId,
    pub(super) session_id: harness_contracts::SessionId,
    #[cfg(feature = "memory-provider-registry")]
    pub(super) user_id: Option<String>,
    #[cfg(feature = "memory-provider-registry")]
    pub(super) team_id: Option<harness_contracts::TeamId>,
    pub(super) project_workspace_root: Option<PathBuf>,
    #[cfg(feature = "memory-provider-registry")]
    pub(super) memory_database_path: PathBuf,
    pub(super) redactor: Arc<dyn Redactor>,
    pub(super) session_limits: Arc<SessionLimitState>,
    pub(super) deleted_conversation_sessions:
        Arc<parking_lot::Mutex<HashSet<(TenantId, SessionId)>>>,
    pub(super) evidence_ref_store: Option<Arc<harness_journal::EvidenceRefStore>>,
    pub(super) summary_state: parking_lot::Mutex<MemorySessionSummaryState>,
    #[cfg(feature = "memory-provider-registry")]
    pub(super) memory_manager: Option<Arc<harness_memory::MemoryManager>>,
}

#[derive(Debug, Default, Clone)]
pub(super) struct MemorySessionSummaryState {
    pub(super) turn_count: u32,
    pub(super) tool_use_count: u32,
    pub(super) final_assistant_text: Option<String>,
    pub(super) final_assistant_message_id: Option<MessageId>,
    pub(super) final_assistant_run_id: Option<RunId>,
    pub(super) has_external_context: bool,
}

pub(super) struct ConversationDeletionGuardEventStore {
    pub(super) inner: Arc<dyn EventStore>,
    pub(super) deleted_conversation_sessions:
        Arc<parking_lot::Mutex<HashSet<(TenantId, SessionId)>>>,
    pub(super) evidence_ref_store: Option<Arc<harness_journal::EvidenceRefStore>>,
}

#[derive(Default)]
pub(super) struct PendingSessionEvents {
    events: parking_lot::Mutex<Vec<Event>>,
}

impl PendingSessionEvents {
    pub(super) fn push(&self, event: Event) {
        self.events.lock().push(event);
    }

    pub(super) fn drain(&self) -> Vec<Event> {
        self.events.lock().drain(..).collect()
    }
}

impl ConversationDeletionGuardEventStore {
    fn ensure_not_deleted(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<(), harness_contracts::JournalError> {
        if self
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant, session_id))
        {
            return Err(harness_contracts::JournalError::Message(format!(
                "conversation session was deleted: {session_id}"
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl EventStore for ConversationDeletionGuardEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        self.ensure_not_deleted(tenant, session_id)?;
        self.inner.append(tenant, session_id, events).await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        self.ensure_not_deleted(tenant, session_id)?;
        self.inner
            .append_with_metadata(tenant, session_id, metadata, events)
            .await
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, harness_contracts::JournalError> {
        self.inner.read_envelopes(tenant, session_id, cursor).await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<harness_contracts::EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, harness_contracts::JournalError> {
        self.inner.query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, harness_contracts::JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), harness_contracts::JournalError> {
        self.ensure_not_deleted(tenant, snapshot.session_id)?;
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: harness_contracts::ForkReason,
    ) -> Result<(), harness_contracts::JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, harness_contracts::JournalError> {
        invalidate_session_evidence_refs(self.evidence_ref_store.as_ref(), tenant, session_id)
            .await?;
        self.inner.delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, harness_contracts::JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, harness_contracts::JournalError> {
        prune_with_evidence_invalidation(
            &self.inner,
            self.evidence_ref_store.as_ref(),
            tenant,
            policy,
        )
        .await
    }

    async fn prune_sessions(
        &self,
        tenant: TenantId,
        session_ids: &[SessionId],
        keep_snapshots: bool,
    ) -> Result<PruneReport, harness_contracts::JournalError> {
        if let Some(evidence_ref_store) = self.evidence_ref_store.as_ref() {
            invalidate_sessions_evidence_refs(evidence_ref_store, tenant, session_ids).await?;
        }
        self.inner
            .prune_sessions(tenant, session_ids, keep_snapshots)
            .await
    }
}

#[async_trait]
impl EventStore for LifecycleHookEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        if self
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant, session_id))
        {
            return Err(harness_contracts::JournalError::Message(format!(
                "conversation session was deleted: {session_id}"
            )));
        }
        let mut combined = events.to_vec();
        combined.extend(self.lifecycle_hook_events(events).await?);
        let result = self.inner.append(tenant, session_id, &combined).await;
        if result.is_ok()
            && events
                .iter()
                .any(|event| matches!(event, Event::SessionEnded(_)))
        {
            self.session_limits.release();
        }
        result
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        if self
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant, session_id))
        {
            return Err(harness_contracts::JournalError::Message(format!(
                "conversation session was deleted: {session_id}"
            )));
        }
        let mut combined = events.to_vec();
        combined.extend(self.lifecycle_hook_events(events).await?);
        let result = self
            .inner
            .append_with_metadata(tenant, session_id, metadata, &combined)
            .await;
        if result.is_ok()
            && events
                .iter()
                .any(|event| matches!(event, Event::SessionEnded(_)))
        {
            self.session_limits.release();
        }
        result
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, harness_contracts::JournalError> {
        self.inner.read_envelopes(tenant, session_id, cursor).await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<harness_contracts::EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, harness_contracts::JournalError> {
        self.inner.query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<Option<SessionSnapshot>, harness_contracts::JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), harness_contracts::JournalError> {
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: harness_contracts::SessionId,
        child: harness_contracts::SessionId,
        reason: harness_contracts::ForkReason,
    ) -> Result<(), harness_contracts::JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<bool, harness_contracts::JournalError> {
        invalidate_session_evidence_refs(self.evidence_ref_store.as_ref(), tenant, session_id)
            .await?;
        self.inner.delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, harness_contracts::JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, harness_contracts::JournalError> {
        prune_with_evidence_invalidation(
            &self.inner,
            self.evidence_ref_store.as_ref(),
            tenant,
            policy,
        )
        .await
    }

    async fn prune_sessions(
        &self,
        tenant: TenantId,
        session_ids: &[SessionId],
        keep_snapshots: bool,
    ) -> Result<PruneReport, harness_contracts::JournalError> {
        if let Some(evidence_ref_store) = self.evidence_ref_store.as_ref() {
            invalidate_sessions_evidence_refs(evidence_ref_store, tenant, session_ids).await?;
        }
        self.inner
            .prune_sessions(tenant, session_ids, keep_snapshots)
            .await
    }
}

async fn prune_with_evidence_invalidation(
    inner: &Arc<dyn EventStore>,
    evidence_ref_store: Option<&Arc<harness_journal::EvidenceRefStore>>,
    tenant: TenantId,
    policy: PrunePolicy,
) -> Result<PruneReport, harness_contracts::JournalError> {
    let Some(evidence_ref_store) = evidence_ref_store else {
        return inner.prune(tenant, policy).await;
    };

    let candidate_sessions = prune_candidate_sessions(inner.as_ref(), tenant, &policy).await?;
    invalidate_sessions_evidence_refs(evidence_ref_store, tenant, &candidate_sessions).await?;
    if candidate_sessions.is_empty() {
        return Ok(PruneReport::default());
    }
    inner
        .prune_sessions(tenant, &candidate_sessions, policy.keep_snapshots)
        .await
}

async fn invalidate_session_evidence_refs(
    evidence_ref_store: Option<&Arc<harness_journal::EvidenceRefStore>>,
    tenant: TenantId,
    session_id: SessionId,
) -> Result<(), harness_contracts::JournalError> {
    if let Some(evidence_ref_store) = evidence_ref_store {
        evidence_ref_store
            .delete_for_conversation(tenant, &session_id.to_string())
            .await?;
    }
    Ok(())
}

async fn invalidate_sessions_evidence_refs(
    evidence_ref_store: &Arc<harness_journal::EvidenceRefStore>,
    tenant: TenantId,
    session_ids: &[SessionId],
) -> Result<(), harness_contracts::JournalError> {
    for session_id in session_ids {
        evidence_ref_store
            .delete_for_conversation(tenant, &session_id.to_string())
            .await?;
    }
    Ok(())
}

async fn prune_candidate_sessions(
    event_store: &dyn EventStore,
    tenant: TenantId,
    policy: &PrunePolicy,
) -> Result<Vec<SessionId>, harness_contracts::JournalError> {
    let mut sessions = event_store
        .list_sessions(
            tenant,
            SessionFilter {
                since: None,
                end_reason: None,
                project_compression_tips: false,
                limit: u32::MAX,
            },
        )
        .await?;
    sessions.sort_by_key(|summary| summary.last_event_at);
    sessions.reverse();
    let keep: HashSet<_> = policy
        .keep_latest_n_sessions
        .map(|limit| {
            sessions
                .iter()
                .take(limit as usize)
                .map(|summary| summary.session_id)
                .collect()
        })
        .unwrap_or_default();
    let cutoff = harness_contracts::now()
        - chrono::Duration::from_std(policy.older_than)
            .unwrap_or_else(|_| chrono::Duration::zero());
    Ok(sessions
        .into_iter()
        .filter_map(|summary| {
            (summary.last_event_at <= cutoff && !keep.contains(&summary.session_id))
                .then_some(summary.session_id)
        })
        .collect())
}

impl LifecycleHookEventStore {
    async fn lifecycle_hook_events(
        &self,
        events: &[Event],
    ) -> Result<Vec<Event>, harness_contracts::JournalError> {
        let mut output = Vec::new();
        for event in events {
            self.record_memory_summary_event(event);
            match event {
                Event::SessionCreated(created) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Setup {
                            workspace_root: self.project_workspace_root.clone(),
                        })
                        .await?,
                    );
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SessionStart {
                            session_id: created.session_id,
                        })
                        .await?,
                    );
                }
                Event::SessionEnded(ended) => {
                    self.call_memory_session_end(ended).await?;
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SessionEnd {
                            session_id: ended.session_id,
                            reason: ended.reason.clone(),
                        })
                        .await?,
                    );
                }
                Event::SubagentSpawned(spawned) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SubagentStart {
                            subagent_id: spawned.subagent_id,
                            spec: SubagentSpecView {
                                name: spawned.agent_ref.name.clone(),
                                description: spawned.trigger_tool_name.clone(),
                            },
                        })
                        .await?,
                    );
                }
                Event::SubagentTerminated(terminated) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SubagentStop {
                            subagent_id: terminated.subagent_id,
                            status: subagent_status_from_reason(&terminated.reason),
                        })
                        .await?,
                    );
                }
                Event::McpElicitationRequested(requested) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Elicitation {
                            mcp_server_id: requested.server_id.clone(),
                            schema: json!({
                                "subject": &requested.subject,
                                "summary": &requested.schema_summary,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpConnectionLost(lost) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Warning,
                            body: json!({
                                "kind": "mcp_connection_lost",
                                "server_id": &lost.server_id,
                                "terminal": lost.terminal,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpConnectionRecovered(recovered) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_connection_recovered",
                                "server_id": &recovered.server_id,
                                "schema_changed": recovered.schema_changed,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpToolsListChanged(changed) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_tools_list_changed",
                                "server_id": &changed.server_id,
                                "added_count": changed.added_count,
                                "removed_count": changed.removed_count,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpResourceUpdated(updated) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_resource_updated",
                                "server_id": &updated.server_id,
                                "resource_kind": &updated.kind,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpSamplingRequested(requested) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_sampling_requested",
                                "server_id": &requested.server_id,
                                "request_id": requested.request_id,
                                "outcome": &requested.outcome,
                            }),
                        })
                        .await?,
                    );
                }
                _ => {}
            }
        }
        Ok(output)
    }

    async fn dispatch_lifecycle_hook(
        &self,
        event: HookEvent,
    ) -> Result<Vec<Event>, harness_contracts::JournalError> {
        let kind = event.kind();
        let result = self
            .hooks
            .dispatch(event, self.hook_context())
            .await
            .map_err(|error| harness_contracts::JournalError::Message(error.to_string()))?;
        Ok(sdk_hook_events(kind, &result, None))
    }

    fn hook_context(&self) -> HookContext {
        HookContext {
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            run_id: None,
            turn_index: None,
            correlation_id: harness_contracts::CorrelationId::new(),
            causation_id: harness_contracts::CausationId::new(),
            trust_level: TrustLevel::AdminTrusted,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::NoInteractive,
            at: harness_contracts::now(),
            view: Arc::new(SdkHookView {
                workspace_root: self.project_workspace_root.clone(),
                redactor: Arc::clone(&self.redactor),
            }),
            upstream_outcome: None,
            replay_mode: ReplayMode::Live,
        }
    }

    fn record_memory_summary_event(&self, event: &Event) {
        let mut state = self.summary_state.lock();
        record_memory_summary_event(&mut state, event);
    }

    #[cfg(feature = "memory-provider-registry")]
    async fn memory_summary_state(&self) -> MemorySessionSummaryState {
        let fallback = self.summary_state.lock().clone();
        let Ok(mut stream) = self
            .inner
            .read_envelopes(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
        else {
            return fallback;
        };
        let mut state = MemorySessionSummaryState::default();
        while let Some(envelope) = stream.next().await {
            record_memory_summary_event(&mut state, &envelope.payload);
        }
        if state.final_assistant_text.is_none() {
            state.final_assistant_text = fallback.final_assistant_text;
            state.final_assistant_message_id = fallback.final_assistant_message_id;
            state.final_assistant_run_id = fallback.final_assistant_run_id;
        }
        state.turn_count = state.turn_count.max(fallback.turn_count);
        state.tool_use_count = state.tool_use_count.max(fallback.tool_use_count);
        state.has_external_context |= fallback.has_external_context;
        state
    }

    #[cfg(feature = "memory-provider-registry")]
    async fn call_memory_session_end(
        &self,
        ended: &harness_contracts::SessionEndedEvent,
    ) -> Result<(), harness_contracts::JournalError> {
        let Some(memory) = &self.memory_manager else {
            return Ok(());
        };
        let summary_state = self.memory_summary_state().await;
        let ctx = harness_contracts::MemorySessionCtx {
            tenant_id: ended.tenant_id,
            session_id: ended.session_id,
            workspace_id: None,
            user_id: self.user_id.as_deref(),
            team_id: self.team_id,
        };
        let summary = harness_contracts::SessionSummaryView {
            end_reason: ended.reason.clone(),
            turn_count: summary_state.turn_count,
            tool_use_count: summary_state.tool_use_count,
            usage: ended.final_usage.clone(),
            final_assistant_text: summary_state.final_assistant_text.as_deref(),
        };
        self.enqueue_session_end_extraction(ended, &summary_state)?;
        memory
            .on_session_end(&ctx, &summary)
            .await
            .map_err(memory_journal_error)?;
        Ok(())
    }

    #[cfg(not(feature = "memory-provider-registry"))]
    async fn call_memory_session_end(
        &self,
        _ended: &harness_contracts::SessionEndedEvent,
    ) -> Result<(), harness_contracts::JournalError> {
        Ok(())
    }

    #[cfg(feature = "memory-provider-registry")]
    fn enqueue_session_end_extraction(
        &self,
        ended: &harness_contracts::SessionEndedEvent,
        summary_state: &MemorySessionSummaryState,
    ) -> Result<(), harness_contracts::JournalError> {
        let Some(message_id) = summary_state.final_assistant_message_id else {
            return Ok(());
        };
        let Some(run_id) = summary_state.final_assistant_run_id else {
            return Ok(());
        };
        let Some(text) = summary_state.final_assistant_text.as_deref() else {
            return Ok(());
        };
        if text.trim().is_empty() {
            return Ok(());
        }
        let settings_store = harness_memory::settings::MemorySettingsStore::open(
            &self.memory_database_path.to_string_lossy(),
        )
        .map_err(|error| {
            harness_contracts::JournalError::Message(format!(
                "open memory extraction settings failed: {error}"
            ))
        })?;
        let global = settings_store
            .get_global(ended.tenant_id)
            .map_err(|error| {
                harness_contracts::JournalError::Message(format!(
                    "read memory extraction settings failed: {error}"
                ))
            })?;
        if global.max_memory_bytes == 0 {
            return Ok(());
        }
        let current_memory_bytes = settings_store
            .current_memory_bytes(ended.tenant_id)
            .map_err(|error| {
                harness_contracts::JournalError::Message(format!(
                    "read memory extraction quota failed: {error}"
                ))
            })?;
        if current_memory_bytes >= global.max_memory_bytes {
            return Ok(());
        }
        let thread = settings_store
            .get_thread(ended.tenant_id, ended.session_id)
            .map_err(|error| {
                harness_contracts::JournalError::Message(format!(
                    "read memory extraction thread settings failed: {error}"
                ))
            })?;
        let permission = harness_contracts::MemoryPermissionContext {
            explicit_user_instruction: false,
            include_raw_content: false,
            action_plan_id: None,
            authorization_ticket_id: None,
            non_interactive_policy_grant: true,
        };
        let decision = harness_memory::MemoryPolicyEngine::new(global).evaluate_generation(
            &thread,
            summary_state.has_external_context,
            &permission,
        );
        if !matches!(decision, harness_contracts::MemoryPolicyDecision::Allow) {
            return Ok(());
        }
        let memory_db_path = self.memory_database_path.clone();
        let queue = harness_memory::ExtractionJobQueue::open(
            &memory_db_path.to_string_lossy(),
            harness_memory::ExtractionJobConfig::default(),
        )
        .map_err(|error| {
            harness_contracts::JournalError::Message(format!(
                "enqueue memory extraction queue open failed: {error}"
            ))
        })?;
        let hash = blake3::hash(text.as_bytes());
        queue
            .enqueue(harness_memory::ExtractionJob {
                job_id: format!("session-end-{}-{}", ended.session_id, hash.to_hex()),
                tenant_id: ended.tenant_id,
                session_id: ended.session_id,
                run_id,
                source_message_id: Some(message_id),
                source_user_id: self.user_id.clone(),
                source_excerpt: Some(text.to_owned()),
                evidence_hash: *hash.as_bytes(),
                job_kind: harness_memory::ExtractionJobKind::MemoryExtraction,
                state: harness_memory::ExtractionJobState::Queued,
                attempt_count: 0,
                lease_owner: None,
                lease_expires_at: None,
                next_attempt_at: None,
                created_at: harness_contracts::now(),
                updated_at: harness_contracts::now(),
            })
            .map(|_| ())
            .map_err(|error| {
                harness_contracts::JournalError::Message(format!(
                    "enqueue memory extraction failed: {error}"
                ))
            })
    }
}

#[cfg(feature = "memory-provider-registry")]
fn memory_journal_error(error: harness_contracts::MemoryError) -> harness_contracts::JournalError {
    harness_contracts::JournalError::Message(format!("memory session end failed: {error}"))
}

pub(super) fn subagent_status_from_reason(
    reason: &harness_contracts::SubagentTerminationReason,
) -> harness_contracts::SubagentStatus {
    match reason {
        harness_contracts::SubagentTerminationReason::NaturalCompletion => {
            harness_contracts::SubagentStatus::Completed
        }
        harness_contracts::SubagentTerminationReason::ParentCancelled
        | harness_contracts::SubagentTerminationReason::AdminInterrupted { .. } => {
            harness_contracts::SubagentStatus::Cancelled
        }
        harness_contracts::SubagentTerminationReason::Stalled { .. } => {
            harness_contracts::SubagentStatus::Stalled
        }
        harness_contracts::SubagentTerminationReason::BridgeBroken
        | harness_contracts::SubagentTerminationReason::Failed { .. } => {
            harness_contracts::SubagentStatus::Failed
        }
        _ => harness_contracts::SubagentStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_contracts::{BlobId, ConfigHash, NoopRedactor, SessionCreatedEvent, SnapshotId};
    use harness_journal::evidence::{
        EvidenceRefRecord, EvidenceRefSource, InMemoryEvidenceRefRegistry, RedactionProvenance,
    };
    use harness_journal::{InMemoryBlobStore, InMemoryEventStore};

    fn evidence_record(id: &str, session_id: SessionId, bytes: &[u8]) -> EvidenceRefRecord {
        let hash = blake3::hash(bytes);
        EvidenceRefRecord {
            id: EvidenceRefId::new(id),
            kind: EvidenceRefKind::CommandOutput,
            conversation_id: session_id.to_string(),
            run_id: RunId::new().to_string(),
            source_event_refs: Vec::new(),
            artifact_id: None,
            revision_id: None,
            content_type: "text/plain".to_owned(),
            byte_length: bytes.len() as u64,
            content_hash: hash.as_bytes().to_vec(),
            redaction_state: EvidenceRedactionState::Clean,
            redaction_provenance: RedactionProvenance {
                redactor_version: "test".to_owned(),
            },
            retention: BlobRetention::TenantScoped,
            source: EvidenceRefSource::Blob {
                blob_ref: BlobRef {
                    id: BlobId::new(),
                    size: bytes.len() as u64,
                    content_hash: *hash.as_bytes(),
                    content_type: Some("text/plain".to_owned()),
                },
            },
        }
    }

    fn session_created(session_id: SessionId) -> Event {
        Event::SessionCreated(SessionCreatedEvent {
            session_id,
            tenant_id: TenantId::SINGLE,
            options_hash: [1; 32],
            snapshot_id: SnapshotId::from_u128(1),
            effective_config_hash: ConfigHash([2; 32]),
            created_at: harness_contracts::now(),
        })
    }

    #[tokio::test]
    async fn prune_invalidates_matching_evidence_refs() {
        let event_store: Arc<dyn EventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let evidence_store = Arc::new(harness_journal::EvidenceRefStore::new(
            Arc::new(InMemoryEvidenceRefRegistry::new()),
            Arc::new(InMemoryBlobStore::default()),
        ));
        let session_id = SessionId::new();
        let bytes = b"pruned command output".to_vec();
        let record = evidence_record("ref-prune", session_id, &bytes);
        evidence_store
            .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
            .await
            .expect("evidence stores");
        event_store
            .append(TenantId::SINGLE, session_id, &[session_created(session_id)])
            .await
            .expect("event appends");

        let wrapper = ConversationDeletionGuardEventStore {
            inner: Arc::clone(&event_store),
            deleted_conversation_sessions: Arc::new(parking_lot::Mutex::new(HashSet::new())),
            evidence_ref_store: Some(Arc::clone(&evidence_store)),
        };
        wrapper
            .prune(
                TenantId::SINGLE,
                PrunePolicy {
                    older_than: Duration::from_secs(0),
                    keep_snapshots: false,
                    keep_latest_n_sessions: None,
                    target_size_bytes: None,
                },
            )
            .await
            .expect("prune succeeds");

        let error = evidence_store
            .read_evidence(
                TenantId::SINGLE,
                &session_id.to_string(),
                &record.id,
                EvidenceRefKind::CommandOutput,
            )
            .await
            .expect_err("pruned evidence is unreadable");
        assert!(error.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn prune_invalidates_matching_evidence_refs_before_inner_prune() {
        let inner = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let event_store: Arc<dyn EventStore> = Arc::new(PruneFailingEventStore {
            inner: Arc::clone(&inner),
        });
        let evidence_store = Arc::new(harness_journal::EvidenceRefStore::new(
            Arc::new(InMemoryEvidenceRefRegistry::new()),
            Arc::new(InMemoryBlobStore::default()),
        ));
        let session_id = SessionId::new();
        let bytes = b"prune failure command output".to_vec();
        let record = evidence_record("ref-prune-failure", session_id, &bytes);
        evidence_store
            .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
            .await
            .expect("evidence stores");
        inner
            .append(TenantId::SINGLE, session_id, &[session_created(session_id)])
            .await
            .expect("event appends");

        let error = prune_with_evidence_invalidation(
            &event_store,
            Some(&evidence_store),
            TenantId::SINGLE,
            PrunePolicy {
                older_than: Duration::from_secs(0),
                keep_snapshots: false,
                keep_latest_n_sessions: None,
                target_size_bytes: None,
            },
        )
        .await
        .expect_err("inner prune failure is returned");

        assert!(error.to_string().contains("forced prune failure"));
        assert!(evidence_store
            .read_evidence(
                TenantId::SINGLE,
                &session_id.to_string(),
                &record.id,
                EvidenceRefKind::CommandOutput,
            )
            .await
            .is_err());
    }

    struct PruneFailingEventStore {
        inner: Arc<InMemoryEventStore>,
    }

    #[async_trait]
    impl EventStore for PruneFailingEventStore {
        async fn append(
            &self,
            tenant: TenantId,
            session_id: SessionId,
            events: &[Event],
        ) -> Result<JournalOffset, harness_contracts::JournalError> {
            self.inner.append(tenant, session_id, events).await
        }

        async fn append_with_metadata(
            &self,
            tenant: TenantId,
            session_id: SessionId,
            metadata: AppendMetadata,
            events: &[Event],
        ) -> Result<JournalOffset, harness_contracts::JournalError> {
            self.inner
                .append_with_metadata(tenant, session_id, metadata, events)
                .await
        }

        async fn read_envelopes(
            &self,
            tenant: TenantId,
            session_id: SessionId,
            cursor: ReplayCursor,
        ) -> Result<
            futures::stream::BoxStream<'static, EventEnvelope>,
            harness_contracts::JournalError,
        > {
            self.inner.read_envelopes(tenant, session_id, cursor).await
        }

        async fn query_after(
            &self,
            tenant: TenantId,
            after: Option<harness_contracts::EventId>,
            limit: usize,
        ) -> Result<Vec<EventEnvelope>, harness_contracts::JournalError> {
            self.inner.query_after(tenant, after, limit).await
        }

        async fn snapshot(
            &self,
            tenant: TenantId,
            session_id: SessionId,
        ) -> Result<Option<SessionSnapshot>, harness_contracts::JournalError> {
            self.inner.snapshot(tenant, session_id).await
        }

        async fn save_snapshot(
            &self,
            tenant: TenantId,
            snapshot: SessionSnapshot,
        ) -> Result<(), harness_contracts::JournalError> {
            self.inner.save_snapshot(tenant, snapshot).await
        }

        async fn compact_link(
            &self,
            parent: SessionId,
            child: SessionId,
            reason: harness_contracts::ForkReason,
        ) -> Result<(), harness_contracts::JournalError> {
            self.inner.compact_link(parent, child, reason).await
        }

        async fn delete_session(
            &self,
            tenant: TenantId,
            session_id: SessionId,
        ) -> Result<bool, harness_contracts::JournalError> {
            self.inner.delete_session(tenant, session_id).await
        }

        async fn list_sessions(
            &self,
            tenant: TenantId,
            filter: SessionFilter,
        ) -> Result<Vec<SessionSummary>, harness_contracts::JournalError> {
            self.inner.list_sessions(tenant, filter).await
        }

        async fn prune(
            &self,
            _tenant: TenantId,
            _policy: PrunePolicy,
        ) -> Result<PruneReport, harness_contracts::JournalError> {
            panic!("prune_with_evidence_invalidation must use exact-session prune")
        }

        async fn prune_sessions(
            &self,
            _tenant: TenantId,
            _session_ids: &[SessionId],
            _keep_snapshots: bool,
        ) -> Result<PruneReport, harness_contracts::JournalError> {
            Err(harness_contracts::JournalError::Message(
                "forced prune failure".to_owned(),
            ))
        }
    }
}

struct SdkHookView {
    workspace_root: Option<PathBuf>,
    redactor: Arc<dyn Redactor>,
}

impl HookSessionView for SdkHookView {
    fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        Vec::new()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn Redactor {
        self.redactor.as_ref()
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

pub(super) fn sdk_hook_events(
    kind: harness_contracts::HookEventKind,
    result: &DispatchResult,
    fail_closed_denied: Option<harness_contracts::EventId>,
) -> Vec<Event> {
    let mut events = Vec::with_capacity(result.trail.len() + result.failures.len());
    for record in &result.trail {
        events.push(Event::HookTriggered(
            harness_contracts::HookTriggeredEvent {
                hook_event_kind: kind.clone(),
                handler_id: record.handler_id.clone(),
                outcome_summary: hook_outcome_summary(&record.outcome),
                duration_ms: hook_duration_ms(record.duration),
                at: harness_contracts::now(),
            },
        ));
    }
    for failure in &result.failures {
        let causation_id = harness_contracts::EventId::new();
        events.push(Event::HookFailed(harness_contracts::HookFailedEvent {
            hook_event_kind: kind.clone(),
            handler_id: failure.handler_id.clone(),
            failure_mode: failure.mode,
            cause_kind: failure.cause_kind,
            cause_detail: hook_failure_detail(&failure.cause),
            duration_ms: hook_duration_ms(failure.duration),
            fail_closed_denied,
            at: harness_contracts::now(),
        }));
        match &failure.cause {
            HookFailureCause::Unsupported {
                kind: returned_kind,
            } => events.push(Event::HookReturnedUnsupported(
                harness_contracts::HookReturnedUnsupportedEvent {
                    hook_event_kind: kind.clone(),
                    handler_id: failure.handler_id.clone(),
                    returned_kind: returned_kind.clone(),
                    causation_id,
                    at: harness_contracts::now(),
                },
            )),
            HookFailureCause::Inconsistent { reason } => {
                events.push(Event::HookOutcomeInconsistent(
                    harness_contracts::HookOutcomeInconsistentEvent {
                        hook_event_kind: kind.clone(),
                        handler_id: failure.handler_id.clone(),
                        reason: reason.clone(),
                        causation_id,
                        at: harness_contracts::now(),
                    },
                ));
            }
            _ => {}
        }
    }
    events
}

fn hook_outcome_summary(outcome: &HookOutcome) -> harness_contracts::HookOutcomeSummary {
    match outcome {
        HookOutcome::Continue => harness_contracts::HookOutcomeSummary {
            continued: true,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::Block { reason } => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: Some(reason.clone()),
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::PreToolUse(outcome) => harness_contracts::HookOutcomeSummary {
            continued: outcome.is_continue(),
            blocked_reason: outcome.block.clone(),
            rewrote_input: outcome.rewrite_input.is_some(),
            overrode_permission: outcome.override_permission.clone(),
            added_context_bytes: outcome
                .additional_context
                .as_ref()
                .map(|context| context.content.len() as u64),
            transformed: false,
        },
        HookOutcome::RewriteInput(_) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: true,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::OverridePermission(decision) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: Some(decision.clone()),
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::AddContext(context) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: Some(context.content.len() as u64),
            transformed: false,
        },
        HookOutcome::Transform(_) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: true,
        },
    }
}

fn hook_failure_detail(cause: &HookFailureCause) -> String {
    match cause {
        HookFailureCause::Unsupported { kind } => format!("unsupported outcome: {kind:?}"),
        HookFailureCause::Inconsistent { reason } => format!("inconsistent outcome: {reason:?}"),
        HookFailureCause::Panicked { snippet } => snippet.clone(),
        HookFailureCause::Timeout => "timeout".to_owned(),
        HookFailureCause::Transport { kind, detail } => format!("{kind:?}: {detail}"),
        HookFailureCause::Unauthorized { capability } => format!("unauthorized: {capability}"),
    }
}

fn hook_duration_ms(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
