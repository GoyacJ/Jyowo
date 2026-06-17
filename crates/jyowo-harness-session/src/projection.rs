use std::collections::{BTreeSet, HashMap};

use harness_contracts::{
    Decision, DecisionScope, DeferredToolsDeltaAttachment, EndReason, Event, JournalOffset,
    Message, MessageContent, MessagePart, MessageRole, PermissionSubject, RequestId, SessionError,
    SessionId, SnapshotId, TenantId, ToolErrorPayload, ToolName, ToolResult, ToolUseId,
    UsageSnapshot,
};
use harness_journal::EventEnvelope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionProjection {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub messages: Vec<Message>,
    pub tool_uses: HashMap<ToolUseId, ToolUseRecord>,
    pub permission_log: Vec<PermissionRecord>,
    pub usage: UsageSnapshot,
    pub allowlist: BTreeSet<String>,
    pub end_reason: Option<EndReason>,
    pub last_offset: JournalOffset,
    pub snapshot_id: SnapshotId,
    pub discovered_tools: DiscoveredToolProjection,
    pending_deferred_tools_delta: Option<DeferredToolsDeltaAttachment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUseRecord {
    pub tool_use_id: ToolUseId,
    pub run_id: harness_contracts::RunId,
    pub tool_name: ToolName,
    pub input: serde_json::Value,
    pub result: Option<ToolResult>,
    pub error: Option<ToolErrorPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionRecord {
    pub request_id: RequestId,
    pub tool_use_id: ToolUseId,
    pub tool_name: ToolName,
    pub subject: PermissionSubject,
    pub decision: Option<Decision>,
    pub scope: DecisionScope,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredToolProjection {
    materialized: BTreeSet<ToolName>,
}

impl DiscoveredToolProjection {
    pub fn contains(&self, name: &ToolName) -> bool {
        self.materialized.contains(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ToolName> {
        self.materialized.iter()
    }

    pub fn len(&self) -> usize {
        self.materialized.len()
    }

    pub fn is_empty(&self) -> bool {
        self.materialized.is_empty()
    }
}

impl SessionProjection {
    pub fn empty(tenant_id: TenantId, session_id: SessionId) -> Self {
        let mut projection = Self {
            session_id,
            tenant_id,
            messages: Vec::new(),
            tool_uses: HashMap::new(),
            permission_log: Vec::new(),
            usage: UsageSnapshot::default(),
            allowlist: BTreeSet::new(),
            end_reason: None,
            last_offset: JournalOffset(0),
            snapshot_id: SnapshotId::from_u128(0),
            discovered_tools: DiscoveredToolProjection::default(),
            pending_deferred_tools_delta: None,
        };
        projection.refresh_snapshot_id();
        projection
    }

    pub fn replay(envelopes: Vec<EventEnvelope>) -> Result<Self, SessionError> {
        let Some(first) = envelopes.first() else {
            return Err(SessionError::Message(
                "cannot replay empty session event stream".to_owned(),
            ));
        };
        let mut state = Self::empty(first.tenant_id, first.session_id);
        let mut pending_permissions = HashMap::<RequestId, PermissionRecord>::new();
        for envelope in envelopes {
            state.last_offset = envelope.offset;
            state.apply_event(envelope.payload, &mut pending_permissions);
        }
        state.refresh_snapshot_id();
        Ok(state)
    }

    fn apply_event(
        &mut self,
        event: Event,
        pending_permissions: &mut HashMap<RequestId, PermissionRecord>,
    ) {
        match event {
            Event::SessionCreated(event) => {
                self.session_id = event.session_id;
                self.tenant_id = event.tenant_id;
            }
            Event::UserMessageAppended(event) => {
                self.messages.push(Message {
                    id: event.message_id,
                    role: MessageRole::User,
                    parts: message_parts(event.content),
                    created_at: event.at,
                });
            }
            Event::AssistantMessageCompleted(event) => {
                self.messages.push(Message {
                    id: event.message_id,
                    role: MessageRole::Assistant,
                    parts: message_parts(event.content),
                    created_at: event.at,
                });
                add_usage(&mut self.usage, &event.usage);
            }
            Event::ToolUseRequested(event) => {
                self.tool_uses.insert(
                    event.tool_use_id,
                    ToolUseRecord {
                        tool_use_id: event.tool_use_id,
                        run_id: event.run_id,
                        tool_name: event.tool_name,
                        input: event.input,
                        result: None,
                        error: None,
                    },
                );
            }
            Event::ToolUseCompleted(event) => {
                if let Some(record) = self.tool_uses.get_mut(&event.tool_use_id) {
                    record.result = Some(event.result);
                }
                if let Some(usage) = event.usage {
                    add_usage(&mut self.usage, &usage);
                }
            }
            Event::ToolUseFailed(event) => {
                if let Some(record) = self.tool_uses.get_mut(&event.tool_use_id) {
                    record.error = Some(event.error);
                }
            }
            Event::PermissionRequested(event) => {
                pending_permissions.insert(
                    event.request_id,
                    PermissionRecord {
                        request_id: event.request_id,
                        tool_use_id: event.tool_use_id,
                        tool_name: event.tool_name,
                        subject: event.subject,
                        decision: None,
                        scope: event.scope_hint,
                    },
                );
            }
            Event::PermissionResolved(event) => {
                let mut record =
                    pending_permissions
                        .remove(&event.request_id)
                        .unwrap_or(PermissionRecord {
                            request_id: event.request_id,
                            tool_use_id: ToolUseId::from_u128(0),
                            tool_name: String::new(),
                            subject: PermissionSubject::Custom {
                                kind: "unknown".to_owned(),
                                payload: serde_json::Value::Null,
                            },
                            decision: None,
                            scope: event.scope.clone(),
                        });
                record.decision = Some(event.decision.clone());
                record.scope = event.scope;
                if matches!(
                    event.decision,
                    Decision::AllowSession | Decision::AllowPermanent
                ) {
                    self.allowlist.insert(permission_scope_key(&record.scope));
                }
                self.permission_log.push(record);
            }
            Event::RunEnded(event) => {
                if let Some(usage) = event.usage {
                    add_usage(&mut self.usage, &usage);
                }
            }
            Event::SessionEnded(event) => {
                self.end_reason = Some(event.reason);
                self.usage = event.final_usage;
            }
            Event::ToolSchemaMaterialized(event) => {
                self.remove_pending_deferred_added_names(&event.names);
                self.discovered_tools.materialized.extend(event.names);
            }
            Event::ToolDeferredPoolChanged(event) => {
                for name in &event.removed {
                    self.discovered_tools.materialized.remove(name);
                }
                let mut delta = DeferredToolsDeltaAttachment::from_pool_change(&event);
                delta
                    .added_names
                    .retain(|name| !self.discovered_tools.materialized.contains(name));
                if !delta.is_empty() {
                    match &mut self.pending_deferred_tools_delta {
                        Some(existing) => existing.merge(delta),
                        None => self.pending_deferred_tools_delta = Some(delta),
                    }
                }
            }
            Event::CompactionApplied(_) => {
                self.discovered_tools.materialized.clear();
            }
            _ => {}
        }
    }

    pub(crate) fn apply_events(&mut self, events: &[Event]) {
        let mut pending_permissions = HashMap::<RequestId, PermissionRecord>::new();
        for event in events {
            self.apply_event(event.clone(), &mut pending_permissions);
        }
        self.refresh_snapshot_id();
    }

    pub(crate) fn refresh_snapshot_id(&mut self) {
        self.snapshot_id = crate::snapshot::projection_snapshot_id(self);
    }

    #[must_use]
    pub fn pending_deferred_tools_delta(&self) -> Option<&DeferredToolsDeltaAttachment> {
        self.pending_deferred_tools_delta.as_ref()
    }

    pub fn take_pending_deferred_tools_delta(&mut self) -> Option<DeferredToolsDeltaAttachment> {
        let delta = self.pending_deferred_tools_delta.take();
        if delta.is_some() {
            self.refresh_snapshot_id();
        }
        delta
    }

    fn remove_pending_deferred_added_names(&mut self, names: &[ToolName]) {
        if let Some(delta) = &mut self.pending_deferred_tools_delta {
            delta.remove_added_names(names);
            if delta.is_empty() {
                self.pending_deferred_tools_delta = None;
            }
        }
    }
}

fn message_parts(content: MessageContent) -> Vec<MessagePart> {
    match content {
        MessageContent::Text(text) => vec![MessagePart::Text(text)],
        MessageContent::Structured(value) => vec![MessagePart::Text(value.to_string())],
        MessageContent::Multimodal(parts) => parts,
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

fn permission_scope_key(scope: &DecisionScope) -> String {
    serde_json::to_string(scope).unwrap_or_else(|_| format!("{scope:?}"))
}
