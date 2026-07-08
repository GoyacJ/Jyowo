//! Projection traits and deterministic replay context.
//!

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    BlobId, BlobRef, EndReason, Event, JournalError, JournalOffset, Message, MessageContent,
    MessagePart, MessageRole, OffloadedBlobAuthorizerCap, RunId, SessionId, TenantId, ToolError,
    UsageSnapshot,
};

use crate::{EventStore, ReplayCursor};

pub type ProjectionError = JournalError;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ReplayContext {
    pub now: DateTime<Utc>,
    pub rng_seed: u64,
}

impl ReplayContext {
    pub fn new(now: DateTime<Utc>, rng_seed: u64) -> Self {
        Self { now, rng_seed }
    }
}

pub trait Projection: Sized + Send + Sync {
    type State;

    fn initial() -> Self::State;

    fn apply(
        state: &mut Self::State,
        event: &Event,
        ctx: &ReplayContext,
    ) -> Result<(), ProjectionError>;

    fn replay<'a>(
        events: impl IntoIterator<Item = &'a Event>,
    ) -> Result<Self::State, ProjectionError> {
        Self::replay_with_context(
            events,
            ReplayContext {
                now: DateTime::<Utc>::UNIX_EPOCH,
                rng_seed: 0,
            },
        )
    }

    fn replay_with_context<'a>(
        events: impl IntoIterator<Item = &'a Event>,
        ctx: ReplayContext,
    ) -> Result<Self::State, ProjectionError> {
        let mut state = Self::initial();
        for event in events {
            Self::apply(&mut state, event, &ctx)?;
        }
        Ok(state)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionProjection {
    pub messages: Vec<Message>,
    pub usage: UsageSnapshot,
    pub end_reason: Option<EndReason>,
    pub last_offset: JournalOffset,
}

impl Default for SessionProjection {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            usage: UsageSnapshot::default(),
            end_reason: None,
            last_offset: JournalOffset(0),
        }
    }
}

impl Projection for SessionProjection {
    type State = Self;

    fn initial() -> Self::State {
        Self::default()
    }

    fn apply(
        state: &mut Self::State,
        event: &Event,
        _ctx: &ReplayContext,
    ) -> Result<(), ProjectionError> {
        match event {
            Event::UserMessageAppended(event) => {
                state.messages.push(Message {
                    id: event.message_id,
                    role: MessageRole::User,
                    parts: message_parts(&event.content),
                    created_at: event.at,
                });
            }
            Event::AssistantMessageCompleted(event) => {
                state.messages.push(Message {
                    id: event.message_id,
                    role: MessageRole::Assistant,
                    parts: message_parts(&event.content),
                    created_at: event.at,
                });
                add_usage(&mut state.usage, &event.usage);
            }
            Event::RunEnded(event) => {
                if let Some(usage) = &event.usage {
                    add_usage(&mut state.usage, usage);
                }
            }
            Event::SessionEnded(event) => {
                state.end_reason = Some(event.reason.clone());
                state.usage = event.final_usage.clone();
            }
            _ => {}
        }
        Ok(())
    }
}

fn message_parts(content: &MessageContent) -> Vec<MessagePart> {
    match content {
        MessageContent::Text(text) => vec![MessagePart::Text(text.clone())],
        MessageContent::Structured(value) => vec![MessagePart::Text(value.to_string())],
        MessageContent::Multimodal(parts) => parts.clone(),
    }
}

fn add_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens += delta.input_tokens;
    total.output_tokens += delta.output_tokens;
    total.cache_read_tokens += delta.cache_read_tokens;
    total.cache_write_tokens += delta.cache_write_tokens;
    total.cost_micros += delta.cost_micros;
    total.tool_calls += delta.tool_calls;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageProjection {
    pub usage: UsageSnapshot,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ToolPoolProjection {
    pub materialized_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OffloadedBlobProjection {
    blobs: HashMap<(RunId, BlobId), BlobRef>,
}

impl OffloadedBlobProjection {
    pub fn apply_event(&mut self, event: &Event) {
        if let Event::ToolResultOffloaded(event) = event {
            self.blobs
                .insert((event.run_id, event.blob_ref.id), event.blob_ref.clone());
        }
    }

    #[must_use]
    pub fn allows(&self, run_id: RunId, blob: &BlobRef) -> bool {
        self.blobs
            .get(&(run_id, blob.id))
            .is_some_and(|stored| stored == blob)
    }
}

#[derive(Clone)]
pub struct EventStoreOffloadedBlobAuthorizer {
    event_store: Arc<dyn EventStore>,
}

impl EventStoreOffloadedBlobAuthorizer {
    #[must_use]
    pub fn new(event_store: Arc<dyn EventStore>) -> Self {
        Self { event_store }
    }
}

impl OffloadedBlobAuthorizerCap for EventStoreOffloadedBlobAuthorizer {
    fn authorize_offloaded_blob(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        blob: BlobRef,
    ) -> BoxFuture<'_, Result<(), ToolError>> {
        let event_store = Arc::clone(&self.event_store);
        Box::pin(async move {
            let mut events = event_store
                .read(tenant_id, session_id, ReplayCursor::FromStart)
                .await
                .map_err(|error| ToolError::Message(error.to_string()))?;
            let mut projection = OffloadedBlobProjection::default();
            while let Some(event) = events.next().await {
                projection.apply_event(&event);
            }
            if projection.allows(run_id, &blob) {
                return Ok(());
            }
            Err(ToolError::PermissionDenied(
                "blob was not offloaded in current run".to_owned(),
            ))
        })
    }
}
