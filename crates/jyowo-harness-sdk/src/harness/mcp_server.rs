use super::session_runtime::{
    conversation_run_options_hash, run_model_snapshot, session_options_for_run,
};
use super::*;

#[cfg(feature = "mcp-server-adapter")]
struct McpSessionReplay {
    projection: SessionProjection,
    created_options_hash: [u8; 32],
}

#[async_trait]
impl HarnessMcpBackend for Harness {
    async fn call_harness_tool(
        &self,
        context: &McpServerRequestContext,
        capability: ExposedCapability,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        match capability {
            ExposedCapability::SessionsList => self.mcp_sessions_list(context, arguments).await,
            ExposedCapability::SessionGet => self.mcp_session_get(context, arguments).await,
            ExposedCapability::MessagesRead => self.mcp_messages_read(context, arguments).await,
            ExposedCapability::MessagesSend => self.mcp_messages_send(context, arguments).await,
            ExposedCapability::AttachmentsFetch => {
                self.mcp_attachments_fetch(context, arguments).await
            }
            ExposedCapability::EventsPoll => self.mcp_events_poll(context, arguments).await,
            ExposedCapability::EventsWait => self.mcp_events_wait(context, arguments).await,
            ExposedCapability::PermissionsListOpen => {
                self.mcp_permissions_list_open(context, arguments).await
            }
            ExposedCapability::PermissionsRespond => {
                self.mcp_permissions_respond(context, arguments).await
            }
            ExposedCapability::ChannelsList => Ok(json!({ "count": 0, "channels": [] })),
            _ => Err(McpServerError::InvalidParams(
                "unsupported harness MCP capability".to_owned(),
            )),
        }
    }
}

#[cfg(feature = "mcp-server-adapter")]
impl Harness {
    async fn mcp_sessions_list(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: SessionsListArgs = mcp_args(arguments)?;
        let mut sessions = self
            .inner
            .event_store
            .list_sessions(
                context.tenant_id,
                SessionFilter {
                    since: args.since,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: args.limit(),
                },
            )
            .await
            .map_err(mcp_journal_error)?;
        if !args.include_ended {
            sessions.retain(|session| session.end_reason.is_none());
        }
        Ok(json!({
            "count": sessions.len(),
            "sessions": sessions,
        }))
    }

    async fn mcp_session_get(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: SessionGetArgs = mcp_args(arguments)?;
        let session_id = parse_session_id(&args.session_id)?;
        let projection = self
            .read_session_projection(context.tenant_id, session_id)
            .await?;
        Ok(json!({
            "session": {
                "session_id": projection.session_id,
                "tenant_id": projection.tenant_id,
                "message_count": projection.messages.len(),
                "permission_count": projection.permission_log.len(),
                "tool_use_count": projection.tool_uses.len(),
                "end_reason": projection.end_reason,
                "last_offset": projection.last_offset,
                "snapshot_id": projection.snapshot_id,
                "usage": projection.usage,
            }
        }))
    }

    async fn mcp_messages_read(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: MessagesReadArgs = mcp_args(arguments)?;
        let session_id = parse_session_id(&args.session_id)?;
        let projection = self
            .read_session_projection(context.tenant_id, session_id)
            .await?;
        let offset = args.offset.unwrap_or(0);
        let limit = args.limit();
        let messages = projection
            .messages
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        Ok(json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "count": messages.len(),
            "messages": messages,
        }))
    }

    async fn mcp_messages_send(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: MessagesSendArgs = mcp_args(arguments)?;
        let session_id = parse_session_id(&args.session_id)?;
        let replay = self
            .read_session_replay(context.tenant_id, session_id)
            .await?;
        let projection = replay.projection;
        if projection.end_reason.is_some() {
            return Err(McpServerError::InvalidParams(
                "cannot send message to ended session".to_owned(),
            ));
        }
        let session = self
            .resume_session_from_projection(
                context.tenant_id,
                session_id,
                projection,
                replay.created_options_hash,
            )
            .await?;
        session
            .run_turn(args.message)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let projection = session.projection().await;
        Ok(json!({
            "session_id": projection.session_id,
            "message_count": projection.messages.len(),
            "last_offset": projection.last_offset,
            "snapshot_id": projection.snapshot_id,
        }))
    }

    async fn mcp_attachments_fetch(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: AttachmentsFetchArgs = mcp_args(arguments)?;
        let Some(blob_store) = &self.inner.blob_store else {
            return Err(McpServerError::InvalidParams(
                "blob store is not configured".to_owned(),
            ));
        };
        const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
        let meta = blob_store
            .head(context.tenant_id, &args.blob_ref)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?
            .ok_or_else(|| McpServerError::InvalidParams("blob not found".to_owned()))?;
        if meta.size as usize > MAX_ATTACHMENT_BYTES {
            return Err(McpServerError::InvalidParams(format!(
                "blob exceeds MCP attachment fetch limit: {} > {}",
                meta.size, MAX_ATTACHMENT_BYTES
            )));
        }
        let mut bytes = Vec::new();
        let chunks = blob_store
            .get(context.tenant_id, &args.blob_ref)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?
            .collect::<Vec<_>>()
            .await;
        for chunk in chunks {
            if bytes.len() + chunk.len() > MAX_ATTACHMENT_BYTES {
                return Err(McpServerError::InvalidParams(format!(
                    "blob exceeds MCP attachment fetch limit: > {MAX_ATTACHMENT_BYTES}"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(json!({
            "blob_ref": args.blob_ref,
            "meta": meta,
            "content_base64": BASE64_STANDARD.encode(bytes),
        }))
    }

    async fn mcp_events_poll(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: EventsPollArgs = mcp_args(arguments)?;
        self.poll_events(context.tenant_id, args).await
    }

    async fn mcp_events_wait(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: EventsWaitArgs = mcp_args(arguments)?;
        let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(30_000).min(300_000));
        let started = std::time::Instant::now();
        loop {
            let result = self
                .poll_events(
                    context.tenant_id,
                    EventsPollArgs {
                        after_event_id: args.after_event_id.clone(),
                        session_id: args.session_id.clone(),
                        limit: args.limit,
                    },
                )
                .await?;
            if result["count"].as_u64().unwrap_or(0) > 0 || started.elapsed() >= timeout {
                return Ok(result);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(result);
            }
            tokio::time::sleep(Duration::from_millis(200).min(remaining)).await;
        }
    }

    #[cfg(feature = "stream-permission")]
    async fn mcp_permissions_list_open(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: PermissionsListOpenArgs = mcp_args(arguments)?;
        let limit = args.limit();
        let mut permissions = Vec::new();
        let session_id_filter = args
            .session_id
            .as_deref()
            .map(parse_session_id)
            .transpose()?;
        append_pending_stream_permissions(
            &mut permissions,
            self.inner.permission_resolver.as_ref(),
            context.tenant_id,
            session_id_filter,
            limit,
        );
        if permissions.len() < limit {
            if let Some(session_id) = session_id_filter {
                let projection = self
                    .read_session_projection(context.tenant_id, session_id)
                    .await?;
                permissions.extend(open_permissions(projection, limit - permissions.len()));
            } else {
                let sessions = self
                    .inner
                    .event_store
                    .list_sessions(
                        context.tenant_id,
                        SessionFilter {
                            since: None,
                            end_reason: None,
                            project_compression_tips: false,
                            limit: limit as u32,
                        },
                    )
                    .await
                    .map_err(mcp_journal_error)?;
                for summary in sessions {
                    if permissions.len() >= limit {
                        break;
                    }
                    let projection = self
                        .read_session_projection(context.tenant_id, summary.session_id)
                        .await?;
                    permissions.extend(open_permissions(projection, limit - permissions.len()));
                }
            }
        }
        Ok(json!({
            "count": permissions.len(),
            "permissions": permissions,
        }))
    }

    #[cfg(not(feature = "stream-permission"))]
    async fn mcp_permissions_list_open(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: PermissionsListOpenArgs = mcp_args(arguments)?;
        let limit = args.limit();
        let mut permissions = Vec::new();
        if let Some(session_id) = args.session_id {
            let projection = self
                .read_session_projection(context.tenant_id, parse_session_id(&session_id)?)
                .await?;
            permissions.extend(open_permissions(projection, limit));
        } else {
            let sessions = self
                .inner
                .event_store
                .list_sessions(
                    context.tenant_id,
                    SessionFilter {
                        since: None,
                        end_reason: None,
                        project_compression_tips: false,
                        limit: limit as u32,
                    },
                )
                .await
                .map_err(mcp_journal_error)?;
            for summary in sessions {
                if permissions.len() >= limit {
                    break;
                }
                let projection = self
                    .read_session_projection(context.tenant_id, summary.session_id)
                    .await?;
                permissions.extend(open_permissions(projection, limit - permissions.len()));
            }
        }
        Ok(json!({
            "count": permissions.len(),
            "permissions": permissions,
        }))
    }

    async fn mcp_permissions_respond(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: PermissionsRespondArgs = mcp_args(arguments)?;
        validate_mcp_permission_decision(args.decision.clone())?;
        let session_id = parse_session_id(&args.session_id)?;
        let request_id = parse_request_id(&args.request_id)?;
        let option_id = parse_permission_option_id(&args.option_id)?;
        let resolver = self.inner.permission_resolver.as_ref().ok_or_else(|| {
            McpServerError::Internal("permission resolver is not configured".to_owned())
        })?;
        let pending = resolver
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.request_id == request_id)
            .ok_or_else(|| {
                McpServerError::InvalidParams("permission request is not pending".to_owned())
            })?;
        if pending.request.tenant_id != context.tenant_id
            || pending.request.session_id != session_id
        {
            return Err(McpServerError::InvalidParams(
                "permission request is not pending for this session".to_owned(),
            ));
        }
        resolver
            .resolve_option_for(
                request_id,
                context.tenant_id,
                session_id,
                option_id,
                args.decision,
                args.confirmation_text.as_deref(),
            )
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        Ok(json!({ "resolved": true }))
    }

    async fn poll_events(
        &self,
        tenant_id: TenantId,
        args: EventsPollArgs,
    ) -> Result<Value, McpServerError> {
        let limit = args.limit();
        let after_event_id = args
            .after_event_id
            .as_deref()
            .map(parse_event_id)
            .transpose()?;
        let envelopes = if let Some(session_id) = args.session_id {
            let session_id = parse_session_id(&session_id)?;
            let mut envelopes = self
                .inner
                .event_store
                .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
                .await
                .map_err(mcp_journal_error)?
                .collect::<Vec<_>>()
                .await;
            if let Some(after) = after_event_id {
                match envelopes
                    .iter()
                    .position(|envelope| envelope.event_id == after)
                {
                    Some(position) => envelopes.drain(0..=position).for_each(drop),
                    None => envelopes.clear(),
                }
            }
            envelopes.truncate(limit);
            envelopes
        } else {
            self.inner
                .event_store
                .query_after(tenant_id, after_event_id, limit)
                .await
                .map_err(mcp_journal_error)?
        };
        let next_event_id = envelopes
            .last()
            .map(|envelope| envelope.event_id.to_string());
        Ok(json!({
            "count": envelopes.len(),
            "next_event_id": next_event_id,
            "events": envelopes,
        }))
    }

    async fn read_session_projection(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<SessionProjection, McpServerError> {
        Ok(self
            .read_session_replay(tenant_id, session_id)
            .await?
            .projection)
    }

    async fn read_session_replay(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<McpSessionReplay, McpServerError> {
        let envelopes = self
            .inner
            .event_store
            .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
            .await
            .map_err(mcp_journal_error)?
            .collect::<Vec<_>>()
            .await;
        if envelopes.is_empty() {
            return Err(McpServerError::InvalidParams(format!(
                "session not found: {session_id}"
            )));
        }
        let Some(Event::SessionCreated(created)) =
            envelopes.first().map(|envelope| &envelope.payload)
        else {
            return Err(McpServerError::Internal(
                "session event stream does not start with SessionCreated".to_owned(),
            ));
        };
        if created.tenant_id != tenant_id || created.session_id != session_id {
            return Err(McpServerError::InvalidParams(format!(
                "session not found: {session_id}"
            )));
        }
        let created_options_hash = created.options_hash;
        let projection = SessionProjection::replay(envelopes)
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        Ok(McpSessionReplay {
            projection,
            created_options_hash,
        })
    }

    fn mcp_resume_options_for_hash(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        created_options_hash: [u8; 32],
    ) -> Result<SessionOptions, McpServerError> {
        let mut candidates = Vec::new();
        let mut default_options = self.inner.options.default_session_options.clone();
        default_options.workspace_root = self.inner.options.workspace_root.clone();
        default_options.tenant_id = tenant_id;
        default_options.session_id = session_id;
        candidates.push(default_options);

        for workspace in self.inner.workspace_registry.list(tenant_id) {
            let explicit = SessionOptions::default()
                .with_tenant_id(tenant_id)
                .with_workspace(workspace.id)
                .with_session_id(session_id);
            let options = self
                .effective_session_options(explicit)
                .map_err(|error| McpServerError::Internal(error.to_string()))?;
            candidates.push(options);
        }

        for mut candidate in candidates {
            candidate.workspace_root =
                candidate.workspace_root.canonicalize().map_err(|error| {
                    McpServerError::Internal(format!("workspace_root invalid: {error}"))
                })?;
            if self.session_options_hash_matches(&candidate, created_options_hash) {
                return Ok(candidate);
            }
        }

        Err(McpServerError::InvalidParams(
            "session options do not match a resumable workspace context".to_owned(),
        ))
    }

    async fn resume_session_from_projection(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        projection: SessionProjection,
        created_options_hash: [u8; 32],
    ) -> Result<Session, McpServerError> {
        let options =
            self.mcp_resume_options_for_hash(tenant_id, session_id, created_options_hash)?;
        self.enforce_tenant(&options)
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let prompt_inputs = self
            .load_effective_prompt_inputs(&options)
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let prompt_inputs_hash = effective_prompt_inputs_hash(&prompt_inputs);
        let run_options = ConversationRunOptions::from_session_options(&options);
        let run_options_hash = conversation_run_options_hash(&run_options);
        let limit_permit = self
            .inner
            .session_limits
            .try_acquire()
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self
            .memory_manager_for_session(&options)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        #[cfg(feature = "memory-external-slot")]
        let session_engine = self
            .engine_for_session(
                &options,
                &run_options,
                &prompt_inputs,
                memory_manager.clone(),
                None,
                #[cfg(feature = "agents-subagent")]
                None,
            )
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        #[cfg(not(feature = "memory-external-slot"))]
        let session_engine = self
            .engine_for_session(
                &options,
                &run_options,
                &prompt_inputs,
                None,
                #[cfg(feature = "agents-subagent")]
                None,
            )
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let run_model = run_model_snapshot(&session_engine.model_snapshot, &run_options);
        let effective_config_hash = run_effective_config_hash(
            session_options_hash(&options),
            run_options_hash,
            Some(prompt_inputs_hash),
            Some(session_engine.runtime_prompt_context_hash),
        );
        let turn_options = session_options_for_run(options, &run_options);
        let event_store: Arc<dyn EventStore> = Arc::new(LifecycleHookEventStore {
            inner: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: turn_options.tenant_id,
            session_id: turn_options.session_id,
            #[cfg(feature = "memory-external-slot")]
            user_id: turn_options.user_id.clone(),
            #[cfg(feature = "memory-external-slot")]
            team_id: turn_options.team_id,
            workspace_root: turn_options.workspace_root.clone(),
            redactor: self.hook_redactor(),
            session_limits: Arc::clone(&self.inner.session_limits),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            evidence_ref_store: self.inner.evidence_ref_store.clone(),
            summary_state: parking_lot::Mutex::new(MemorySessionSummaryState::default()),
            #[cfg(feature = "memory-external-slot")]
            memory_manager,
        });
        let session = Session::builder()
            .with_options(turn_options)
            .with_effective_prompt_inputs_hash(prompt_inputs_hash)
            .with_runtime_prompt_context_hash(session_engine.runtime_prompt_context_hash)
            .with_effective_config_hash(effective_config_hash)
            .with_turn_model_snapshot(run_model)
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
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        limit_permit.disarm();
        Ok(session)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionsListArgs {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    since: Option<DateTime<chrono::Utc>>,
    #[serde(default = "default_true")]
    include_ended: bool,
}

#[cfg(feature = "mcp-server-adapter")]
impl SessionsListArgs {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionGetArgs {
    session_id: String,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesReadArgs {
    session_id: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[cfg(feature = "mcp-server-adapter")]
impl MessagesReadArgs {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesSendArgs {
    session_id: String,
    message: String,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentsFetchArgs {
    blob_ref: BlobRef,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsPollArgs {
    #[serde(default)]
    after_event_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[cfg(feature = "mcp-server-adapter")]
impl EventsPollArgs {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(20).clamp(1, 500)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsWaitArgs {
    #[serde(default)]
    after_event_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionsListOpenArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[cfg(feature = "mcp-server-adapter")]
impl PermissionsListOpenArgs {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionsRespondArgs {
    session_id: String,
    request_id: String,
    option_id: String,
    decision: Decision,
    #[serde(default)]
    confirmation_text: Option<String>,
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_permission_option_id(
    value: &str,
) -> Result<harness_contracts::PermissionOptionId, McpServerError> {
    let option_id = harness_contracts::PermissionOptionId::parse(value).map_err(|error| {
        McpServerError::InvalidParams(format!(
            "option_id must be a valid permission option id: {error}"
        ))
    })?;
    if option_id.to_string() != value {
        return Err(McpServerError::InvalidParams(
            "option_id must be a canonical permission option id".to_owned(),
        ));
    }
    Ok(option_id)
}

#[cfg(feature = "mcp-server-adapter")]
fn validate_mcp_permission_decision(decision: Decision) -> Result<(), McpServerError> {
    match decision {
        Decision::AllowOnce | Decision::DenyOnce => Ok(()),
        Decision::Escalate => Err(McpServerError::InvalidParams(
            "permissions_respond requires a backend-issued allow or deny option".to_owned(),
        )),
        Decision::AllowSession | Decision::AllowPermanent | Decision::DenyPermanent => {
            Err(McpServerError::InvalidParams(
                "permissions_respond only accepts allow_once or deny_once".to_owned(),
            ))
        }
        _ => Err(McpServerError::InvalidParams(
            "permissions_respond received an unsupported decision".to_owned(),
        )),
    }
}

fn default_true() -> bool {
    true
}

#[cfg(feature = "mcp-server-adapter")]
fn mcp_args<T>(arguments: Value) -> Result<T, McpServerError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(arguments)
        .map_err(|error| McpServerError::InvalidParams(error.to_string()))
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_session_id(value: &str) -> Result<harness_contracts::SessionId, McpServerError> {
    value
        .parse()
        .map_err(|error| McpServerError::InvalidParams(format!("invalid session_id: {error}")))
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_event_id(value: &str) -> Result<EventId, McpServerError> {
    value
        .parse()
        .map_err(|error| McpServerError::InvalidParams(format!("invalid event_id: {error}")))
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_request_id(value: &str) -> Result<harness_contracts::RequestId, McpServerError> {
    value
        .parse()
        .map_err(|error| McpServerError::InvalidParams(format!("invalid request_id: {error}")))
}

#[cfg(feature = "mcp-server-adapter")]
fn mcp_journal_error(error: harness_contracts::JournalError) -> McpServerError {
    McpServerError::Internal(error.to_string())
}

#[cfg(all(feature = "mcp-server-adapter", feature = "stream-permission"))]
fn append_pending_stream_permissions(
    permissions: &mut Vec<harness_session::PermissionRecord>,
    resolver: Option<&ResolverHandle>,
    tenant_id: TenantId,
    session_id: Option<harness_contracts::SessionId>,
    limit: usize,
) {
    let Some(resolver) = resolver else {
        return;
    };

    let remaining = limit.saturating_sub(permissions.len());
    if remaining == 0 {
        return;
    }

    permissions.extend(
        resolver
            .pending_permission_requests()
            .into_iter()
            .filter(|pending| pending.request.tenant_id == tenant_id)
            .filter(|pending| {
                session_id
                    .map(|session_id| pending.request.session_id == session_id)
                    .unwrap_or(true)
            })
            .take(remaining)
            .map(permission_pending_request_to_record),
    );
}

#[cfg(all(feature = "mcp-server-adapter", feature = "stream-permission"))]
fn permission_pending_request_to_record(
    pending: PendingPermissionRequest,
) -> harness_session::PermissionRecord {
    harness_session::PermissionRecord {
        request_id: pending.request.request_id,
        tool_use_id: pending.request.tool_use_id,
        tool_name: pending.request.tool_name,
        subject: pending.request.subject,
        decision: None,
        scope: pending.request.scope_hint,
        decision_options: pending.decision_options,
    }
}

#[cfg(feature = "mcp-server-adapter")]
fn open_permissions(
    projection: SessionProjection,
    limit: usize,
) -> Vec<harness_session::PermissionRecord> {
    projection
        .permission_log
        .into_iter()
        .filter(|record| record.decision.is_none())
        .take(limit)
        .collect()
}
