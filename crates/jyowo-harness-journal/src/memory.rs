//! In-memory `EventStore` for tests.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use harness_contracts::{
    Event, EventId, ForkReason, JournalError, JournalOffset, Redactor, SessionId, TenantId,
};
use tokio::sync::Mutex;

use crate::{
    apply_cursor, AppendMetadata, EventEnvelope, EventStore, JournalRedaction, PrunePolicy,
    PruneReport, ReplayCursor, SessionFilter, SessionSnapshot, SessionSummary,
};

type SessionKey = (TenantId, SessionId);

pub struct InMemoryEventStore {
    events: Mutex<HashMap<SessionKey, Vec<EventEnvelope>>>,
    snapshots: Mutex<HashMap<SessionKey, SessionSnapshot>>,
    lineage: Mutex<Vec<(SessionId, SessionId, ForkReason)>>,
    redaction: JournalRedaction,
}

impl InMemoryEventStore {
    pub fn new(redactor: Arc<dyn Redactor>) -> Self {
        Self {
            events: Mutex::new(HashMap::new()),
            snapshots: Mutex::new(HashMap::new()),
            lineage: Mutex::new(Vec::new()),
            redaction: JournalRedaction::new(redactor),
        }
    }

    fn envelope(
        tenant_id: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        offset: JournalOffset,
        payload: Event,
    ) -> EventEnvelope {
        EventEnvelope {
            offset,
            event_id: harness_contracts::EventId::new(),
            session_id,
            tenant_id,
            run_id: metadata.run_id,
            correlation_id: metadata.correlation_id,
            causation_id: metadata.causation_id,
            recorded_at: harness_contracts::now(),
            payload,
        }
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append_with_metadata(tenant, session_id, AppendMetadata::default(), events)
            .await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let mut guard = self.events.lock().await;
        let entries = guard.entry((tenant, session_id)).or_default();
        let mut offset = entries.len() as u64;
        for event in events {
            let payload = self.redaction.redact_event(event)?;
            entries.push(Self::envelope(
                tenant,
                session_id,
                metadata,
                JournalOffset(offset),
                payload,
            ));
            offset += 1;
        }
        Ok(JournalOffset(offset.saturating_sub(1)))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        let mut events = self
            .events
            .lock()
            .await
            .get(&(tenant, session_id))
            .cloned()
            .unwrap_or_default();
        apply_cursor(&mut events, cursor);
        Ok(Box::pin(stream::iter(events)))
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let mut events: Vec<_> = self
            .events
            .lock()
            .await
            .iter()
            .filter(|((entry_tenant, _), _)| *entry_tenant == tenant)
            .flat_map(|(_, envelopes)| envelopes.clone())
            .collect();
        events.sort_by_key(|envelope| (envelope.recorded_at, envelope.offset));
        if let Some(after) = after {
            if let Some(position) = events
                .iter()
                .position(|envelope| envelope.event_id == after)
            {
                events = events.into_iter().skip(position + 1).collect();
            } else {
                let after = after.to_string();
                events.retain(|envelope| envelope.event_id.to_string() > after);
            }
        }
        events.truncate(limit);
        Ok(events)
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(self
            .snapshots
            .lock()
            .await
            .get(&(tenant, session_id))
            .cloned())
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.snapshots
            .lock()
            .await
            .insert((tenant, snapshot.session_id), snapshot);
        Ok(())
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError> {
        self.lineage.lock().await.push((parent, child, reason));
        Ok(())
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        let mut removed = self
            .events
            .lock()
            .await
            .remove(&(tenant, session_id))
            .is_some();
        if self
            .snapshots
            .lock()
            .await
            .remove(&(tenant, session_id))
            .is_some()
        {
            removed = true;
        }
        let mut lineage = self.lineage.lock().await;
        let original_len = lineage.len();
        lineage.retain(|(parent, child, _)| *parent != session_id && *child != session_id);
        if lineage.len() != original_len {
            removed = true;
        }
        Ok(removed)
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        let mut sessions: Vec<_> = self
            .events
            .lock()
            .await
            .iter()
            .filter_map(|((entry_tenant, session_id), events)| {
                if *entry_tenant != tenant || events.is_empty() {
                    return None;
                }
                let created_at = events.first()?.recorded_at;
                if filter.since.is_some_and(|since| created_at < since) {
                    return None;
                }
                let end_reason = events
                    .iter()
                    .filter_map(|envelope| crate::session_end_reason(&envelope.payload))
                    .find_map(|(ended_session, reason)| {
                        (ended_session == *session_id).then_some(reason)
                    });
                if filter
                    .end_reason
                    .as_ref()
                    .is_some_and(|expected| end_reason.as_ref() != Some(expected))
                {
                    return None;
                }
                Some(SessionSummary {
                    session_id: *session_id,
                    created_at,
                    last_event_at: events.last()?.recorded_at,
                    event_count: events.len() as u64,
                    end_reason,
                    root_session: None,
                })
            })
            .collect();
        if filter.project_compression_tips {
            apply_lineage_projection(&mut sessions, &self.lineage.lock().await);
        }
        sessions.sort_by_key(|summary| summary.created_at);
        sessions.truncate(filter.limit as usize);
        Ok(sessions)
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        let cutoff = harness_contracts::now()
            - chrono::Duration::from_std(policy.older_than)
                .unwrap_or_else(|_| chrono::Duration::zero());
        let mut summaries: Vec<_> = {
            let events = self.events.lock().await;
            events
                .iter()
                .filter_map(|((entry_tenant, session_id), envelopes)| {
                    if *entry_tenant != tenant || envelopes.is_empty() {
                        return None;
                    }
                    Some((*session_id, envelopes.last()?.recorded_at))
                })
                .collect()
        };
        summaries.sort_by_key(|(_, last_event_at)| *last_event_at);
        summaries.reverse();
        let keep: HashSet<_> = policy
            .keep_latest_n_sessions
            .map(|limit| {
                summaries
                    .iter()
                    .take(limit as usize)
                    .map(|(session_id, _)| *session_id)
                    .collect()
            })
            .unwrap_or_default();
        let candidates: Vec<_> = summaries
            .into_iter()
            .filter_map(|(session_id, last_event_at)| {
                (last_event_at <= cutoff && !keep.contains(&session_id)).then_some(session_id)
            })
            .collect();
        self.prune_sessions(tenant, &candidates, policy.keep_snapshots)
            .await
    }

    async fn prune_sessions(
        &self,
        tenant: TenantId,
        session_ids: &[SessionId],
        keep_snapshots: bool,
    ) -> Result<PruneReport, JournalError> {
        let mut report = PruneReport::default();
        let candidate_set: HashSet<_> = session_ids.iter().copied().collect();
        let mut events = self.events.lock().await;
        for session_id in session_ids {
            if let Some(removed) = events.remove(&(tenant, *session_id)) {
                report.events_removed += removed.len() as u64;
            }
        }
        if !keep_snapshots {
            let mut snapshots = self.snapshots.lock().await;
            for session_id in session_ids {
                if snapshots.remove(&(tenant, *session_id)).is_some() {
                    report.snapshots_removed += 1;
                }
            }
        }
        self.lineage.lock().await.retain(|(parent, child, _)| {
            !candidate_set.contains(parent) && !candidate_set.contains(child)
        });
        Ok(report)
    }
}

fn apply_lineage_projection(
    sessions: &mut Vec<SessionSummary>,
    lineage: &[(SessionId, SessionId, ForkReason)],
) {
    let parent_by_child: HashMap<_, _> = lineage
        .iter()
        .map(|(parent, child, _)| (*child, *parent))
        .collect();
    let parents: HashSet<_> = lineage.iter().map(|(parent, _, _)| *parent).collect();
    sessions.retain(|summary| !parents.contains(&summary.session_id));
    for summary in sessions {
        let mut root = None;
        let mut cursor = summary.session_id;
        while let Some(parent) = parent_by_child.get(&cursor) {
            root = Some(*parent);
            cursor = *parent;
        }
        summary.root_session = root;
    }
}
