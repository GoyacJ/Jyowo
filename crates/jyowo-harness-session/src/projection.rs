use std::collections::{BTreeMap, BTreeSet, HashMap};

use harness_contracts::{
    ContentHash, ConversationContextReference, Decision, DecisionScope,
    DeferredToolsDeltaAttachment, DenyReason, EndReason, Event, JournalOffset, Message,
    MessageContent, MessageId, MessagePart, MessageRole, PermissionDecisionOption,
    PermissionSubject, RequestId, RunId, SessionError, SessionId, SnapshotId, TenantId,
    ToolErrorPayload, ToolName, ToolResult, ToolUseId, UsageSnapshot,
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
    #[serde(default)]
    pub skill_context_deliveries: BTreeMap<String, SkillContextDeliveryRecord>,
    pending_deferred_tools_delta: Option<DeferredToolsDeltaAttachment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillContextDeliveryStage {
    Prepared,
    ContextAssembled,
    ProviderAccepted,
    Consumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillContextDeliveryRecord {
    pub delivery_key: String,
    pub reference: ConversationContextReference,
    pub body_hash: ContentHash,
    pub stage: SkillContextDeliveryStage,
    pub prepared_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub run_id: RunId,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decision_options: Vec<PermissionDecisionOption>,
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
            skill_context_deliveries: BTreeMap::new(),
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
            state.apply_event(envelope.payload, &mut pending_permissions)?;
        }
        state.refresh_snapshot_id();
        Ok(state)
    }

    fn apply_event(
        &mut self,
        event: Event,
        pending_permissions: &mut HashMap<RequestId, PermissionRecord>,
    ) -> Result<(), SessionError> {
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
            Event::ToolUseDenied(event) => {
                if let Some(record) = self.tool_uses.get_mut(&event.tool_use_id) {
                    record.error.get_or_insert_with(|| ToolErrorPayload {
                        code: "permission_denied".to_owned(),
                        message: denied_tool_result_message(&event.reason),
                        retriable: false,
                    });
                }
            }
            Event::SandboxPreflightFailed(event) => {
                if let Some(record) = event
                    .tool_use_id
                    .and_then(|tool_use_id| self.tool_uses.get_mut(&tool_use_id))
                {
                    record.error = Some(ToolErrorPayload {
                        code: "sandbox_preflight_failed".to_owned(),
                        message: format!("sandbox preflight failed: {}", event.reason),
                        retriable: false,
                    });
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
                        decision_options: event.presented_options,
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
                            decision_options: Vec::new(),
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
            Event::SkillContextPrepared(event) => {
                if event.session_id != self.session_id {
                    return Err(projection_error(
                        "skill context prepared event has a different session id",
                    ));
                }
                if !matches!(&event.reference, ConversationContextReference::Skill { .. }) {
                    return Err(projection_error(
                        "skill context prepared event requires a skill reference",
                    ));
                }
                if self
                    .skill_context_deliveries
                    .contains_key(&event.delivery_key)
                {
                    return Err(projection_error(
                        "skill context delivery was prepared more than once",
                    ));
                }
                self.skill_context_deliveries.insert(
                    event.delivery_key.clone(),
                    SkillContextDeliveryRecord {
                        delivery_key: event.delivery_key,
                        reference: event.reference,
                        body_hash: event.body_hash,
                        stage: SkillContextDeliveryStage::Prepared,
                        prepared_at: event.at,
                        updated_at: event.at,
                        run_id: event.run_id,
                    },
                );
            }
            Event::SkillContextAssembled(event) => self.advance_skill_context_delivery(
                event.session_id,
                &event.delivery_key,
                SkillContextDeliveryStage::Prepared,
                SkillContextDeliveryStage::ContextAssembled,
                event.run_id,
                event.at,
            )?,
            Event::SkillContextProviderAccepted(event) => self.advance_skill_context_delivery(
                event.session_id,
                &event.delivery_key,
                SkillContextDeliveryStage::ContextAssembled,
                SkillContextDeliveryStage::ProviderAccepted,
                event.run_id,
                event.at,
            )?,
            Event::SkillContextConsumed(event) => self.advance_skill_context_delivery(
                event.session_id,
                &event.delivery_key,
                SkillContextDeliveryStage::ProviderAccepted,
                SkillContextDeliveryStage::Consumed,
                event.run_id,
                event.at,
            )?,
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn apply_events(&mut self, events: &[Event]) {
        let mut pending_permissions = HashMap::<RequestId, PermissionRecord>::new();
        for event in events {
            // Task-backed stores reject invalid skill delivery transitions before
            // committing them. Keep live projection updates infallible for the
            // existing session API while replay remains strict.
            let _ = self.apply_event(event.clone(), &mut pending_permissions);
        }
        self.refresh_snapshot_id();
    }

    /// Rebuilds provider-facing history with a result for every historical tool call.
    ///
    /// `messages` remains the visible conversation transcript. Tool outcomes are
    /// stored separately in `tool_uses`, so they must be reinserted before the
    /// transcript is sent back to a model on a later turn.
    #[must_use]
    pub fn model_context_messages(&self) -> Vec<Message> {
        let mut context =
            Vec::with_capacity(self.messages.len().saturating_add(self.tool_uses.len()));
        for message in &self.messages {
            context.push(message.clone());
            if message.role != MessageRole::Assistant {
                continue;
            }
            for part in &message.parts {
                let MessagePart::ToolUse { id, .. } = part else {
                    continue;
                };
                context.push(self.model_tool_result_message(*id, message.created_at));
            }
        }
        context
    }

    fn model_tool_result_message(
        &self,
        tool_use_id: ToolUseId,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Message {
        let content = self
            .tool_uses
            .get(&tool_use_id)
            .and_then(|record| {
                record.result.clone().or_else(|| {
                    record
                        .error
                        .as_ref()
                        .map(|error| ToolResult::Text(error.message.clone()))
                })
            })
            .unwrap_or_else(|| {
                ToolResult::Text(
                    "tool execution did not complete before the previous run ended".to_owned(),
                )
            });
        Message {
            id: tool_result_message_id(tool_use_id),
            role: MessageRole::Tool,
            parts: vec![MessagePart::ToolResult {
                tool_use_id,
                content,
            }],
            created_at,
        }
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

    #[must_use]
    pub fn skill_context_delivery(
        &self,
        delivery_key: &str,
    ) -> Option<&SkillContextDeliveryRecord> {
        self.skill_context_deliveries.get(delivery_key)
    }

    pub fn unconsumed_skill_context_deliveries(
        &self,
    ) -> impl Iterator<Item = &SkillContextDeliveryRecord> {
        self.skill_context_deliveries
            .values()
            .filter(|delivery| delivery.stage != SkillContextDeliveryStage::Consumed)
    }

    fn advance_skill_context_delivery(
        &mut self,
        session_id: SessionId,
        delivery_key: &str,
        required: SkillContextDeliveryStage,
        next: SkillContextDeliveryStage,
        run_id: RunId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), SessionError> {
        if session_id != self.session_id {
            return Err(projection_error(
                "skill context lifecycle event has a different session id",
            ));
        }
        let delivery = self
            .skill_context_deliveries
            .get_mut(delivery_key)
            .ok_or_else(|| projection_error("skill context delivery was not prepared"))?;
        if delivery.stage == next
            && matches!(
                next,
                SkillContextDeliveryStage::ContextAssembled
                    | SkillContextDeliveryStage::ProviderAccepted
            )
        {
            delivery.run_id = run_id;
            delivery.updated_at = at;
            return Ok(());
        }
        if delivery.stage != required {
            return Err(projection_error(format!(
                "skill context delivery cannot advance from {:?} to {next:?}",
                delivery.stage
            )));
        }
        delivery.stage = next;
        delivery.run_id = run_id;
        delivery.updated_at = at;
        Ok(())
    }
}

fn projection_error(message: impl Into<String>) -> SessionError {
    SessionError::Message(message.into())
}

fn tool_result_message_id(tool_use_id: ToolUseId) -> MessageId {
    MessageId::from_u128(u128::from_be_bytes(tool_use_id.as_bytes()))
}

fn denied_tool_result_message(reason: &DenyReason) -> String {
    match reason {
        DenyReason::UserDenied => "tool use denied by user".to_owned(),
        DenyReason::RuleDenied => "tool use denied by rule".to_owned(),
        DenyReason::DefaultModeDenied => "tool use denied by permission mode".to_owned(),
        DenyReason::HookBlocked { handler_id } => {
            format!("tool use blocked by hook `{handler_id}`")
        }
        DenyReason::SubagentBlocked => "tool use denied for subagent".to_owned(),
        DenyReason::PolicyDenied => "tool use denied by runtime policy".to_owned(),
        DenyReason::Other(message) => format!("tool use denied: {message}"),
        _ => "tool use denied".to_owned(),
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
