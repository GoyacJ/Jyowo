//! Conversation read model projection store.

use std::path::Path;

use chrono::{DateTime, Utc};
use harness_contracts::{
    ArtifactStatus, ConversationCursor, ConversationMessage, ConversationMessageAuthor,
    ConversationSnapshot, ConversationSummary, ConversationTimelineEvent, ConversationTimelinePage,
    Decision, DecisionScope, DeltaChunk, Event, EventId, JournalError, MessageContent, MessagePart,
    PermissionSubject, RequestId, RunId, SessionId, Severity, TenantId, ToolUseId, UiSafeText,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::{journal_error, EventEnvelope, SessionSummary};

pub struct SqliteConversationReadModelStore {
    connection: Mutex<Connection>,
}

impl SqliteConversationReadModelStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(journal_error)?;
        }
        let connection = Connection::open(path).map_err(journal_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA busy_timeout = 5000;
                 CREATE TABLE IF NOT EXISTS conversation_projection_state (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    last_event_id TEXT,
                    last_offset INTEGER NOT NULL DEFAULT -1,
                    last_conversation_sequence INTEGER NOT NULL DEFAULT 0,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS conversation_summary (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    last_message_preview TEXT,
                    updated_at TEXT NOT NULL,
                    is_empty INTEGER NOT NULL,
                    model_config_id TEXT,
                    last_event_id TEXT,
                    last_conversation_sequence INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (tenant_id, session_id)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS conversation_message (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    message_id TEXT NOT NULL,
                    author TEXT NOT NULL,
                    body TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    client_message_id TEXT,
                    conversation_sequence INTEGER NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, message_id)
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS idx_conversation_message_order
                    ON conversation_message(tenant_id, session_id, conversation_sequence);
                 CREATE TABLE IF NOT EXISTS conversation_timeline_event (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    event_id TEXT NOT NULL,
                    conversation_sequence INTEGER NOT NULL,
                    run_id TEXT NOT NULL,
                    run_sequence INTEGER NOT NULL,
                    event_type TEXT NOT NULL,
                    source TEXT NOT NULL,
                    visibility TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, event_id)
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS idx_conversation_timeline_order
                    ON conversation_timeline_event(tenant_id, session_id, conversation_sequence);
                 CREATE INDEX IF NOT EXISTS idx_conversation_timeline_run
                    ON conversation_timeline_event(tenant_id, session_id, run_id, run_sequence);
                 CREATE TABLE IF NOT EXISTS conversation_projection_tool_context (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    tool_use_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, tool_use_id)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS conversation_projection_permission_context (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    request_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, request_id)
                 ) STRICT;",
            )
            .map_err(journal_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub async fn apply_envelopes(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        envelopes: &[EventEnvelope],
        model_config_id: Option<&str>,
    ) -> Result<(), JournalError> {
        let mut connection = self.connection.lock().await;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(journal_error)?;
        for envelope in envelopes {
            if event_exists(&tx, tenant_id, session_id, envelope.event_id)? {
                continue;
            }
            let conversation_sequence = next_conversation_sequence(&tx, tenant_id, session_id)?;
            let Some(projected) =
                project_envelope(&tx, tenant_id, session_id, envelope, conversation_sequence)?
            else {
                if matches!(envelope.payload, Event::SessionCreated(_)) {
                    touch_summary_event(
                        &tx,
                        tenant_id,
                        session_id,
                        envelope.event_id,
                        envelope.recorded_at,
                        conversation_sequence.saturating_sub(1),
                        model_config_id,
                    )?;
                }
                upsert_projection_state(
                    &tx,
                    tenant_id,
                    session_id,
                    envelope,
                    conversation_sequence.saturating_sub(1),
                )?;
                continue;
            };
            insert_timeline_event(&tx, tenant_id, session_id, &projected)?;
            record_projection_contexts(&tx, tenant_id, session_id, &projected)?;
            if let Some(message) = projected.message.as_ref() {
                insert_message(&tx, tenant_id, session_id, message)?;
                upsert_summary(
                    &tx,
                    tenant_id,
                    session_id,
                    message,
                    envelope.event_id,
                    conversation_sequence,
                    model_config_id,
                )?;
            } else {
                touch_summary_event(
                    &tx,
                    tenant_id,
                    session_id,
                    envelope.event_id,
                    envelope.recorded_at,
                    conversation_sequence,
                    model_config_id,
                )?;
            }
            upsert_projection_state(&tx, tenant_id, session_id, envelope, conversation_sequence)?;
        }
        tx.commit().map_err(journal_error)
    }

    pub async fn list_summaries(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, JournalError> {
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare(
                "SELECT session_id, title, last_message_preview, updated_at, is_empty,
                        model_config_id, last_event_id, last_conversation_sequence
                 FROM conversation_summary
                 WHERE tenant_id = ?1
                 ORDER BY updated_at DESC
                 LIMIT ?2",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(
                params![tenant_id.to_string(), limit.clamp(1, 200) as i64],
                |row| summary_from_row(row),
            )
            .map_err(journal_error)?;
        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(row.map_err(journal_error)?);
        }
        Ok(summaries)
    }

    pub async fn seed_empty_summary(
        &self,
        tenant_id: TenantId,
        summary: &SessionSummary,
        model_config_id: Option<&str>,
    ) -> Result<(), JournalError> {
        self.seed_empty_conversation(
            tenant_id,
            summary.session_id,
            summary.last_event_at,
            model_config_id,
        )
        .await
    }

    pub async fn seed_empty_conversation(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        updated_at: DateTime<Utc>,
        model_config_id: Option<&str>,
    ) -> Result<(), JournalError> {
        let connection = self.connection.lock().await;
        connection
            .execute(
                "INSERT INTO conversation_summary (
                    tenant_id, session_id, title, last_message_preview, updated_at, is_empty,
                    model_config_id, last_event_id, last_conversation_sequence
                 )
                 VALUES (?1, ?2, ?3, NULL, ?4, 1, ?5, NULL, 0)
                 ON CONFLICT(tenant_id, session_id) DO NOTHING",
                params![
                    tenant_id.to_string(),
                    session_id.to_string(),
                    UiSafeText::from_trusted_redacted("New conversation").as_str(),
                    updated_at.to_rfc3339(),
                    model_config_id,
                ],
            )
            .map_err(journal_error)?;
        Ok(())
    }

    pub async fn reset_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), JournalError> {
        let mut connection = self.connection.lock().await;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(journal_error)?;
        for table in [
            "conversation_projection_permission_context",
            "conversation_projection_tool_context",
            "conversation_timeline_event",
            "conversation_message",
            "conversation_summary",
            "conversation_projection_state",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE tenant_id = ?1 AND session_id = ?2"),
                params![tenant_id.to_string(), session_id.to_string()],
            )
            .map_err(journal_error)?;
        }
        tx.commit().map_err(journal_error)
    }

    pub async fn projection_cursor(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<Option<ConversationCursor>, JournalError> {
        let connection = self.connection.lock().await;
        connection
            .query_row(
                "SELECT last_event_id, last_conversation_sequence
                 FROM conversation_projection_state
                 WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant_id.to_string(), session_id.to_string()],
                |row| {
                    let event_id: String = row.get(0)?;
                    let conversation_sequence = row.get::<_, i64>(1)? as u64;
                    Ok(ConversationCursor {
                        event_id: EventId::parse(&event_id).map_err(|error| {
                            rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                        })?,
                        conversation_sequence,
                    })
                },
            )
            .optional()
            .map_err(journal_error)
    }

    pub async fn summary(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<Option<ConversationSummary>, JournalError> {
        let connection = self.connection.lock().await;
        connection
            .query_row(
                "SELECT session_id, title, last_message_preview, updated_at, is_empty,
                        model_config_id, last_event_id, last_conversation_sequence
                 FROM conversation_summary
                 WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant_id.to_string(), session_id.to_string()],
                summary_from_row,
            )
            .optional()
            .map_err(journal_error)
    }

    pub async fn snapshot(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        message_limit: usize,
    ) -> Result<Option<ConversationSnapshot>, JournalError> {
        let summary = self.summary(tenant_id, session_id).await?;
        let Some(summary) = summary else {
            return Ok(None);
        };
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare(
                "SELECT message_id, author, body, timestamp, client_message_id,
                        conversation_sequence
                 FROM conversation_message
                 WHERE tenant_id = ?1 AND session_id = ?2
                 ORDER BY conversation_sequence ASC
                 LIMIT ?3",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(
                params![
                    tenant_id.to_string(),
                    session_id.to_string(),
                    message_limit.clamp(1, 1000) as i64
                ],
                message_from_row,
            )
            .map_err(journal_error)?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row.map_err(journal_error)?);
        }
        Ok(Some(ConversationSnapshot {
            id: summary.id,
            messages,
            model_config_id: summary.model_config_id,
            title: summary.title,
            updated_at: summary.updated_at,
            cursor: summary.cursor,
        }))
    }

    pub async fn page_timeline(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        after_cursor: Option<ConversationCursor>,
        limit: usize,
    ) -> Result<ConversationTimelinePage, JournalError> {
        let connection = self.connection.lock().await;
        if let Some(cursor) = after_cursor {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM conversation_timeline_event
                     WHERE tenant_id = ?1 AND session_id = ?2 AND event_id = ?3
                       AND conversation_sequence = ?4",
                    params![
                        tenant_id.to_string(),
                        session_id.to_string(),
                        cursor.event_id.to_string(),
                        cursor.conversation_sequence as i64
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(journal_error)?
                .is_some();
            if !exists {
                return Err(journal_error("conversation cursor is unknown"));
            }
        }
        let after_sequence =
            after_cursor.map_or(0_i64, |cursor| cursor.conversation_sequence as i64);
        let mut statement = connection
            .prepare(
                "SELECT event_id, conversation_sequence, run_id, run_sequence, event_type,
                        source, visibility, timestamp, payload
                 FROM conversation_timeline_event
                 WHERE tenant_id = ?1 AND session_id = ?2 AND conversation_sequence > ?3
                 ORDER BY conversation_sequence ASC
                 LIMIT ?4",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(
                params![
                    tenant_id.to_string(),
                    session_id.to_string(),
                    after_sequence,
                    limit.clamp(1, 200) as i64
                ],
                timeline_event_from_row,
            )
            .map_err(journal_error)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(journal_error)?);
        }
        let cursor = events.last().map(|event| event.cursor);
        Ok(ConversationTimelinePage {
            events,
            cursor,
            gap: false,
        })
    }
}

struct ProjectedEnvelope {
    timeline_event: ConversationTimelineEvent,
    message: Option<ConversationMessage>,
    tool_context: Option<(ToolUseId, RunId)>,
    permission_context: Option<(RequestId, RunId)>,
}

fn project_envelope(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    envelope: &EventEnvelope,
    conversation_sequence: u64,
) -> Result<Option<ProjectedEnvelope>, JournalError> {
    let Some(run_id) = event_run_id(tx, tenant_id, session_id, &envelope.payload)? else {
        return Ok(None);
    };
    let mut tool_context = None;
    let mut permission_context = None;
    let (event_type, source, visibility, payload, message, timestamp) = match &envelope.payload {
        Event::RunStarted(event) => (
            "run.started",
            "engine",
            "public",
            json!({ "sessionId": event.session_id.to_string() }),
            None,
            event.started_at,
        ),
        Event::RunEnded(event) => (
            "run.ended",
            "engine",
            "public",
            json!({ "reason": run_end_reason_label(&event.reason) }),
            None,
            event.ended_at,
        ),
        Event::UserMessageAppended(event) => {
            let body = message_content_display(&event.content);
            let message = ConversationMessage {
                author: ConversationMessageAuthor::User,
                body: body.clone(),
                client_message_id: event
                    .metadata
                    .labels
                    .get("clientMessageId")
                    .filter(|value| is_uuid_v4_like(value))
                    .cloned(),
                id: event.message_id.to_string(),
                timestamp: event.at,
                conversation_sequence,
            };
            (
                "user.message.appended",
                "user",
                "public",
                json!({
                    "messageId": event.message_id.to_string(),
                    "body": body.as_str(),
                    "clientMessageId": message.client_message_id,
                }),
                Some(message),
                event.at,
            )
        }
        Event::AssistantDeltaProduced(event) => match &event.delta {
            DeltaChunk::Text(text) => (
                "assistant.delta",
                "assistant",
                "public",
                json!({ "text": safe_text(text).as_str() }),
                None,
                event.at,
            ),
            DeltaChunk::Thought(thought) => (
                "assistant.thinking.delta",
                "assistant",
                "public",
                json!({ "text": safe_text(thought.text.clone().unwrap_or_default()).as_str() }),
                None,
                event.at,
            ),
            DeltaChunk::ToolUseStart { .. }
            | DeltaChunk::ToolUseInputDelta { .. }
            | DeltaChunk::ToolUseEnd { .. } => return Ok(None),
            _ => return Ok(None),
        },
        Event::AssistantMessageCompleted(event) => {
            let body = message_content_display(&event.content);
            let message = ConversationMessage {
                author: ConversationMessageAuthor::Assistant,
                body: body.clone(),
                client_message_id: None,
                id: event.message_id.to_string(),
                timestamp: event.at,
                conversation_sequence,
            };
            (
                "assistant.completed",
                "assistant",
                "public",
                json!({
                    "messageId": event.message_id.to_string(),
                    "body": body.as_str(),
                }),
                Some(message),
                event.at,
            )
        }
        Event::ArtifactCreated(event) => (
            "artifact.created",
            "engine",
            "public",
            json!({
                "artifactId": event.artifact_id,
                "status": artifact_status_label(event.status),
            }),
            None,
            event.at,
        ),
        Event::ArtifactUpdated(event) => {
            let mut payload = json!({ "artifactId": event.artifact_id });
            if let Some(status) = event.status {
                payload["status"] = json!(artifact_status_label(status));
            }
            (
                "artifact.updated",
                "engine",
                "public",
                payload,
                None,
                event.at,
            )
        }
        Event::ToolUseRequested(event) => {
            tool_context = Some((event.tool_use_id, event.run_id));
            (
                "tool.requested",
                "tool",
                "redacted",
                json!({
                    "argumentsSummary": "Input withheld from conversation timeline.",
                    "toolName": safe_text(&event.tool_name).as_str(),
                    "toolUseId": event.tool_use_id.to_string(),
                }),
                None,
                event.at,
            )
        }
        Event::ToolUseApproved(event) => (
            "tool.approved",
            "tool",
            "public",
            json!({ "toolUseId": event.tool_use_id.to_string() }),
            None,
            event.at,
        ),
        Event::ToolUseDenied(event) => (
            "tool.denied",
            "tool",
            "public",
            json!({ "toolUseId": event.tool_use_id.to_string() }),
            None,
            event.at,
        ),
        Event::ToolUseCompleted(event) => (
            "tool.completed",
            "tool",
            "redacted",
            json!({
                "durationMs": event.duration_ms,
                "outputSummary": "Output withheld from conversation timeline.",
                "toolUseId": event.tool_use_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::ToolUseFailed(event) => (
            "tool.failed",
            "tool",
            "redacted",
            json!({
                "code": "tool_error",
                "message": "Tool error withheld from conversation timeline.",
                "toolUseId": event.tool_use_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::PermissionRequested(event) => {
            permission_context = Some((event.request_id, event.run_id));
            let subject = permission_subject_display(&event.subject);
            (
                "permission.requested",
                "policy",
                "public",
                json!({
                    "decisionScope": decision_scope_display(&event.scope_hint),
                    "exposure": subject.exposure,
                    "operation": subject.operation,
                    "reason": "The runtime requires approval before continuing.",
                    "requestId": event.request_id.to_string(),
                    "severity": severity_label(event.severity),
                    "target": subject.target,
                    "workspaceBoundary": "current workspace",
                }),
                None,
                event.at,
            )
        }
        Event::PermissionResolved(event) => (
            "permission.resolved",
            "policy",
            "public",
            json!({
                "decision": permission_decision_label(&event.decision),
                "requestId": event.request_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::EngineFailed(event) => (
            "engine.failed",
            "engine",
            "redacted",
            json!({ "message": "Engine error withheld from conversation timeline." }),
            None,
            event.at,
        ),
        _ => return Ok(None),
    };
    Ok(Some(ProjectedEnvelope {
        timeline_event: ConversationTimelineEvent {
            id: envelope.event_id.to_string(),
            cursor: ConversationCursor {
                event_id: envelope.event_id,
                conversation_sequence,
            },
            payload,
            run_id: run_id.to_string(),
            sequence: 0,
            source: source.to_owned(),
            timestamp,
            event_type: event_type.to_owned(),
            visibility: visibility.to_owned(),
        },
        message,
        tool_context,
        permission_context,
    }))
}

fn event_run_id(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    event: &Event,
) -> Result<Option<RunId>, JournalError> {
    Ok(match event {
        Event::RunStarted(event) => Some(event.run_id),
        Event::RunEnded(event) => Some(event.run_id),
        Event::UserMessageAppended(event) => Some(event.run_id),
        Event::AssistantDeltaProduced(event) => Some(event.run_id),
        Event::AssistantMessageCompleted(event) => Some(event.run_id),
        Event::ArtifactCreated(event) => Some(event.run_id),
        Event::ArtifactUpdated(event) => Some(event.run_id),
        Event::ToolUseRequested(event) => Some(event.run_id),
        Event::ToolUseApproved(event) => {
            tool_context_run_id(tx, tenant_id, session_id, event.tool_use_id)?
        }
        Event::ToolUseDenied(event) => {
            tool_context_run_id(tx, tenant_id, session_id, event.tool_use_id)?
        }
        Event::ToolUseCompleted(event) => {
            tool_context_run_id(tx, tenant_id, session_id, event.tool_use_id)?
        }
        Event::ToolUseFailed(event) => {
            tool_context_run_id(tx, tenant_id, session_id, event.tool_use_id)?
        }
        Event::PermissionRequested(event) => Some(event.run_id),
        Event::PermissionResolved(event) => {
            permission_context_run_id(tx, tenant_id, session_id, event.request_id)?
        }
        Event::EngineFailed(event) => event.run_id,
        _ => None,
    })
}

fn message_content_display(content: &MessageContent) -> UiSafeText {
    let value = match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Structured(value) => value.to_string(),
        MessageContent::Multimodal(parts) => parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    safe_text(value)
}

fn safe_text(value: impl AsRef<str>) -> UiSafeText {
    UiSafeText::from_redacted_display(value, &harness_contracts::NoopRedactor)
}

fn artifact_status_label(status: ArtifactStatus) -> &'static str {
    match status {
        ArtifactStatus::Pending => "pending",
        ArtifactStatus::Running => "running",
        ArtifactStatus::Ready => "ready",
        ArtifactStatus::Failed => "failed",
        _ => "pending",
    }
}

fn permission_decision_label(decision: &Decision) -> &'static str {
    match decision {
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => "approve",
        Decision::DenyOnce | Decision::DenyPermanent | Decision::Escalate => "deny",
        _ => "deny",
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Info | Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
        _ => "medium",
    }
}

struct PermissionSubjectDisplay {
    exposure: String,
    operation: String,
    target: String,
}

fn permission_subject_display(subject: &PermissionSubject) -> PermissionSubjectDisplay {
    match subject {
        PermissionSubject::CommandExec { command, .. } => PermissionSubjectDisplay {
            exposure: "Can execute a command inside the workspace boundary.".to_owned(),
            operation: "Execute command".to_owned(),
            target: safe_command_label(command),
        },
        PermissionSubject::ToolInvocation { tool, .. } => PermissionSubjectDisplay {
            exposure: "Can invoke a runtime tool.".to_owned(),
            operation: "Use tool".to_owned(),
            target: safe_text(tool).as_str().to_owned(),
        },
        PermissionSubject::FileWrite { path, .. } => PermissionSubjectDisplay {
            exposure: "Can write a file in the workspace.".to_owned(),
            operation: "Write file".to_owned(),
            target: safe_path_label(path),
        },
        PermissionSubject::FileDelete { path } => PermissionSubjectDisplay {
            exposure: "Can delete a file in the workspace.".to_owned(),
            operation: "Delete file".to_owned(),
            target: safe_path_label(path),
        },
        PermissionSubject::NetworkAccess { host, port } => PermissionSubjectDisplay {
            exposure: "Can access a network endpoint.".to_owned(),
            operation: "Access network".to_owned(),
            target: safe_text(port.map_or_else(|| host.clone(), |port| format!("{host}:{port}")))
                .as_str()
                .to_owned(),
        },
        PermissionSubject::DangerousCommand { command, .. } => PermissionSubjectDisplay {
            exposure: "Can execute a dangerous command.".to_owned(),
            operation: "Execute dangerous command".to_owned(),
            target: safe_command_label(command),
        },
        PermissionSubject::McpToolCall { server, tool, .. } => PermissionSubjectDisplay {
            exposure: "Can invoke an MCP tool.".to_owned(),
            operation: "Use MCP tool".to_owned(),
            target: safe_text(format!("{server}/{tool}")).as_str().to_owned(),
        },
        PermissionSubject::Custom { kind, .. } => PermissionSubjectDisplay {
            exposure: "Can perform a custom permission-gated operation.".to_owned(),
            operation: "Review custom operation".to_owned(),
            target: safe_text(kind).as_str().to_owned(),
        },
        _ => PermissionSubjectDisplay {
            exposure: "Can continue a permission-gated operation.".to_owned(),
            operation: "Review permission".to_owned(),
            target: "runtime operation".to_owned(),
        },
    }
}

fn decision_scope_display(scope: &DecisionScope) -> String {
    match scope {
        DecisionScope::ExactCommand { .. } => "this exact command".to_owned(),
        DecisionScope::ExactArgs(_) => "these exact command arguments".to_owned(),
        DecisionScope::ToolName(_) => "this tool".to_owned(),
        DecisionScope::Category(_) => "this tool category".to_owned(),
        DecisionScope::PathPrefix(_) => "this workspace path scope".to_owned(),
        DecisionScope::GlobPattern(_) => "this workspace glob".to_owned(),
        DecisionScope::ExecuteCodeScript { .. } => "execute code script".to_owned(),
        DecisionScope::Any => "any matching operation".to_owned(),
        _ => "current operation".to_owned(),
    }
}

fn safe_command_label(command: &str) -> String {
    let executable_token = command.split_whitespace().next().unwrap_or(command);
    let executable = Path::new(executable_token)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(executable_token);
    safe_text(executable).as_str().to_owned()
}

fn safe_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(
            || "workspace file".to_owned(),
            |name| format!("workspace file: {}", safe_text(name).as_str()),
        )
}

fn run_end_reason_label(reason: &harness_contracts::EndReason) -> &'static str {
    match reason {
        harness_contracts::EndReason::Completed => "completed",
        harness_contracts::EndReason::MaxIterationsReached => "max iterations reached",
        harness_contracts::EndReason::TokenBudgetExhausted => "token budget exhausted",
        harness_contracts::EndReason::BudgetExhausted(_) => "budget exhausted",
        harness_contracts::EndReason::Interrupted => "interrupted",
        harness_contracts::EndReason::Cancelled { .. } => "cancelled",
        harness_contracts::EndReason::Compacted => "compacted",
        harness_contracts::EndReason::Error(_) => "error",
        _ => "ended",
    }
}

fn event_exists(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    event_id: EventId,
) -> Result<bool, JournalError> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM conversation_timeline_event
             WHERE tenant_id = ?1 AND session_id = ?2 AND event_id = ?3",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                event_id.to_string()
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(journal_error)?
        .is_some())
}

fn next_conversation_sequence(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
) -> Result<u64, JournalError> {
    let value: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(conversation_sequence), 0) + 1
             FROM conversation_timeline_event
             WHERE tenant_id = ?1 AND session_id = ?2",
            params![tenant_id.to_string(), session_id.to_string()],
            |row| row.get(0),
        )
        .map_err(journal_error)?;
    Ok(value as u64)
}

fn insert_timeline_event(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    event: &ProjectedEnvelope,
) -> Result<(), JournalError> {
    let run_sequence = next_run_sequence(tx, tenant_id, session_id, &event.timeline_event.run_id)?;
    tx.execute(
        "INSERT INTO conversation_timeline_event (
            tenant_id, session_id, event_id, conversation_sequence, run_id, run_sequence,
            event_type, source, visibility, timestamp, payload
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            tenant_id.to_string(),
            session_id.to_string(),
            event.timeline_event.id,
            event.timeline_event.cursor.conversation_sequence as i64,
            event.timeline_event.run_id,
            run_sequence as i64,
            event.timeline_event.event_type,
            event.timeline_event.source,
            event.timeline_event.visibility,
            event.timeline_event.timestamp.to_rfc3339(),
            serde_json::to_string(&event.timeline_event.payload).map_err(journal_error)?,
        ],
    )
    .map_err(journal_error)?;
    Ok(())
}

fn record_projection_contexts(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    event: &ProjectedEnvelope,
) -> Result<(), JournalError> {
    if let Some((tool_use_id, run_id)) = event.tool_context {
        tx.execute(
            "INSERT OR IGNORE INTO conversation_projection_tool_context (
                tenant_id, session_id, tool_use_id, run_id
             )
             VALUES (?1, ?2, ?3, ?4)",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                tool_use_id.to_string(),
                run_id.to_string(),
            ],
        )
        .map_err(journal_error)?;
    }

    if let Some((request_id, run_id)) = event.permission_context {
        tx.execute(
            "INSERT OR IGNORE INTO conversation_projection_permission_context (
                tenant_id, session_id, request_id, run_id
             )
             VALUES (?1, ?2, ?3, ?4)",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                request_id.to_string(),
                run_id.to_string(),
            ],
        )
        .map_err(journal_error)?;
    }

    Ok(())
}

fn tool_context_run_id(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    tool_use_id: ToolUseId,
) -> Result<Option<RunId>, JournalError> {
    let run_id = tx
        .query_row(
            "SELECT run_id FROM conversation_projection_tool_context
             WHERE tenant_id = ?1 AND session_id = ?2 AND tool_use_id = ?3",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                tool_use_id.to_string()
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(journal_error)?;
    run_id
        .map(|run_id| RunId::parse(&run_id).map_err(journal_error))
        .transpose()
}

fn permission_context_run_id(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    request_id: RequestId,
) -> Result<Option<RunId>, JournalError> {
    let run_id = tx
        .query_row(
            "SELECT run_id FROM conversation_projection_permission_context
             WHERE tenant_id = ?1 AND session_id = ?2 AND request_id = ?3",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                request_id.to_string()
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(journal_error)?;
    run_id
        .map(|run_id| RunId::parse(&run_id).map_err(journal_error))
        .transpose()
}

fn next_run_sequence(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: &str,
) -> Result<u64, JournalError> {
    let value: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(run_sequence), 0) + 1
             FROM conversation_timeline_event
             WHERE tenant_id = ?1 AND session_id = ?2 AND run_id = ?3",
            params![tenant_id.to_string(), session_id.to_string(), run_id],
            |row| row.get(0),
        )
        .map_err(journal_error)?;
    Ok(value as u64)
}

fn insert_message(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    message: &ConversationMessage,
) -> Result<(), JournalError> {
    tx.execute(
        "INSERT OR IGNORE INTO conversation_message (
            tenant_id, session_id, message_id, author, body, timestamp,
            client_message_id, conversation_sequence
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            tenant_id.to_string(),
            session_id.to_string(),
            message.id,
            message_author_label(message.author),
            message.body.as_str(),
            message.timestamp.to_rfc3339(),
            message.client_message_id,
            message.conversation_sequence as i64,
        ],
    )
    .map_err(journal_error)?;
    Ok(())
}

fn upsert_summary(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    message: &ConversationMessage,
    event_id: EventId,
    conversation_sequence: u64,
    model_config_id: Option<&str>,
) -> Result<(), JournalError> {
    let current_title: Option<String> = tx
        .query_row(
            "SELECT title FROM conversation_summary
             WHERE tenant_id = ?1 AND session_id = ?2",
            params![tenant_id.to_string(), session_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(journal_error)?;
    let title = match (current_title, message.author) {
        (Some(existing), ConversationMessageAuthor::Assistant) => existing,
        (_, ConversationMessageAuthor::User | ConversationMessageAuthor::Assistant) => {
            snippet(message.body.as_str())
        }
    };
    tx.execute(
        "INSERT INTO conversation_summary (
            tenant_id, session_id, title, last_message_preview, updated_at, is_empty,
            model_config_id, last_event_id, last_conversation_sequence
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8)
         ON CONFLICT(tenant_id, session_id) DO UPDATE SET
            title = excluded.title,
            last_message_preview = excluded.last_message_preview,
            updated_at = excluded.updated_at,
            is_empty = 0,
            model_config_id = excluded.model_config_id,
            last_event_id = excluded.last_event_id,
            last_conversation_sequence = excluded.last_conversation_sequence",
        params![
            tenant_id.to_string(),
            session_id.to_string(),
            title,
            snippet(message.body.as_str()),
            message.timestamp.to_rfc3339(),
            model_config_id,
            event_id.to_string(),
            conversation_sequence as i64,
        ],
    )
    .map_err(journal_error)?;
    Ok(())
}

fn touch_summary_event(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    event_id: EventId,
    updated_at: DateTime<Utc>,
    conversation_sequence: u64,
    model_config_id: Option<&str>,
) -> Result<(), JournalError> {
    tx.execute(
        "INSERT INTO conversation_summary (
            tenant_id, session_id, title, last_message_preview, updated_at, is_empty,
            model_config_id, last_event_id, last_conversation_sequence
         )
         VALUES (?1, ?2, 'New conversation', NULL, ?3, 1, ?4, ?5, ?6)
         ON CONFLICT(tenant_id, session_id) DO UPDATE SET
            updated_at = excluded.updated_at,
            model_config_id = excluded.model_config_id,
            last_event_id = excluded.last_event_id,
            last_conversation_sequence = excluded.last_conversation_sequence",
        params![
            tenant_id.to_string(),
            session_id.to_string(),
            updated_at.to_rfc3339(),
            model_config_id,
            event_id.to_string(),
            conversation_sequence as i64,
        ],
    )
    .map_err(journal_error)?;
    Ok(())
}

fn upsert_projection_state(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    envelope: &EventEnvelope,
    conversation_sequence: u64,
) -> Result<(), JournalError> {
    tx.execute(
        "INSERT INTO conversation_projection_state (
            tenant_id, session_id, last_event_id, last_offset,
            last_conversation_sequence, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(tenant_id, session_id) DO UPDATE SET
            last_event_id = excluded.last_event_id,
            last_offset = excluded.last_offset,
            last_conversation_sequence = excluded.last_conversation_sequence,
            updated_at = excluded.updated_at",
        params![
            tenant_id.to_string(),
            session_id.to_string(),
            envelope.event_id.to_string(),
            envelope.offset.0 as i64,
            conversation_sequence as i64,
            envelope.recorded_at.to_rfc3339(),
        ],
    )
    .map_err(journal_error)?;
    Ok(())
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSummary> {
    let last_event_id: Option<String> = row.get(6)?;
    let last_sequence: i64 = row.get(7)?;
    Ok(ConversationSummary {
        id: row.get(0)?,
        title: UiSafeText::from_trusted_redacted(row.get::<_, String>(1)?),
        last_message_preview: row
            .get::<_, Option<String>>(2)?
            .map(UiSafeText::from_trusted_redacted),
        updated_at: parse_rfc3339(row.get::<_, String>(3)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        is_empty: row.get::<_, i64>(4)? != 0,
        model_config_id: row.get(5)?,
        cursor: match last_event_id {
            Some(event_id) => Some(ConversationCursor {
                event_id: EventId::parse(&event_id)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                conversation_sequence: last_sequence as u64,
            }),
            None => None,
        },
    })
}

fn message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationMessage> {
    let author = match row.get::<_, String>(1)?.as_str() {
        "user" => ConversationMessageAuthor::User,
        "assistant" => ConversationMessageAuthor::Assistant,
        _ => ConversationMessageAuthor::Assistant,
    };
    Ok(ConversationMessage {
        id: row.get(0)?,
        author,
        body: UiSafeText::from_trusted_redacted(row.get::<_, String>(2)?),
        timestamp: parse_rfc3339(row.get::<_, String>(3)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        client_message_id: row.get(4)?,
        conversation_sequence: row.get::<_, i64>(5)? as u64,
    })
}

fn timeline_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationTimelineEvent> {
    let event_id: String = row.get(0)?;
    let conversation_sequence = row.get::<_, i64>(1)? as u64;
    Ok(ConversationTimelineEvent {
        id: event_id.clone(),
        cursor: ConversationCursor {
            event_id: EventId::parse(&event_id)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            conversation_sequence,
        },
        run_id: row.get(2)?,
        sequence: row.get::<_, i64>(3)? as u64,
        event_type: row.get(4)?,
        source: row.get(5)?,
        visibility: row.get(6)?,
        timestamp: parse_rfc3339(row.get::<_, String>(7)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        payload: serde_json::from_str::<Value>(&row.get::<_, String>(8)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
    })
}

fn parse_rfc3339(value: String) -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339(&value)?.with_timezone(&Utc))
}

fn message_author_label(author: ConversationMessageAuthor) -> &'static str {
    match author {
        ConversationMessageAuthor::User => "user",
        ConversationMessageAuthor::Assistant => "assistant",
    }
}

fn snippet(value: &str) -> String {
    let line = value
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .unwrap_or("New conversation");
    line.chars().take(96).collect()
}

fn is_uuid_v4_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes[14] == b'4'
}
