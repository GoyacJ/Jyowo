use super::*;

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
                .map(render_context_reference),
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
        ConversationContextReference::Memory { id, label } => {
            format!(
                "- memory: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Skill { id, label } => {
            format!(
                "- skill: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
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

impl Harness {
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
        #[cfg_attr(not(feature = "sqlite-store"), allow(unused_mut))]
        let mut deleted = self
            .inner
            .event_store
            .delete_session(options.tenant_id, options.session_id)
            .await
            .map_err(HarnessError::Journal)?;
        #[cfg(feature = "sqlite-store")]
        {
            let read_model = self.conversation_read_model().await?;
            if deleted {
                read_model
                    .reset_session(options.tenant_id, options.session_id)
                    .await
                    .map_err(HarnessError::Journal)?;
            } else if read_model
                .summary(options.tenant_id, options.session_id)
                .await
                .map_err(HarnessError::Journal)?
                .is_some()
            {
                read_model
                    .reset_session(options.tenant_id, options.session_id)
                    .await
                    .map_err(HarnessError::Journal)?;
                deleted = true;
            }
        }
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

    pub async fn submit_conversation_turn(
        &self,
        request: ConversationTurnRequest,
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
        let last_offset = projection.last_offset;
        let model_id = run_options
            .model_id
            .clone()
            .unwrap_or_else(|| self.inner.options.model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.inner.model.as_ref(), &model_id)?;
        let parts = conversation_turn_parts(
            &request.input,
            &model_snapshot.conversation_capability.input_modalities,
        );
        let session = self
            .resume_sdk_session_from_projection(options.clone(), &run_options, projection)
            .await?;
        session
            .run_turn_parts_with_client_message_id_attachments_permission_mode_and_actor_source(
                parts,
                request.input.client_message_id.clone(),
                request.input.attachments.clone(),
                None,
                request
                    .permission_actor_source
                    .unwrap_or(harness_contracts::PermissionActorSource::ParentRun),
            )
            .await?;
        let new_events = self
            .inner
            .event_store
            .read_envelopes(
                options.tenant_id,
                options.session_id,
                ReplayCursor::FromOffset(last_offset),
            )
            .await
            .map_err(HarnessError::Journal)?
            .collect::<Vec<_>>()
            .await;
        let run_id = new_events
            .iter()
            .find_map(|envelope| match &envelope.payload {
                Event::RunStarted(started) => Some(started.run_id),
                _ => None,
            })
            .ok_or_else(|| {
                HarnessError::Session(SessionError::Message(
                    "run did not emit RunStarted".to_owned(),
                ))
            })?;
        #[cfg(feature = "agents-team")]
        if let Some(agent_run_options) = &run_options.agent_run_options {
            if harness_agent_runtime::should_start_run_scoped_team(agent_run_options) {
                let profiles = crate::list_agent_profiles(&options.workspace_root)
                    .map_err(|error| HarnessError::Other(error.to_string()))?;
                self.start_run_scoped_team(super::team_runtime::RunScopedTeamStartupRequest {
                    agent_run_options: agent_run_options.clone(),
                    profiles,
                    run_id,
                    conversation_session_id: options.session_id,
                    goal: request.input.prompt.clone(),
                    workspace_root: options.workspace_root.clone(),
                    workspace_bootstrap: options.workspace_bootstrap.clone(),
                })
                .await?;
            }
        }
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

        #[cfg(feature = "agents-team")]
        if self.has_active_run_team(run_id) {
            self.cancel_active_run_team(run_id).await?;
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
