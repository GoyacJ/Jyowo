#[cfg(feature = "memory-provider-registry")]
use super::memory::memory_actor_from_options;
use super::*;

struct HydratedConversationInput {
    input: ConversationTurnInput,
    memory_patches: Vec<HydratedMemoryPatch>,
    skill_patches: Vec<HydratedSkillPatch>,
    skill_turn_snapshot: Option<SkillTurnSnapshot>,
}

struct HydratedMemoryPatch {
    memory_id: MemoryId,
    provider_id: String,
    fence: String,
}

struct HydratedSkillPatch {
    delivery_key: String,
    skill_id: SkillId,
    skill_name: String,
    body: String,
    stage: SkillContextDeliveryStage,
}

fn render_conversation_turn_prompt(
    input: &ConversationTurnInput,
    supported_modalities: &[ModelModality],
) -> String {
    let text_attachments = input
        .attachments
        .iter()
        .filter(|attachment| {
            !is_image_attachment(attachment)
                && !is_video_attachment(attachment)
                && !(supports_file_input(supported_modalities) && is_file_attachment(attachment))
        })
        .collect::<Vec<_>>();
    if input.context_references.is_empty() && text_attachments.is_empty() {
        return input.prompt.clone();
    }

    let mut lines = vec!["<conversation-context>".to_owned()];

    if !input.context_references.is_empty() {
        lines.push("references:".to_owned());
        lines.extend(
            input
                .context_references
                .iter()
                .map(render_context_reference)
                .filter(|line| !line.is_empty()),
        );
    }

    if !text_attachments.is_empty() {
        lines.push("attachments:".to_owned());
        lines.extend(
            text_attachments
                .into_iter()
                .map(render_attachment_reference),
        );
    }

    lines.push("</conversation-context>".to_owned());
    lines.push(input.prompt.clone());
    lines.join("\n")
}

fn conversation_turn_parts(
    input: &ConversationTurnInput,
    supported_modalities: &[ModelModality],
) -> Vec<MessagePart> {
    let mut parts = vec![MessagePart::Text(render_conversation_turn_prompt(
        input,
        supported_modalities,
    ))];
    parts.extend(input.attachments.iter().filter_map(|attachment| {
        if is_image_attachment(attachment) {
            Some(MessagePart::Image {
                mime_type: attachment.mime_type.clone(),
                blob_ref: attachment.blob_ref.clone(),
            })
        } else if is_video_attachment(attachment) {
            Some(MessagePart::Video {
                mime_type: attachment.mime_type.clone(),
                blob_ref: attachment.blob_ref.clone(),
            })
        } else if supports_file_input(supported_modalities) && is_file_attachment(attachment) {
            Some(MessagePart::File {
                mime_type: attachment.mime_type.clone(),
                blob_ref: attachment.blob_ref.clone(),
            })
        } else {
            None
        }
    }));
    parts
}

fn is_image_attachment(attachment: &ConversationAttachmentReference) -> bool {
    attachment.mime_type.starts_with("image/")
}

fn is_video_attachment(attachment: &ConversationAttachmentReference) -> bool {
    attachment.mime_type.starts_with("video/")
}

fn is_file_attachment(attachment: &ConversationAttachmentReference) -> bool {
    !is_image_attachment(attachment) && !is_video_attachment(attachment)
}

fn supports_file_input(supported_modalities: &[ModelModality]) -> bool {
    supported_modalities.contains(&ModelModality::File)
}

fn render_context_reference(reference: &ConversationContextReference) -> String {
    match reference {
        ConversationContextReference::WorkspaceFile { path, label } => {
            format!(
                "- workspace_file: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(path)
            )
        }
        ConversationContextReference::Artifact { id, label } => {
            format!(
                "- artifact: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Conversation { id, label } => {
            format!(
                "- conversation: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Memory {
            resolved_content, ..
        } => {
            if let Some(content) = resolved_content {
                content.clone()
            } else {
                String::new()
            }
        }
        ConversationContextReference::Skill { .. } => String::new(),
        ConversationContextReference::Tool { id, label } => {
            format!(
                "- tool: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::McpServer { id, label } => {
            format!(
                "- mcp_server: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
    }
}

fn render_attachment_reference(attachment: &ConversationAttachmentReference) -> String {
    format!(
        "- attachment: {} {} {} bytes {}",
        sanitize_context_line(&attachment.name),
        sanitize_context_line(&attachment.mime_type),
        attachment.size_bytes,
        sanitize_context_line(&attachment.id)
    )
}

fn sanitize_context_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fallback_skill_context_delivery_key(
    session_id: SessionId,
    client_message_id: Option<&str>,
    run_id: RunId,
    reference_index: usize,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"jyowo.sdk.skill-context-delivery.v1\0");
    hasher.update(&session_id.as_bytes());
    match client_message_id {
        Some(client_message_id) => hasher.update(client_message_id.as_bytes()),
        None => hasher.update(&run_id.as_bytes()),
    };
    hasher.update(&(reference_index as u64).to_be_bytes());
    format!("sdk-skill-context-v1:{}", hasher.finalize().to_hex())
}

fn selected_skill_render_error(skill_id: &SkillId, error: RenderError) -> HarnessError {
    let error = match error {
        RenderError::MissingParam(parameter) => SkillContextError::MissingParameter {
            skill_id: skill_id.0.clone(),
            parameter,
        },
        RenderError::InvalidParam { name, expected } => SkillContextError::InvalidParameter {
            skill_id: skill_id.0.clone(),
            parameter: name,
            expected: expected.to_owned(),
        },
        RenderError::SkillNotVisible(_) => SkillContextError::NotVisible {
            skill_id: skill_id.0.clone(),
        },
        RenderError::MissingConfig { config_keys, .. } => SkillContextError::MissingConfig {
            skill_id: skill_id.0.clone(),
            config_keys,
        },
        RenderError::UnknownConfigKey(_) | RenderError::ConfigResolve(_) => {
            SkillContextError::RenderFailed {
                skill_id: skill_id.0.clone(),
                reason: "configuration resolution failed".to_owned(),
            }
        }
        RenderError::ShellNotAllowed(_) | RenderError::ShellExec(_) => {
            SkillContextError::RenderFailed {
                skill_id: skill_id.0.clone(),
                reason: "render policy rejected selected skill".to_owned(),
            }
        }
    };
    error.into()
}

impl Harness {
    async fn hydrate_memory_references(
        &self,
        mut input: ConversationTurnInput,
        options: &SessionOptions,
        projection: &SessionProjection,
        run_id: RunId,
        delivery_keys: &[Option<String>],
    ) -> Result<HydratedConversationInput, HarnessError> {
        #[cfg(not(feature = "memory-provider-registry"))]
        let _ = options;
        #[cfg(feature = "memory-provider-registry")]
        let mut memory_patches = Vec::new();
        #[cfg(not(feature = "memory-provider-registry"))]
        let memory_patches = Vec::new();
        #[cfg(feature = "memory-provider-registry")]
        let resolver = {
            let harness = self.clone();
            let options = options.clone();
            harness_memory::FnMemoryResolver::new(move |memory_id| {
                let harness = harness.clone();
                let options = options.clone();
                async move {
                    let manager =
                        harness
                            .memory_manager_for_browser(&options)
                            .await
                            .map_err(|error| match error {
                                HarnessError::Memory(error) => error,
                                other => harness_contracts::MemoryError::Message(other.to_string()),
                            })?;
                    manager
                        .get_for_actor_with_provider(memory_id, memory_actor_from_options(&options))
                        .await
                        .map(|source| (source.record.content, source.provider_id))
                }
            })
        };

        for reference in &mut input.context_references {
            let ConversationContextReference::Memory {
                resolved_content, ..
            } = reference
            else {
                continue;
            };
            *resolved_content = None;

            #[cfg(feature = "memory-provider-registry")]
            {
                let ConversationContextReference::Memory {
                    id: memory_reference_id,
                    label: memory_reference_label,
                    ..
                } = reference
                else {
                    continue;
                };
                let redactor = self.hook_redactor();
                let redact_rules = RedactRules {
                    scope: RedactScope::All,
                    ..RedactRules::default()
                };
                let memory_id = MemoryId::parse(memory_reference_id).map_err(|error| {
                    HarnessError::Memory(harness_contracts::MemoryError::Message(format!(
                        "invalid memory reference id: {memory_reference_id}: {error}"
                    )))
                })?;
                let resolved = harness_memory::ContextReferenceResolver::resolve_memory(
                    &resolver,
                    memory_id,
                    memory_reference_label.clone(),
                )
                .await
                .map_err(HarnessError::Memory)?;
                match resolved.outcome {
                    harness_memory::MemoryReferenceOutcome::Hydrated {
                        content,
                        provider_id,
                    } => {
                        let redacted = redactor.redact(&content, &redact_rules);
                        memory_patches.push(HydratedMemoryPatch {
                            memory_id,
                            fence: harness_memory::fence_memory_content(
                                &redacted,
                                memory_id,
                                &provider_id,
                            ),
                            provider_id,
                        });
                    }
                    harness_memory::MemoryReferenceOutcome::Failed { reason } => {
                        return Err(HarnessError::Memory(
                            harness_contracts::MemoryError::Message(format!(
                                "memory reference could not be resolved: {reason}"
                            )),
                        ));
                    }
                }
            }

            #[cfg(not(feature = "memory-provider-registry"))]
            {
                return Err(HarnessError::Memory(
                    harness_contracts::MemoryError::ExternalProviderNotConfigured,
                ));
            }
        }

        let skill_turn_snapshot = input
            .context_references
            .iter()
            .any(|reference| matches!(reference, ConversationContextReference::Skill { .. }))
            .then(|| self.capture_skill_turn_snapshot(options, None));
        let skill_turn_snapshot = match skill_turn_snapshot {
            Some(snapshot) => Some(snapshot.await?),
            None => None,
        };
        let mut skill_patches = Vec::new();
        if let Some(snapshot) = &skill_turn_snapshot {
            let service = snapshot.service();
            let agent = AgentId::from_u128(1);
            for (reference_index, requested_reference) in
                input.context_references.iter().enumerate()
            {
                if !matches!(
                    requested_reference,
                    ConversationContextReference::Skill { .. }
                ) {
                    continue;
                }
                let delivery_key = delivery_keys
                    .get(reference_index)
                    .and_then(Clone::clone)
                    .unwrap_or_else(|| {
                        fallback_skill_context_delivery_key(
                            options.session_id,
                            input.client_message_id.as_deref(),
                            run_id,
                            reference_index,
                        )
                    });
                let existing = projection.skill_context_delivery(&delivery_key);
                if existing
                    .is_some_and(|delivery| delivery.stage == SkillContextDeliveryStage::Consumed)
                {
                    continue;
                }
                let reference = existing
                    .map(|delivery| delivery.reference.clone())
                    .unwrap_or_else(|| requested_reference.clone());
                let ConversationContextReference::Skill {
                    version,
                    skill_id,
                    label,
                    parameters,
                    source,
                } = reference
                else {
                    return Err(SkillContextError::InvalidPersistedReference.into());
                };
                let skill = snapshot
                    .registry_snapshot
                    .entries
                    .values()
                    .find(|skill| skill.id == skill_id)
                    .cloned()
                    .ok_or_else(|| SkillContextError::Unavailable {
                        skill_id: skill_id.0.clone(),
                    })?;
                let actual_source = skill.source.to_kind();
                if source
                    .as_ref()
                    .is_some_and(|source| source != &actual_source)
                {
                    return Err(SkillContextError::SourceMismatch {
                        skill_id: skill_id.0.clone(),
                    }
                    .into());
                }
                let normalized_reference = ConversationContextReference::Skill {
                    version,
                    skill_id: skill_id.clone(),
                    label,
                    parameters: parameters.clone(),
                    source: Some(actual_source),
                };
                if let Some(existing) = existing {
                    if existing.reference != normalized_reference {
                        return Err(SkillContextError::ReferenceMismatch {
                            delivery_key: delivery_key.clone(),
                        }
                        .into());
                    }
                }
                for parameter in parameters.keys() {
                    if !skill
                        .frontmatter
                        .parameters
                        .iter()
                        .any(|declaration| declaration.name == *parameter)
                    {
                        return Err(SkillContextError::UnknownParameter {
                            skill_id: skill_id.0.clone(),
                            parameter: parameter.clone(),
                        }
                        .into());
                    }
                }
                if service.view(&agent, &skill.name, false).is_none() {
                    return Err(SkillContextError::NotVisible {
                        skill_id: skill_id.0.clone(),
                    }
                    .into());
                }
                let rendered = service
                    .render(
                        &agent,
                        &skill.name,
                        Value::Object(parameters.into_iter().collect()),
                    )
                    .await
                    .map_err(|error| selected_skill_render_error(&skill_id, error))?;
                let body_hash = ContentHash(*blake3::hash(rendered.content.as_bytes()).as_bytes());
                let stage = existing
                    .map(|delivery| delivery.stage)
                    .unwrap_or(SkillContextDeliveryStage::Prepared);
                if let Some(existing) = existing {
                    if existing.body_hash != body_hash {
                        return Err(SkillContextError::IntegrityMismatch {
                            delivery_key: delivery_key.clone(),
                        }
                        .into());
                    }
                } else {
                    self.inner
                        .event_store
                        .append(
                            options.tenant_id,
                            options.session_id,
                            &[Event::SkillContextPrepared(SkillContextPreparedEvent {
                                session_id: options.session_id,
                                run_id,
                                delivery_key: delivery_key.clone(),
                                reference: normalized_reference,
                                body_hash,
                                at: harness_contracts::now(),
                            })],
                        )
                        .await
                        .map_err(HarnessError::Journal)?;
                }
                skill_patches.push(HydratedSkillPatch {
                    delivery_key,
                    skill_id,
                    skill_name: skill.name.clone(),
                    body: rendered.content,
                    stage,
                });
            }
        }

        input.context_references.retain(|reference| {
            !matches!(
                reference,
                ConversationContextReference::Memory { .. }
                    | ConversationContextReference::Skill { .. }
            )
        });

        Ok(HydratedConversationInput {
            input,
            memory_patches,
            skill_patches,
            skill_turn_snapshot,
        })
    }

    pub async fn open_or_create_conversation_session(
        &self,
        options: SessionOptions,
    ) -> Result<ConversationSession, HarnessError> {
        let effective = self.effective_sdk_session_options(options.clone())?;
        self.ensure_conversation_session_not_deleted(effective.tenant_id, effective.session_id)?;
        match self.read_sdk_session_state(&effective).await? {
            Some(state) => Ok(ConversationSession {
                tenant_id: state.projection.tenant_id,
                session_id: state.projection.session_id,
                message_count: state.projection.messages.len(),
            }),
            None => {
                let session = self.create_session(options).await?;
                let projection = session.projection().await;
                Ok(ConversationSession {
                    tenant_id: projection.tenant_id,
                    session_id: projection.session_id,
                    message_count: projection.messages.len(),
                })
            }
        }
    }

    pub async fn list_conversation_sessions(
        &self,
        tenant_id: TenantId,
        limit: u32,
    ) -> Result<Vec<ConversationSessionSummary>, HarnessError> {
        let sessions = self
            .inner
            .event_store
            .list_sessions(
                tenant_id,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit,
                },
            )
            .await
            .map_err(HarnessError::Journal)?;

        let mut conversation_sessions = Vec::new();
        for session in sessions {
            if session.end_reason.is_some() {
                continue;
            }
            if self
                .is_conversation_session_stream(tenant_id, session.session_id)
                .await?
            {
                conversation_sessions.push(session);
            }
        }
        conversation_sessions.sort_by_key(|session| session.last_event_at);
        conversation_sessions.reverse();

        Ok(conversation_sessions
            .into_iter()
            .map(|session| ConversationSessionSummary {
                session_id: session.session_id,
                created_at: session.created_at,
                last_event_at: session.last_event_at,
                event_count: session.event_count,
            })
            .collect())
    }

    pub async fn delete_conversation_session(
        &self,
        options: SessionOptions,
    ) -> Result<bool, HarnessError> {
        let options = self.effective_sdk_session_options(options)?;
        let journal_session_exists = self
            .conversation_session_has_journal_events(options.tenant_id, options.session_id)
            .await?;
        if !journal_session_exists {
            return Ok(false);
        }

        if let Some(store) = &self.inner.provider_continuation_store {
            store
                .prune_session(options.tenant_id, options.session_id)
                .await
                .map_err(|_| {
                    HarnessError::Internal("provider continuation pruning failed".to_owned())
                })?;
        }
        if let Some(store) = &self.inner.evidence_ref_store {
            store
                .delete_for_conversation(options.tenant_id, &options.session_id.to_string())
                .await
                .map_err(HarnessError::Journal)?;
        }

        let deleted = self
            .inner
            .event_store
            .delete_session(options.tenant_id, options.session_id)
            .await
            .map_err(HarnessError::Journal)?;
        if !deleted {
            return Ok(false);
        }
        self.inner
            .deleted_conversation_sessions
            .lock()
            .insert((options.tenant_id, options.session_id));
        self.cancel_conversation_session_runs(options.tenant_id, options.session_id);
        Ok(true)
    }

    async fn conversation_session_has_journal_events(
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
        Ok(!page.envelopes.is_empty())
    }

    pub async fn submit_conversation_turn(
        &self,
        request: ConversationTurnRequest,
    ) -> Result<ConversationTurnReceipt, HarnessError> {
        self.submit_conversation_turn_inner(request, None, Vec::new())
            .await
    }

    pub async fn submit_conversation_turn_with_run_control(
        &self,
        request: ConversationTurnRequest,
        run_id: RunId,
        run_control: RunControlHandle,
    ) -> Result<ConversationTurnReceipt, HarnessError> {
        self.submit_conversation_turn_inner(request, Some((run_id, run_control)), Vec::new())
            .await
    }

    pub async fn submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
        &self,
        request: ConversationTurnRequest,
        run_id: RunId,
        run_control: RunControlHandle,
        skill_context_delivery_keys: Vec<Option<String>>,
    ) -> Result<ConversationTurnReceipt, HarnessError> {
        self.submit_conversation_turn_inner(
            request,
            Some((run_id, run_control)),
            skill_context_delivery_keys,
        )
        .await
    }

    async fn submit_conversation_turn_inner(
        &self,
        request: ConversationTurnRequest,
        controlled_run: Option<(RunId, RunControlHandle)>,
        skill_context_delivery_keys: Vec<Option<String>>,
    ) -> Result<ConversationTurnReceipt, HarnessError> {
        if request.input.prompt.trim().is_empty() {
            return Err(HarnessError::Session(SessionError::Message(
                "prompt must not be empty".to_owned(),
            )));
        }

        let options = self.effective_sdk_session_options(request.options)?;
        let mut run_options = request.run_options;
        if !self.inner.options.tool_search_enabled {
            run_options.tool_search = ToolSearchMode::Disabled;
        }
        self.ensure_conversation_session_not_deleted(options.tenant_id, options.session_id)?;
        let state = self
            .read_sdk_session_state(&options)
            .await?
            .ok_or_else(|| sdk_session_not_found(options.session_id))?;
        let projection = state.projection;
        if projection.end_reason.is_some() {
            return Err(HarnessError::Session(SessionError::Message(
                "cannot submit turn to ended session".to_owned(),
            )));
        }
        let (run_id, run_control) = match controlled_run {
            Some((run_id, run_control)) => (run_id, Some(run_control)),
            None => (RunId::new(), None),
        };
        let _active_session = ActiveConversationSessionGuard::register(
            Arc::clone(&self.inner.active_conversation_sessions),
            options.tenant_id,
            options.session_id,
            run_id,
        )
        .map_err(HarnessError::Session)?;
        let model_id = run_options
            .model_id
            .clone()
            .unwrap_or_else(|| self.inner.options.model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.inner.model.as_ref(), &model_id)?;
        let hydrated = self
            .hydrate_memory_references(
                request.input,
                &options,
                &projection,
                run_id,
                &skill_context_delivery_keys,
            )
            .await?;
        let parts = conversation_turn_parts(
            &hydrated.input,
            &model_snapshot.conversation_capability.input_modalities,
        );
        let session = self
            .resume_sdk_session_from_projection(
                options.clone(),
                &run_options,
                projection,
                run_control.map(|run_control| (run_id, run_control)),
                hydrated.skill_turn_snapshot.clone(),
            )
            .await?;
        for patch in hydrated.memory_patches {
            session
                .push_context_patch(harness_contracts::ContextPatchRequest {
                    tenant_id: options.tenant_id,
                    session_id: options.session_id,
                    run_id: RunId::new(),
                    source: harness_contracts::ContextPatchSource::MemoryReference {
                        provider_id: patch.provider_id,
                        memory_ids: vec![patch.memory_id],
                    },
                    body: patch.fence,
                    lifecycle: harness_contracts::ContextPatchLifecycle::Transient,
                })
                .await?;
        }
        for patch in &hydrated.skill_patches {
            session
                .push_context_patch(harness_contracts::ContextPatchRequest {
                    tenant_id: options.tenant_id,
                    session_id: options.session_id,
                    run_id,
                    source: harness_contracts::ContextPatchSource::SkillReference {
                        skill_id: patch.skill_id.clone(),
                        skill_name: patch.skill_name.clone(),
                        delivery_key: patch.delivery_key.clone(),
                    },
                    body: patch.body.clone(),
                    lifecycle: harness_contracts::ContextPatchLifecycle::Transient,
                })
                .await?;
            if matches!(
                patch.stage,
                SkillContextDeliveryStage::Prepared | SkillContextDeliveryStage::ContextAssembled
            ) {
                self.inner
                    .event_store
                    .append(
                        options.tenant_id,
                        options.session_id,
                        &[Event::SkillContextAssembled(SkillContextAssembledEvent {
                            session_id: options.session_id,
                            run_id,
                            delivery_key: patch.delivery_key.clone(),
                            at: harness_contracts::now(),
                        })],
                    )
                    .await
                    .map_err(HarnessError::Journal)?;
            }
        }
        let run_id = session
            .run_turn_parts_with_client_message_id_attachments_permission_mode_actor_source_and_run_id(
                parts,
                hydrated.input.client_message_id.clone(),
                hydrated.input.attachments.clone(),
                Some(run_options.permission_mode),
                request
                    .permission_actor_source
                    .unwrap_or(harness_contracts::PermissionActorSource::ParentRun),
                run_id,
            )
            .await?;
        let projection = session.projection().await;
        Ok(ConversationTurnReceipt {
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            run_id,
            message_count: projection.messages.len(),
        })
    }

    pub async fn page_conversation_events(
        &self,
        request: ConversationEventsPageRequest,
    ) -> Result<ConversationEventsPage, HarnessError> {
        let options = self.effective_sdk_session_options(request.options)?;
        let limit = request.limit.clamp(1, 200);
        let page = self
            .inner
            .event_store
            .page_session_envelopes(
                options.tenant_id,
                options.session_id,
                request.after_event_id,
                limit,
            )
            .await
            .map_err(HarnessError::Journal)?;
        let mut envelopes = page.envelopes;
        if request.after_event_id.is_none() {
            self.enforce_sdk_session_options_hash(&options, &envelopes)?;
        } else {
            let header = self
                .inner
                .event_store
                .page_session_envelopes(options.tenant_id, options.session_id, None, 1)
                .await
                .map_err(HarnessError::Journal)?;
            self.enforce_sdk_session_options_hash(&options, &header.envelopes)?;
        }
        let redactor = self.hook_redactor();
        for envelope in &mut envelopes {
            envelope.payload =
                redact_business_event_for_display(envelope.payload.clone(), redactor.as_ref());
        }
        Ok(ConversationEventsPage {
            events: envelopes,
            next_event_id: page.next_event_id,
        })
    }

    pub async fn cancel_conversation_run(&self, run_id: RunId) -> Result<(), HarnessError> {
        let active_run = self
            .inner
            .active_conversation_runs
            .lock()
            .get(&run_id)
            .cloned();

        let mut cancelled = false;
        if let Some(active_run) = active_run {
            active_run.cancellation.cancel(InterruptCause::User);
            cancelled = true;
        }

        if !cancelled {
            return Err(HarnessError::Session(SessionError::Message(format!(
                "run is not active or cannot be cancelled through this facade: {run_id}"
            ))));
        }

        Ok(())
    }

    pub(super) fn run_scoped_process_registry(
        &self,
    ) -> Option<Arc<dyn RunScopedProcessRegistryCap>> {
        self.inner
            .cap_registry
            .get::<dyn RunScopedProcessRegistryCap>(&ToolCapability::Custom(
                RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned(),
            ))
    }

    fn cancel_conversation_session_runs(&self, tenant_id: TenantId, session_id: SessionId) {
        let active_runs: Vec<_> = self
            .inner
            .active_conversation_runs
            .lock()
            .values()
            .filter(|active_run| {
                active_run.tenant_id == tenant_id && active_run.session_id == session_id
            })
            .cloned()
            .collect();

        for active_run in active_runs {
            active_run.cancellation.cancel(InterruptCause::System {
                reason: "conversation deleted".to_owned(),
            });
        }
    }

    fn ensure_conversation_session_not_deleted(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), HarnessError> {
        if self
            .inner
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant_id, session_id))
        {
            return Err(sdk_session_not_found(session_id));
        }

        Ok(())
    }

    pub(super) fn conversation_deletion_guarded_event_store(&self) -> Arc<dyn EventStore> {
        Arc::new(ConversationDeletionGuardEventStore {
            inner: Arc::clone(&self.inner.event_store),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            evidence_ref_store: self.inner.evidence_ref_store.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use harness_contracts::{BlobId, BlobRef, ModelModality};

    use super::*;

    #[test]
    fn conversation_turn_parts_promotes_file_attachment_when_model_accepts_file_input() {
        let input = ConversationTurnInput {
            client_message_id: None,
            prompt: "Summarize this".to_owned(),
            context_references: Vec::new(),
            attachments: vec![attachment("notes.pdf", "application/pdf")],
        };

        let parts = conversation_turn_parts(&input, &[ModelModality::Text, ModelModality::File]);

        assert!(parts.iter().any(|part| {
            matches!(
                part,
                MessagePart::File {
                    mime_type,
                    blob_ref
                } if mime_type == "application/pdf" && blob_ref.content_type.as_deref() == Some("application/pdf")
            )
        }));
        assert!(
            !parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text),
                    _ => None,
                })
                .any(|text| text.contains("notes.pdf")),
            "file-capable models should receive files as model input, not text context"
        );
    }

    #[test]
    fn conversation_turn_parts_keeps_file_attachment_as_text_when_model_lacks_file_input() {
        let input = ConversationTurnInput {
            client_message_id: None,
            prompt: "Summarize this".to_owned(),
            context_references: Vec::new(),
            attachments: vec![attachment("notes.pdf", "application/pdf")],
        };

        let parts = conversation_turn_parts(&input, &[ModelModality::Text]);

        assert!(parts
            .iter()
            .any(|part| { matches!(part, MessagePart::Text(text) if text.contains("notes.pdf")) }));
        assert!(!parts
            .iter()
            .any(|part| matches!(part, MessagePart::File { .. })));
    }

    fn attachment(name: &str, mime_type: &str) -> ConversationAttachmentReference {
        ConversationAttachmentReference {
            id: "attachment-test".to_owned(),
            name: name.to_owned(),
            mime_type: mime_type.to_owned(),
            size_bytes: 42,
            blob_ref: BlobRef {
                id: BlobId::from_u128(42),
                size: 42,
                content_hash: [7; 32],
                content_type: Some(mime_type.to_owned()),
            },
        }
    }
}
