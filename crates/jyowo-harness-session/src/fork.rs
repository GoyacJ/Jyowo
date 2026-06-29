use futures::StreamExt;
use harness_contracts::{
    CacheImpact, Event, ForkReason, SessionError, SessionForkedEvent, SessionId,
};
use harness_journal::{EventStore, ReplayCursor, SessionSnapshot, SnapshotBody};

use crate::{session_error, Session, SessionPaths, SessionProjection};

impl Session {
    pub async fn fork(&self, reason: ForkReason) -> Result<Self, SessionError> {
        let parent_projection = self.replay_projection().await?;
        let child_id = SessionId::new();
        let mut child_projection = parent_projection.clone();
        child_projection.session_id = child_id;
        child_projection.end_reason = None;
        child_projection.refresh_snapshot_id();
        let mut child_options = self.options().clone();
        child_options.session_id = child_id;
        let child_paths = SessionPaths::from_workspace(
            &child_options.workspace_root,
            child_options.tenant_id,
            child_options.session_id,
        );

        self.event_store()
            .append(
                self.tenant_id(),
                self.session_id(),
                &[Event::SessionForked(SessionForkedEvent {
                    parent_session_id: self.session_id(),
                    child_session_id: child_id,
                    tenant_id: self.tenant_id(),
                    fork_reason: reason.clone(),
                    from_offset: parent_projection.last_offset,
                    config_delta_hash: None,
                    cache_impact: CacheImpact {
                        prompt_cache_invalidated: false,
                        reason: None,
                    },
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(session_error)?;
        self.event_store()
            .compact_link(self.session_id(), child_id, reason)
            .await
            .map_err(session_error)?;
        save_projection_snapshot(
            self.event_store(),
            self.tenant_id(),
            child_id,
            &child_projection,
        )
        .await?;

        Session::from_projection(
            child_options,
            child_paths,
            self.event_store().clone(),
            self.turn_runtime(),
            self.turn_runner(),
            self.skill_reload_cap(),
            self.effective_prompt_inputs_hash(),
            child_projection,
        )
        .await
    }

    pub async fn last_offset(&self) -> harness_contracts::JournalOffset {
        self.projection().await.last_offset
    }

    pub(crate) async fn replay_projection(&self) -> Result<SessionProjection, SessionError> {
        let envelopes = self
            .event_store()
            .read_envelopes(self.tenant_id(), self.session_id(), ReplayCursor::FromStart)
            .await
            .map_err(session_error)?
            .collect::<Vec<_>>()
            .await;
        SessionProjection::replay(envelopes)
    }
}

async fn save_projection_snapshot(
    store: &std::sync::Arc<dyn EventStore>,
    tenant_id: harness_contracts::TenantId,
    session_id: SessionId,
    projection: &SessionProjection,
) -> Result<(), SessionError> {
    let body = serde_json::to_vec(projection).map_err(session_error)?;
    store
        .save_snapshot(
            tenant_id,
            SessionSnapshot {
                session_id,
                tenant_id,
                offset: projection.last_offset,
                taken_at: harness_contracts::now(),
                body: SnapshotBody::Full(body),
            },
        )
        .await
        .map_err(session_error)
}
