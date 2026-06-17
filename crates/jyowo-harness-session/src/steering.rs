use std::collections::{BTreeMap, VecDeque};

use chrono::{DateTime, Utc};
use harness_contracts::{
    Event, MessageId, RunId, SessionError, SteeringBody, SteeringDropReason, SteeringId,
    SteeringKind, SteeringMessage, SteeringMessageAppliedEvent, SteeringMessageDroppedEvent,
    SteeringMessageQueuedEvent, SteeringOverflow, SteeringPolicy, SteeringPriority,
    SteeringRequest,
};
use tokio::sync::Mutex;

use crate::{session_error, Session};

#[derive(Debug, Clone)]
pub struct SteeringSnapshot {
    pub messages: Vec<SteeringMessage>,
    pub policy: SteeringPolicy,
}

#[derive(Debug, Clone)]
pub struct SynthesizedUserMessage {
    pub body: String,
    pub ids: Vec<SteeringId>,
    pub applied_event: Event,
}

#[derive(Debug)]
pub struct SteeringQueue {
    policy: SteeringPolicy,
    inner: Mutex<VecDeque<SteeringMessage>>,
}

impl Default for SteeringQueue {
    fn default() -> Self {
        Self::new(SteeringPolicy::default())
    }
}

impl SteeringQueue {
    pub fn new(policy: SteeringPolicy) -> Self {
        Self {
            policy,
            inner: Mutex::new(VecDeque::new()),
        }
    }
}

impl Session {
    pub async fn push_steering(
        &self,
        request: SteeringRequest,
    ) -> Result<SteeringId, SessionError> {
        self.push_steering_at(request, harness_contracts::now())
            .await
    }

    pub async fn push_steering_at(
        &self,
        request: SteeringRequest,
        now: DateTime<Utc>,
    ) -> Result<SteeringId, SessionError> {
        let id = SteeringId::new();
        let request_body_hash = body_hash(&request.body);
        let body_size = body_size(&request.body);
        let priority = request.priority.unwrap_or(SteeringPriority::Normal);
        let mut dropped = Vec::new();
        let queued = {
            let mut queue = self.steering().inner.lock().await;
            if self.steering().policy.capacity == 0 {
                dropped.push(drop_event(
                    id,
                    self,
                    None,
                    SteeringDropReason::Capacity,
                    now,
                ));
                None
            } else if priority != SteeringPriority::High {
                if let Some(existing) = queue.iter().find(|message| {
                    message.source == request.source
                        && body_hash(&message.body) == request_body_hash
                        && (now - message.queued_at).num_milliseconds()
                            <= self.steering().policy.dedup_window_ms as i64
                }) {
                    dropped.push(drop_event(
                        existing.id,
                        self,
                        existing.run_id,
                        SteeringDropReason::DedupHit,
                        now,
                    ));
                    return Ok(existing.id);
                }
                enqueue_or_overflow(self, &mut queue, request, id, priority, now, &mut dropped)?
            } else {
                enqueue_or_overflow(self, &mut queue, request, id, priority, now, &mut dropped)?
            }
        };

        if !dropped.is_empty() {
            self.event_store()
                .append(self.tenant_id(), self.session_id(), &dropped)
                .await
                .map_err(session_error)?;
        }
        if let Some(message) = queued {
            self.event_store()
                .append(
                    self.tenant_id(),
                    self.session_id(),
                    &[Event::SteeringMessageQueued(SteeringMessageQueuedEvent {
                        id: message.id,
                        session_id: self.session_id(),
                        run_id: message.run_id,
                        kind: message.kind,
                        priority: message.priority,
                        source: message.source,
                        body_hash: request_body_hash,
                        body_size,
                        body_blob: None,
                        correlation_id: message.correlation_id,
                        at: now,
                    })],
                )
                .await
                .map_err(session_error)?;
            Ok(message.id)
        } else {
            Err(SessionError::Message("steering message dropped".to_owned()))
        }
    }

    pub async fn steering_snapshot(&self) -> SteeringSnapshot {
        SteeringSnapshot {
            messages: self.steering().inner.lock().await.iter().cloned().collect(),
            policy: self.steering().policy.clone(),
        }
    }

    pub async fn drain_and_merge(
        &self,
        run_id: RunId,
    ) -> Result<Option<SynthesizedUserMessage>, SessionError> {
        self.drain_and_merge_into(run_id, None).await
    }

    pub async fn drain_and_merge_at(
        &self,
        run_id: RunId,
        now: DateTime<Utc>,
    ) -> Result<Option<SynthesizedUserMessage>, SessionError> {
        self.drain_and_merge_at_into(run_id, now, None).await
    }

    pub async fn drain_and_merge_into(
        &self,
        run_id: RunId,
        merged_into_message_id: Option<MessageId>,
    ) -> Result<Option<SynthesizedUserMessage>, SessionError> {
        self.drain_and_merge_at_into(run_id, harness_contracts::now(), merged_into_message_id)
            .await
    }

    pub async fn drain_and_merge_at_into(
        &self,
        run_id: RunId,
        now: DateTime<Utc>,
        merged_into_message_id: Option<MessageId>,
    ) -> Result<Option<SynthesizedUserMessage>, SessionError> {
        let mut dropped = Vec::new();
        let drained = {
            let mut queue = self.steering().inner.lock().await;
            let mut kept = VecDeque::new();
            let mut drained = Vec::new();
            while let Some(message) = queue.pop_front() {
                if is_expired(&message, &self.steering().policy, now) {
                    dropped.push(drop_event(
                        message.id,
                        self,
                        message.run_id,
                        SteeringDropReason::TtlExpired,
                        now,
                    ));
                } else {
                    drained.push(message);
                }
            }
            *queue = std::mem::take(&mut kept);
            drained
        };
        if !dropped.is_empty() {
            self.event_store()
                .append(self.tenant_id(), self.session_id(), &dropped)
                .await
                .map_err(session_error)?;
        }
        if drained.is_empty() {
            return Ok(None);
        }

        let mut body = String::new();
        let mut ids = Vec::new();
        let mut distribution = BTreeMap::new();
        for message in &drained {
            ids.push(message.id);
            *distribution.entry(message.kind).or_insert(0) += 1;
            match message.kind {
                SteeringKind::Append => append_body(&mut body, &message.body),
                SteeringKind::Replace => {
                    body.clear();
                    append_body(&mut body, &message.body);
                }
                _ => {}
            }
        }
        let applied_event = Event::SteeringMessageApplied(SteeringMessageAppliedEvent {
            ids: ids.clone(),
            session_id: self.session_id(),
            run_id,
            merged_into_message_id,
            kind_distribution: distribution,
            at: now,
        });
        self.event_store()
            .append(
                self.tenant_id(),
                self.session_id(),
                std::slice::from_ref(&applied_event),
            )
            .await
            .map_err(session_error)?;

        Ok(Some(SynthesizedUserMessage {
            body,
            ids,
            applied_event,
        }))
    }

    pub async fn handle_run_ended_for_test(&self, _run_id: RunId) -> Result<(), SessionError> {
        Ok(())
    }

    pub(crate) async fn drop_steering_for_session_end(&self) -> Result<(), SessionError> {
        let now = harness_contracts::now();
        let dropped = {
            let mut queue = self.steering().inner.lock().await;
            queue
                .drain(..)
                .map(|message| {
                    drop_event(
                        message.id,
                        self,
                        message.run_id,
                        SteeringDropReason::SessionEnded,
                        now,
                    )
                })
                .collect::<Vec<_>>()
        };
        if dropped.is_empty() {
            return Ok(());
        }
        self.event_store()
            .append(self.tenant_id(), self.session_id(), &dropped)
            .await
            .map_err(session_error)?;
        Ok(())
    }
}

fn enqueue_or_overflow(
    session: &Session,
    queue: &mut VecDeque<SteeringMessage>,
    request: SteeringRequest,
    id: SteeringId,
    priority: SteeringPriority,
    now: DateTime<Utc>,
    dropped: &mut Vec<Event>,
) -> Result<Option<SteeringMessage>, SessionError> {
    if queue.len() == session.steering().policy.capacity {
        match session.steering().policy.overflow {
            SteeringOverflow::DropOldest => {
                if let Some(oldest) = queue.pop_front() {
                    dropped.push(drop_event(
                        oldest.id,
                        session,
                        oldest.run_id,
                        SteeringDropReason::Capacity,
                        now,
                    ));
                }
            }
            SteeringOverflow::DropNewest => {
                dropped.push(drop_event(
                    id,
                    session,
                    None,
                    SteeringDropReason::Capacity,
                    now,
                ));
                return Ok(None);
            }
            SteeringOverflow::BackPressure => {
                return Err(SessionError::Message(
                    "steering queue backpressure".to_owned(),
                ));
            }
            _ => {
                return Err(SessionError::Message(
                    "unsupported steering overflow policy".to_owned(),
                ));
            }
        }
    }

    let message = SteeringMessage {
        id,
        session_id: session.session_id(),
        run_id: None,
        kind: request.kind,
        priority,
        body: request.body,
        queued_at: now,
        correlation_id: request.correlation_id,
        source: request.source,
    };
    queue.push_back(message.clone());
    Ok(Some(message))
}

fn drop_event(
    id: SteeringId,
    session: &Session,
    run_id: Option<RunId>,
    reason: SteeringDropReason,
    at: DateTime<Utc>,
) -> Event {
    Event::SteeringMessageDropped(SteeringMessageDroppedEvent {
        id,
        session_id: session.session_id(),
        run_id,
        reason,
        at,
    })
}

fn is_expired(message: &SteeringMessage, policy: &SteeringPolicy, now: DateTime<Utc>) -> bool {
    (now - message.queued_at).num_milliseconds() > policy.ttl_ms as i64
}

fn append_body(target: &mut String, body: &SteeringBody) {
    let text = match body {
        SteeringBody::Text(text) => text.as_str(),
        SteeringBody::Structured { instruction, .. } => instruction.as_str(),
        _ => "",
    };
    if text.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(text);
}

fn body_hash(body: &SteeringBody) -> [u8; 32] {
    blake3::hash(&serde_json::to_vec(body).unwrap_or_default()).into()
}

fn body_size(body: &SteeringBody) -> u32 {
    serde_json::to_vec(body)
        .map(|body| body.len().min(u32::MAX as usize) as u32)
        .unwrap_or(0)
}
