#[cfg(feature = "sqlite-store")]
use super::*;

#[cfg(feature = "sqlite-store")]
impl Harness {
    #[cfg(feature = "sqlite-store")]
    pub(super) async fn conversation_read_model(
        &self,
    ) -> Result<Arc<SqliteConversationReadModelStore>, HarnessError> {
        let path = self
            .inner
            .options
            .workspace_root
            .join(".jyowo/runtime/conversation-read-model.sqlite");
        let store = self
            .inner
            .conversation_read_model
            .get_or_try_init(|| async move {
                SqliteConversationReadModelStore::open(path)
                    .await
                    .map(Arc::new)
                    .map_err(HarnessError::Journal)
            })
            .await?;
        Ok(Arc::clone(store))
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn list_conversation_summaries(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, HarnessError> {
        let bounded_limit = limit.clamp(1, 200);
        let sessions = self
            .inner
            .event_store
            .list_sessions(
                tenant_id,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: u32::MAX,
                },
            )
            .await
            .map_err(HarnessError::Journal)?;
        let mut live_conversation_session_ids = HashSet::new();
        for session in sessions {
            if self
                .is_conversation_session_stream_page(tenant_id, session.session_id)
                .await?
            {
                live_conversation_session_ids.insert(session.session_id);
                self.catch_up_conversation_projection(tenant_id, session.session_id)
                    .await?;
            }
        }
        let read_model = self.conversation_read_model().await?;
        loop {
            let summaries = read_model
                .list_summaries(tenant_id, 200)
                .await
                .map_err(HarnessError::Journal)?;
            let mut visible_summaries = Vec::new();
            let mut removed_stale_summary = false;
            for summary in summaries {
                let Ok(session_id) = SessionId::parse(&summary.id) else {
                    continue;
                };
                if live_conversation_session_ids.contains(&session_id) {
                    visible_summaries.push(summary);
                } else {
                    read_model
                        .reset_session(tenant_id, session_id)
                        .await
                        .map_err(HarnessError::Journal)?;
                    removed_stale_summary = true;
                }
                if visible_summaries.len() >= bounded_limit {
                    break;
                }
            }
            if visible_summaries.len() >= bounded_limit || !removed_stale_summary {
                return Ok(visible_summaries);
            }
        }
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn get_conversation_snapshot(
        &self,
        conversation_id: &str,
        message_limit: usize,
    ) -> Result<Option<ConversationSnapshot>, HarnessError> {
        let tenant_id = self.inner.options.tenant_policy.id;
        let session_id = parse_conversation_session_id(conversation_id)?;
        let read_model = self.conversation_read_model().await?;
        let existing_empty_summary = read_model
            .summary(tenant_id, session_id)
            .await
            .map_err(HarnessError::Journal)?
            .filter(|summary| summary.is_empty);
        self.catch_up_conversation_projection(tenant_id, session_id)
            .await?;
        let snapshot = read_model
            .snapshot(tenant_id, session_id, message_limit)
            .await
            .map_err(HarnessError::Journal)?;
        if snapshot.is_some() {
            return Ok(snapshot);
        }
        if let Some(existing_empty_summary) = existing_empty_summary {
            read_model
                .seed_empty_conversation(
                    tenant_id,
                    session_id,
                    existing_empty_summary.updated_at,
                    existing_empty_summary.model_config_id.as_deref(),
                )
                .await
                .map_err(HarnessError::Journal)?;
            return read_model
                .snapshot(tenant_id, session_id, message_limit)
                .await
                .map_err(HarnessError::Journal);
        }
        let Some(summary) = self
            .conversation_session_summary(tenant_id, session_id)
            .await?
        else {
            return Ok(None);
        };
        read_model
            .seed_empty_summary(tenant_id, &summary, None)
            .await
            .map_err(HarnessError::Journal)?;
        read_model
            .snapshot(tenant_id, session_id, message_limit)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn page_conversation_timeline(
        &self,
        conversation_id: &str,
        after_cursor: Option<ConversationCursor>,
        limit: usize,
    ) -> Result<ConversationTimelinePage, HarnessError> {
        let tenant_id = self.inner.options.tenant_policy.id;
        let session_id = parse_conversation_session_id(conversation_id)?;
        self.catch_up_conversation_projection(tenant_id, session_id)
            .await?;
        self.conversation_read_model()
            .await?
            .page_timeline(tenant_id, session_id, after_cursor, limit)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn page_conversation_worktree(
        &self,
        conversation_id: &str,
        page_cursor: Option<ConversationTurnCursor>,
        direction: ConversationTurnPageDirection,
        limit_turns: usize,
    ) -> Result<ConversationWorktreePage, HarnessError> {
        let tenant_id = self.inner.options.tenant_policy.id;
        let session_id = parse_conversation_session_id(conversation_id)?;
        self.catch_up_conversation_projection(tenant_id, session_id)
            .await?;
        let evidence_store = self.evidence_ref_store()?;
        self.conversation_read_model()
            .await?
            .page_worktree_with_evidence(
                tenant_id,
                session_id,
                page_cursor,
                direction,
                limit_turns,
                evidence_store,
            )
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn catch_up_conversation_projection(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), HarnessError> {
        let read_model = self.conversation_read_model().await?;
        let mut after_event_id = read_model
            .projection_cursor(tenant_id, session_id)
            .await
            .map_err(HarnessError::Journal)?
            .map(|cursor| cursor.event_id);
        let mut reset_stale_projection = false;
        loop {
            let page = match self
                .inner
                .event_store
                .page_session_envelopes(tenant_id, session_id, after_event_id, 200)
                .await
            {
                Ok(page) => page,
                Err(error)
                    if after_event_id.is_some()
                        && !reset_stale_projection
                        && error.to_string().contains("conversation cursor is unknown") =>
                {
                    read_model
                        .reset_session(tenant_id, session_id)
                        .await
                        .map_err(HarnessError::Journal)?;
                    after_event_id = None;
                    reset_stale_projection = true;
                    continue;
                }
                Err(error) => return Err(HarnessError::Journal(error)),
            };
            if page.envelopes.is_empty() {
                return Ok(());
            }
            read_model
                .apply_envelopes(tenant_id, session_id, &page.envelopes, None)
                .await
                .map_err(HarnessError::Journal)?;
            after_event_id = page.next_event_id;
        }
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn conversation_session_exists(
        &self,
        options: SessionOptions,
    ) -> Result<bool, HarnessError> {
        let options = self.effective_sdk_session_options(options)?;
        if self
            .inner
            .deleted_conversation_sessions
            .lock()
            .contains(&(options.tenant_id, options.session_id))
        {
            return Ok(false);
        }
        if self
            .conversation_read_model()
            .await?
            .snapshot(options.tenant_id, options.session_id, 1)
            .await
            .map_err(HarnessError::Journal)?
            .is_some()
        {
            return Ok(true);
        }
        Ok(self
            .conversation_session_summary(options.tenant_id, options.session_id)
            .await?
            .is_some())
    }

    #[cfg(feature = "sqlite-store")]
    async fn conversation_session_summary(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<Option<harness_journal::SessionSummary>, HarnessError> {
        let summaries = self
            .inner
            .event_store
            .list_sessions(
                tenant_id,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: 200,
                },
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(summaries
            .into_iter()
            .find(|summary| summary.session_id == session_id))
    }
}

#[cfg(feature = "sqlite-store")]
fn parse_conversation_session_id(conversation_id: &str) -> Result<SessionId, HarnessError> {
    SessionId::parse(conversation_id).map_err(|error| {
        HarnessError::Session(SessionError::Message(format!(
            "invalid conversation id: {error}"
        )))
    })
}
