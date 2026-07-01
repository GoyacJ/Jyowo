//! Conversation read model projection store.

use std::path::Path;

use chrono::{DateTime, Utc};
use harness_contracts::{
    ArtifactSource, ArtifactStatus, BackgroundAgentId, BlobRef, ConversationAttachmentReference,
    ConversationCursor, ConversationMessage, ConversationMessageAuthor, ConversationSnapshot,
    ConversationSummary, ConversationTimelineEvent, ConversationTimelinePage,
    ConversationTurnCursor, ConversationWorktreePage, Decision, DecisionScope, DeltaChunk, Event,
    EventId, JournalError, MemberLeaveReason, MessageContent, MessagePart, PermissionActorSource,
    PermissionSubject, RequestId, RoutingPolicyKind, RunId, SessionId, Severity, SubagentStatus,
    SubagentTerminationReason, TeamTerminationReason, TenantId, ToolResult, ToolResultPart,
    ToolUseId, TopologyKind, UiSafeText,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::{
    journal_error, project_conversation_worktree_snapshot, ConversationTurnPageDirection,
    EventEnvelope, SessionSummary,
};

const CONVERSATION_READ_MODEL_PROJECTION_VERSION_KEY: &str =
    "conversation_read_model_projection_version";
const CONVERSATION_READ_MODEL_PROJECTION_VERSION: &str = "8";
const CONVERSATION_READ_MODEL_CACHE_TABLES: [&str; 7] = [
    "conversation_projection_background_context",
    "conversation_projection_permission_context",
    "conversation_projection_tool_context",
    "conversation_timeline_event",
    "conversation_message",
    "conversation_summary",
    "conversation_projection_state",
];

pub struct SqliteConversationReadModelStore {
    connection: Mutex<Connection>,
}

impl SqliteConversationReadModelStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(journal_error)?;
        }
        let mut connection = Connection::open(path).map_err(journal_error)?;
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
                    tool_name TEXT,
                    PRIMARY KEY (tenant_id, session_id, tool_use_id)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS conversation_projection_permission_context (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    request_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, request_id)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS conversation_projection_background_context (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    background_agent_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, background_agent_id)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS conversation_read_model_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                 ) STRICT;",
            )
            .map_err(journal_error)?;
        ensure_tool_context_schema(&connection)?;
        ensure_projection_version(&mut connection)?;
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
        for table in CONVERSATION_READ_MODEL_CACHE_TABLES {
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

    pub async fn page_worktree(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        page_cursor: Option<ConversationTurnCursor>,
        direction: ConversationTurnPageDirection,
        limit_turns: usize,
    ) -> Result<ConversationWorktreePage, JournalError> {
        let events = self.load_complete_timeline(tenant_id, session_id).await?;
        let projection = project_conversation_worktree_snapshot(&session_id.to_string(), events);
        let all_turns = projection.turns;
        let limit = limit_turns.clamp(1, 100);
        let boundary = match page_cursor.as_ref() {
            Some(cursor) => {
                let matches_cursor = all_turns
                    .iter()
                    .any(|turn| turn.id == cursor.turn_id && turn.position == cursor.position);
                if !matches_cursor {
                    return Err(journal_error("conversation cursor is unknown"));
                }
                Some(cursor.position)
            }
            None => None,
        };
        let selected = match direction {
            ConversationTurnPageDirection::After => all_turns
                .iter()
                .enumerate()
                .filter(|(_, turn)| boundary.is_none_or(|position| turn.position > position))
                .take(limit)
                .map(|(index, turn)| (index, turn.clone()))
                .collect::<Vec<_>>(),
            ConversationTurnPageDirection::Before => {
                let mut before = all_turns
                    .iter()
                    .enumerate()
                    .filter(|(_, turn)| boundary.is_none_or(|position| turn.position < position))
                    .collect::<Vec<_>>();
                if before.len() > limit {
                    before = before.split_off(before.len() - limit);
                }
                before
                    .into_iter()
                    .map(|(index, turn)| (index, turn.clone()))
                    .collect::<Vec<_>>()
            }
        };
        let first_index = selected.first().map(|(index, _)| *index);
        let last_index = selected.last().map(|(index, _)| *index);
        let turns = selected
            .into_iter()
            .map(|(_, turn)| turn)
            .collect::<Vec<_>>();
        let cursor_turn = match direction {
            ConversationTurnPageDirection::After => turns.last(),
            ConversationTurnPageDirection::Before => turns.first(),
        };
        let page_cursor = cursor_turn.map(|turn| ConversationTurnCursor {
            turn_id: turn.id.clone(),
            position: turn.position,
        });
        let has_more_before = first_index.is_some_and(|index| index > 0);
        let has_more_after = last_index.is_some_and(|index| index + 1 < all_turns.len());

        Ok(ConversationWorktreePage {
            turns,
            page_cursor,
            event_cursor: projection.event_cursor,
            has_more_before,
            has_more_after,
            gap: false,
        })
    }

    async fn load_complete_timeline(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<Vec<ConversationTimelineEvent>, JournalError> {
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare(
                "SELECT event_id, conversation_sequence, run_id, run_sequence, event_type,
                        source, visibility, timestamp, payload
                 FROM conversation_timeline_event
                 WHERE tenant_id = ?1 AND session_id = ?2
                 ORDER BY conversation_sequence ASC",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(
                params![tenant_id.to_string(), session_id.to_string()],
                timeline_event_from_row,
            )
            .map_err(journal_error)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(journal_error)?);
        }
        Ok(events)
    }
}

fn ensure_projection_version(connection: &mut Connection) -> Result<(), JournalError> {
    let current_version = connection
        .query_row(
            "SELECT value FROM conversation_read_model_meta WHERE key = ?1",
            params![CONVERSATION_READ_MODEL_PROJECTION_VERSION_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(journal_error)?;
    if current_version.as_deref() == Some(CONVERSATION_READ_MODEL_PROJECTION_VERSION) {
        return Ok(());
    }

    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(journal_error)?;
    for table in CONVERSATION_READ_MODEL_CACHE_TABLES {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .map_err(journal_error)?;
    }
    tx.execute(
        "INSERT INTO conversation_read_model_meta (key, value)
         VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            CONVERSATION_READ_MODEL_PROJECTION_VERSION_KEY,
            CONVERSATION_READ_MODEL_PROJECTION_VERSION
        ],
    )
    .map_err(journal_error)?;
    tx.commit().map_err(journal_error)
}

fn ensure_tool_context_schema(connection: &Connection) -> Result<(), JournalError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(conversation_projection_tool_context)")
        .map_err(journal_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(journal_error)?;
    let mut has_tool_name = false;
    for row in rows {
        if row.map_err(journal_error)? == "tool_name" {
            has_tool_name = true;
            break;
        }
    }
    if has_tool_name {
        return Ok(());
    }
    connection
        .execute(
            "ALTER TABLE conversation_projection_tool_context ADD COLUMN tool_name TEXT",
            [],
        )
        .map_err(journal_error)?;
    Ok(())
}

struct ProjectedEnvelope {
    timeline_event: ConversationTimelineEvent,
    message: Option<ConversationMessage>,
    tool_context: Option<(ToolUseId, RunId, String)>,
    permission_context: Option<(RequestId, RunId)>,
    background_context: Option<(BackgroundAgentId, RunId)>,
}

fn project_envelope(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    envelope: &EventEnvelope,
    conversation_sequence: u64,
) -> Result<Option<ProjectedEnvelope>, JournalError> {
    let run_id = event_run_id(tx, tenant_id, session_id, &envelope.payload)?.or(envelope.run_id);
    let Some(run_id) = run_id else {
        return Ok(None);
    };
    let mut tool_context = None;
    let mut permission_context = None;
    let mut background_context = None;
    let (event_type, source, visibility, payload, message, timestamp) = match &envelope.payload {
        Event::RunStarted(event) => (
            "run.started",
            "engine",
            "public",
            json!({
                "sessionId": event.session_id.to_string(),
                "permissionMode": event.permission_mode,
            }),
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
                    "attachments": attachment_references_payload(&event.attachments),
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
                json!({
                    "messageId": event.message_id.to_string(),
                    "text": safe_text(text).as_str(),
                }),
                None,
                event.at,
            ),
            DeltaChunk::Thought(_) => (
                "assistant.thinking.delta",
                "assistant",
                "public",
                json!({
                    "status": "running",
                }),
                None,
                event.at,
            ),
            DeltaChunk::ReasoningSummary(summary) => (
                "assistant.thinking.delta",
                "assistant",
                "public",
                json!({
                    "status": "running",
                    "safeSummaryDelta": safe_text(&summary.text).as_str(),
                }),
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
                    "toolUses": event.tool_uses.iter().map(|tool_use| {
                        json!({
                            "toolUseId": tool_use.tool_use_id.to_string(),
                            "toolName": safe_text(&tool_use.tool_name).as_str(),
                        })
                    }).collect::<Vec<_>>(),
                }),
                Some(message),
                event.at,
            )
        }
        Event::ArtifactCreated(event) => {
            let public_kind = safe_text(&event.kind).into_string();
            let mut payload = json!({
                "artifactId": event.artifact_id,
                "kind": public_kind,
                "status": artifact_status_label(event.status),
                "source": artifact_source_label(event.source),
                "title": safe_text(&event.title).into_string(),
            });
            if let Some(preview) = event.preview.as_ref() {
                payload["summary"] = json!(safe_text(preview).into_string());
            }
            if let Some(media) =
                artifact_media_payload(event.blob_ref.as_ref(), Some(event.kind.as_str()))
            {
                payload["media"] = media;
            }
            (
                "artifact.created",
                "engine",
                "public",
                payload,
                None,
                event.at,
            )
        }
        Event::ArtifactUpdated(event) => {
            let mut payload = json!({
                "artifactId": event.artifact_id,
                "source": artifact_source_label(event.source),
            });
            if let Some(title) = event.title.as_ref() {
                payload["title"] = json!(safe_text(title).into_string());
            }
            if let Some(kind) = event.kind.as_ref() {
                payload["kind"] = json!(safe_text(kind).into_string());
            }
            if let Some(status) = event.status {
                payload["status"] = json!(artifact_status_label(status));
            }
            if let Some(preview) = event.preview.as_ref() {
                payload["summary"] = json!(safe_text(preview).into_string());
            }
            if let Some(media) =
                artifact_media_payload(event.blob_ref.as_ref(), event.kind.as_deref())
            {
                payload["media"] = media;
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
        Event::AssistantReviewRequested(event) => {
            let mut payload = json!({
                "requestId": event.request_id.to_string(),
                "title": safe_text(event.title.as_str()).into_string(),
            });
            if let Some(body) = event.body.as_ref() {
                payload["body"] = json!(safe_text(body.as_str()).into_string());
            }
            (
                "assistant.review.requested",
                "assistant",
                "public",
                payload,
                None,
                event.at,
            )
        }
        Event::AssistantClarificationRequested(event) => (
            "assistant.clarification.requested",
            "assistant",
            "public",
            json!({
                "requestId": event.request_id.to_string(),
                "prompt": safe_text(event.prompt.as_str()).into_string(),
            }),
            None,
            event.at,
        ),
        Event::AssistantNotice(event) => (
            "assistant.notice",
            "assistant",
            "public",
            {
                let mut payload = json!({
                "noticeId": event.notice_id.to_string(),
                "body": safe_text(event.body.as_str()).into_string(),
                });
                if let Some(code) = event.code.as_ref() {
                    payload["code"] = json!(code);
                }
                payload
            },
            None,
            event.at,
        ),
        Event::ToolUseRequested(event) => {
            let tool_name = safe_text(&event.tool_name).as_str().to_owned();
            tool_context = Some((event.tool_use_id, event.run_id, tool_name.clone()));
            let mut payload = json!({
                "argumentsSummary": "Input withheld from conversation timeline.",
                "toolName": tool_name,
                "toolUseId": event.tool_use_id.to_string(),
            });
            if let Some(command) = safe_tool_command_preview(&event.tool_name, &event.input) {
                payload["command"] = json!(command);
            }
            if is_file_read_tool_name(&event.tool_name) || is_file_edit_tool_name(&event.tool_name)
            {
                if let Some(target_path) = safe_tool_target_path_preview(&event.input) {
                    payload["targetPath"] = json!(target_path);
                }
            }
            if is_file_search_tool_name(&event.tool_name) {
                if let Some(query) = safe_tool_query_preview(&event.input) {
                    payload["query"] = json!(query);
                }
            }
            if event.tool_name == "agent" {
                if let Some(role) = safe_agent_tool_role_preview(&event.input) {
                    payload["role"] = json!(role);
                }
                if let Some(task) = safe_agent_tool_task_preview(&event.input) {
                    payload["taskSummary"] = json!(task);
                }
            }
            (
                "tool.requested",
                "tool",
                "redacted",
                payload,
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
        Event::ToolUseCompleted(event) => {
            let tool_name = tool_context_tool_name(tx, tenant_id, session_id, event.tool_use_id)?;
            let mut payload = json!({
                "durationMs": event.duration_ms,
                "outputSummary": "Output withheld from conversation timeline.",
                "toolUseId": event.tool_use_id.to_string(),
            });
            if let Some(tool_name) = tool_name.as_ref() {
                payload["toolName"] = json!(safe_text(tool_name).as_str());
            }
            project_safe_tool_result_fields(tool_name.as_deref(), &event.result, &mut payload);
            (
                "tool.completed",
                "tool",
                "redacted",
                payload,
                None,
                event.at,
            )
        }
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
            let reason = if event.auto_resolved {
                "已按本次授权模式自动允许。"
            } else {
                "需要批准后才能继续。"
            };
            (
                "permission.requested",
                "policy",
                "public",
                json!({
                    "autoResolved": event.auto_resolved,
                    "decisionScope": decision_scope_display(&event.scope_hint),
                    "exposure": subject.exposure,
                    "operation": subject.operation,
                    "reason": reason,
                    "requestId": event.request_id.to_string(),
                    "severity": severity_label(event.severity),
                    "target": subject.target,
                    "toolUseId": event.tool_use_id.to_string(),
                    "workspaceBoundary": "current workspace",
                    "actorSource": permission_actor_source_payload(&event.actor_source),
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
        Event::SubagentSpawned(event) => {
            let role = safe_text(&event.agent_ref.name).into_string();
            let task_summary = event
                .trigger_tool_use_id
                .and_then(|tool_use_id| {
                    timeline_tool_task_summary(tx, tenant_id, session_id, tool_use_id)
                })
                .unwrap_or_else(|| {
                    "Subagent task details withheld from conversation timeline.".to_owned()
                });
            (
                "subagent.spawned",
                "agent",
                "public",
                json!({
                    "subagentId": event.subagent_id.to_string(),
                    "role": role,
                    "taskSummary": task_summary,
                    "depth": event.depth,
                    "triggerToolUseId": event.trigger_tool_use_id.map(|id| id.to_string()),
                }),
                None,
                event.at,
            )
        }
        Event::SubagentAnnounced(event) => {
            let safe_summary = safe_text(&event.summary);
            let redacted = safe_summary.as_str().contains("[REDACTED]");
            (
                "subagent.announced",
                "agent",
                if redacted { "redacted" } else { "public" },
                json!({
                    "subagentId": event.subagent_id.to_string(),
                    "status": subagent_status_label(&event.status),
                    "resultSummary": if redacted {
                        "Subagent result withheld from conversation timeline.".to_owned()
                    } else {
                        safe_summary.into_string()
                    },
                    "redacted": redacted,
                }),
                None,
                event.at,
            )
        }
        Event::SubagentTerminated(event) => (
            "subagent.terminated",
            "agent",
            "public",
            json!({
                "subagentId": event.subagent_id.to_string(),
                "reason": subagent_termination_reason_label(&event.reason),
            }),
            None,
            event.at,
        ),
        Event::SubagentStalled(event) => (
            "subagent.stalled",
            "agent",
            "public",
            json!({
                "subagentId": event.subagent_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::SubagentPermissionForwarded(event) => (
            "subagent.permission.forwarded",
            "policy",
            "public",
            json!({
                "subagentId": event.subagent_id.to_string(),
                "requestId": event.original_request_id.to_string(),
                "reason": "Subagent permission forwarded to parent.",
            }),
            None,
            event.forwarded_at,
        ),
        Event::SubagentPermissionResolved(event) => (
            "subagent.permission.resolved",
            "policy",
            "public",
            json!({
                "subagentId": event.subagent_id.to_string(),
                "requestId": event.original_request_id.to_string(),
                "decision": permission_decision_label(&event.decision),
            }),
            None,
            event.at,
        ),
        Event::TeamCreated(event) => (
            "team.created",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "name": safe_text(&event.name).as_str(),
                "topologyKind": topology_kind_label(&event.topology_kind),
            }),
            None,
            event.created_at,
        ),
        Event::TeamMemberJoined(event) => (
            "team.member.joined",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "agentId": event.agent_id.to_string(),
                "role": safe_text(&event.role).as_str(),
            }),
            None,
            event.joined_at,
        ),
        Event::TeamMemberLeft(event) => (
            "team.member.left",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "agentId": event.agent_id.to_string(),
                "reason": member_leave_reason_label(&event.reason),
            }),
            None,
            event.left_at,
        ),
        Event::TeamMemberStalled(event) => (
            "team.member.stalled",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "agentId": event.agent_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::AgentMessageSent(event) => (
            "agent.message.sent",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "messageId": event.message_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::AgentMessageRouted(event) => (
            "agent.message.routed",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "messageId": event.message_id.to_string(),
                "resolvedRecipients": event
                    .resolved_recipients
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "routingPolicy": routing_policy_label(&event.routing_policy),
            }),
            None,
            event.at,
        ),
        Event::TeamTurnCompleted(event) => (
            "team.turn.completed",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "turnId": event.turn_id.to_string(),
                "participatingAgents": event
                    .participating_agents
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            }),
            None,
            event.at,
        ),
        Event::TeamTaskUpdated(event) => (
            "team.task.updated",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "taskId": safe_text(&event.task_id).as_str(),
                "title": safe_text(&event.title).as_str(),
                "status": safe_text(&event.status).as_str(),
                "assigneeProfileId": event
                    .assignee_profile_id
                    .as_ref()
                    .map(|value| safe_text(value).into_string()),
            }),
            None,
            event.at,
        ),
        Event::TeamTerminated(event) => (
            "team.terminated",
            "agent",
            "public",
            json!({
                "teamId": event.team_id.to_string(),
                "reason": team_termination_reason_label(&event.reason),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentStarted(event) => {
            background_context = Some((event.background_agent_id, event.attempt_id));
            (
                "background.started",
                "background",
                "public",
                json!({
                    "backgroundAgentId": event.background_agent_id.to_string(),
                    "title": safe_text(event.title.as_str()).into_string(),
                }),
                None,
                event.at,
            )
        }
        Event::BackgroundAgentStateChanged(event) => (
            "background.state.changed",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "from": event.from,
                "to": event.to,
                "reason": event
                    .reason
                    .as_ref()
                    .map(|reason| safe_text(reason.as_str()).into_string()),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentInputRequested(event) => (
            "background.input.requested",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "requestId": event.request_id.to_string(),
                "prompt": safe_text(event.prompt.as_str()).into_string(),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentInputSubmitted(event) => (
            "background.input.submitted",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "requestId": event.request_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentPermissionRequested(event) => (
            "background.permission.requested",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "requestId": event.request_id.to_string(),
                "reason": safe_text(event.reason.as_str()).into_string(),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentPermissionResolved(event) => (
            "background.permission.resolved",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "requestId": event.request_id.to_string(),
                "decision": permission_decision_label(&event.decision),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentCancelled(event) => (
            "background.cancelled",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "reason": event
                    .reason
                    .as_ref()
                    .map(|reason| safe_text(reason.as_str()).into_string()),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentCompleted(event) => (
            "background.completed",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "summary": event
                    .summary
                    .as_ref()
                    .map(|summary| safe_text(summary.as_str()).into_string()),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentFailed(event) => (
            "background.failed",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "error": safe_text(event.error.as_str()).into_string(),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentInterrupted(event) => (
            "background.interrupted",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
                "reason": safe_text(event.reason.as_str()).into_string(),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentArchived(event) => (
            "background.archived",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
            }),
            None,
            event.at,
        ),
        Event::BackgroundAgentDeleted(event) => (
            "background.deleted",
            "background",
            "public",
            json!({
                "backgroundAgentId": event.background_agent_id.to_string(),
            }),
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
        background_context,
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
        Event::AssistantReviewRequested(event) => Some(event.run_id),
        Event::AssistantClarificationRequested(event) => Some(event.run_id),
        Event::AssistantNotice(event) => Some(event.run_id),
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
        Event::SubagentSpawned(event) => Some(event.parent_run_id),
        Event::SubagentStalled(event) => Some(event.parent_run_id),
        Event::BackgroundAgentStarted(event) => Some(event.attempt_id),
        Event::BackgroundAgentStateChanged(event) => event.attempt_id.or(
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?,
        ),
        Event::BackgroundAgentInputRequested(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentInputSubmitted(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentPermissionRequested(event) => event.attempt_id.or(
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?,
        ),
        Event::BackgroundAgentPermissionResolved(event) => event.attempt_id.or(
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?,
        ),
        Event::BackgroundAgentCancelled(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentCompleted(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentFailed(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentInterrupted(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentArchived(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
        Event::BackgroundAgentDeleted(event) => {
            background_context_run_id(tx, tenant_id, session_id, event.background_agent_id)?
        }
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

fn attachment_references_payload(attachments: &[ConversationAttachmentReference]) -> Vec<Value> {
    attachments
        .iter()
        .map(|attachment| {
            json!({
                "id": attachment.id,
                "name": safe_attachment_name(&attachment.name).as_str(),
                "mimeType": safe_mime_type(&attachment.mime_type),
                "sizeBytes": attachment.size_bytes,
                "blobRef": {
                    "id": attachment.blob_ref.id.to_string(),
                    "size": attachment.blob_ref.size,
                    "contentHash": attachment.blob_ref.content_hash,
                    "contentType": attachment
                        .blob_ref
                        .content_type
                        .as_deref()
                        .and_then(safe_optional_mime_type),
                },
            })
        })
        .collect()
}

fn safe_attachment_name(value: &str) -> UiSafeText {
    let safe = safe_text(value);
    if safe.as_str().contains("[REDACTED]") {
        UiSafeText::from_trusted_redacted("[REDACTED]")
    } else {
        safe
    }
}

fn safe_mime_type(value: &str) -> &str {
    safe_optional_mime_type(value).unwrap_or("application/octet-stream")
}

fn safe_optional_mime_type(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty()
        || contains_obvious_secret(value)
        || redact_unsafe_process_text(value) != value
        || !is_mime_type_shape(value)
    {
        return None;
    }

    Some(value)
}

fn is_mime_type_shape(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };

    !kind.is_empty()
        && !subtype.is_empty()
        && kind.bytes().all(is_mime_token_byte)
        && subtype.bytes().all(is_mime_token_byte)
}

fn is_mime_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
        )
}

fn safe_text(value: impl AsRef<str>) -> UiSafeText {
    UiSafeText::from_redacted_display(
        redact_obvious_secret_tokens(&redact_unsafe_process_text(value.as_ref())),
        &harness_contracts::NoopRedactor,
    )
}

fn permission_actor_source_payload(actor_source: &PermissionActorSource) -> Value {
    match actor_source {
        PermissionActorSource::ParentRun => json!({ "type": "parentRun" }),
        PermissionActorSource::Subagent {
            subagent_id,
            parent_session_id,
            parent_run_id,
            team_id,
            team_member_profile_id,
        } => {
            let mut payload = serde_json::Map::from_iter([
                ("type".to_owned(), json!("subagent")),
                ("subagentId".to_owned(), json!(subagent_id.to_string())),
                (
                    "parentSessionId".to_owned(),
                    json!(parent_session_id.to_string()),
                ),
                ("parentRunId".to_owned(), json!(parent_run_id.to_string())),
            ]);
            if let Some(team_id) = team_id {
                payload.insert("teamId".to_owned(), json!(team_id.to_string()));
            }
            if let Some(profile_id) = team_member_profile_id {
                payload.insert(
                    "teamMemberProfileId".to_owned(),
                    json!(safe_text(profile_id).into_string()),
                );
            }
            Value::Object(payload)
        }
        PermissionActorSource::TeamMember {
            team_id,
            agent_id,
            role,
            parent_run_id,
        } => {
            let mut payload = serde_json::Map::from_iter([
                ("type".to_owned(), json!("teamMember")),
                ("teamId".to_owned(), json!(team_id.to_string())),
                ("agentId".to_owned(), json!(agent_id.to_string())),
                ("role".to_owned(), json!(safe_text(role).into_string())),
            ]);
            if let Some(parent_run_id) = parent_run_id {
                payload.insert("parentRunId".to_owned(), json!(parent_run_id.to_string()));
            }
            Value::Object(payload)
        }
        PermissionActorSource::BackgroundAgent {
            background_agent_id,
            conversation_id,
            attempt_id,
        } => {
            let mut payload = serde_json::Map::from_iter([
                ("type".to_owned(), json!("backgroundAgent")),
                (
                    "backgroundAgentId".to_owned(),
                    json!(background_agent_id.to_string()),
                ),
                (
                    "conversationId".to_owned(),
                    json!(conversation_id.to_string()),
                ),
            ]);
            if let Some(attempt_id) = attempt_id {
                payload.insert("attemptId".to_owned(), json!(attempt_id.to_string()));
            }
            Value::Object(payload)
        }
    }
}

fn redact_obvious_secret_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut pending_secret_value = false;

    for token in value.split_inclusive(char::is_whitespace) {
        let (body, trailing_ws) = token.split_at(token.trim_end_matches(char::is_whitespace).len());
        if body.is_empty() {
            output.push_str(trailing_ws);
            continue;
        }

        let lower = body.to_ascii_lowercase();
        let redact_current = pending_secret_value || is_obvious_secret_token(&lower);
        pending_secret_value = matches!(lower.as_str(), "bearer" | "basic")
            || lower.ends_with("authorization:")
            || lower.ends_with("authorization");

        if redact_current {
            output.push_str("[REDACTED]");
        } else {
            output.push_str(body);
        }
        output.push_str(trailing_ws);
    }

    output
}

fn is_obvious_secret_token(lower: &str) -> bool {
    lower.contains("authorization:")
        || lower == "bearer"
        || lower == "basic"
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("apikey")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("ghu_")
        || lower.contains("ghs_")
        || lower.contains("ghr_")
        || lower.contains("akia")
        || lower.contains("aiza")
        || lower.contains("xoxb-")
        || lower.contains("xoxp-")
        || lower.contains("xoxa-")
        || lower.contains("xoxr-")
        || lower.contains("npm_")
        || lower.contains("lin_api_")
        || lower.contains("secret_")
        || lower.starts_with("eyj")
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

fn artifact_source_label(source: ArtifactSource) -> &'static str {
    match source {
        ArtifactSource::Assistant => "assistant",
        ArtifactSource::Tool => "tool",
        ArtifactSource::File => "file",
        ArtifactSource::ModelService => "model_service",
        _ => "assistant",
    }
}

fn artifact_media_payload(
    blob_ref: Option<&BlobRef>,
    artifact_kind: Option<&str>,
) -> Option<Value> {
    let blob_ref = blob_ref?;
    let safe_mime_type = blob_ref
        .content_type
        .as_deref()
        .and_then(safe_artifact_mime_type);
    let kind = artifact_kind
        .and_then(artifact_media_kind_from_label)
        .or_else(|| {
            safe_mime_type
                .as_deref()
                .and_then(artifact_media_kind_from_mime)
        })?;
    let mime_type = safe_mime_type
        .filter(|mime_type| {
            kind == "file"
                || artifact_media_kind_from_mime(mime_type)
                    .is_some_and(|mime_kind| mime_kind == kind)
        })
        .unwrap_or_else(|| default_artifact_mime_type(kind).to_owned());
    Some(json!({
        "kind": kind,
        "mimeType": mime_type,
        "sizeBytes": blob_ref.size,
    }))
}

fn artifact_media_kind_from_label(value: &str) -> Option<&'static str> {
    match value {
        "image" => Some("image"),
        "video" => Some("video"),
        "audio" => Some("audio"),
        "file" => Some("file"),
        _ => safe_artifact_mime_type(value)
            .as_deref()
            .and_then(artifact_media_kind_from_mime),
    }
}

fn artifact_media_kind_from_mime(value: &str) -> Option<&'static str> {
    if safe_artifact_image_mime_type(value).is_some() {
        Some("image")
    } else if value.starts_with("video/") {
        Some("video")
    } else if value.starts_with("audio/") {
        Some("audio")
    } else if safe_artifact_mime_type(value).is_some() {
        Some("file")
    } else {
        None
    }
}

fn default_artifact_mime_type(kind: &str) -> &'static str {
    match kind {
        "image" => "image/png",
        "video" => "video/mp4",
        "audio" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

fn safe_artifact_mime_type(value: &str) -> Option<String> {
    let mime_type = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime_type.as_str() {
        "image/png"
        | "image/jpeg"
        | "image/gif"
        | "image/webp"
        | "image/avif"
        | "video/mp4"
        | "video/webm"
        | "video/quicktime"
        | "audio/mpeg"
        | "audio/mp4"
        | "audio/ogg"
        | "audio/wav"
        | "audio/webm"
        | "text/plain"
        | "text/markdown"
        | "text/csv"
        | "application/json"
        | "application/pdf"
        | "application/zip"
        | "application/octet-stream" => Some(mime_type),
        _ => None,
    }
}

fn safe_artifact_image_mime_type(value: &str) -> Option<&'static str> {
    match value {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

fn safe_tool_command_preview(tool_name: &str, input: &Value) -> Option<String> {
    if !is_command_tool_name(tool_name) {
        return None;
    }
    let command = ["command", "code", "script"]
        .into_iter()
        .find_map(|field| input.get(field).and_then(Value::as_str))?
        .trim();
    safe_process_preview_text(command)
}

fn safe_tool_target_path_preview(input: &Value) -> Option<String> {
    ["path", "filePath", "file_path", "targetPath", "target_path"]
        .into_iter()
        .find_map(|field| input.get(field).and_then(Value::as_str))
        .and_then(safe_relative_path)
}

fn safe_tool_query_preview(input: &Value) -> Option<String> {
    ["pattern", "query", "glob", "search"]
        .into_iter()
        .find_map(|field| input.get(field).and_then(Value::as_str))
        .and_then(safe_process_preview_text)
}

fn safe_agent_tool_role_preview(input: &Value) -> Option<String> {
    input
        .get("role")
        .and_then(Value::as_str)
        .and_then(safe_process_preview_text)
}

fn safe_agent_tool_task_preview(input: &Value) -> Option<String> {
    input
        .get("task")
        .and_then(Value::as_str)
        .and_then(safe_process_preview_text)
}

fn timeline_tool_task_summary(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    tool_use_id: ToolUseId,
) -> Option<String> {
    let mut statement = tx
        .prepare(
            "SELECT payload FROM conversation_timeline_event
             WHERE tenant_id = ?1 AND session_id = ?2 AND event_type = 'tool.requested'
             ORDER BY conversation_sequence ASC",
        )
        .ok()?;
    let rows = statement
        .query_map(
            params![tenant_id.to_string(), session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .ok()?;
    for row in rows.flatten() {
        let payload: Value = serde_json::from_str(&row).ok()?;
        if payload
            .get("toolUseId")
            .and_then(Value::as_str)
            .is_some_and(|value| value == tool_use_id.to_string())
        {
            return payload
                .get("taskSummary")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    }
    None
}

fn subagent_status_label(status: &SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Completed => "completed",
        SubagentStatus::Cancelled => "cancelled",
        SubagentStatus::Failed => "failed",
        SubagentStatus::Stalled => "stalled",
        SubagentStatus::MaxIterationsReached => "maxIterationsReached",
        SubagentStatus::MaxBudget(_) => "failed",
        _ => "failed",
    }
}

fn subagent_termination_reason_label(reason: &SubagentTerminationReason) -> &'static str {
    match reason {
        SubagentTerminationReason::NaturalCompletion => "naturalCompletion",
        SubagentTerminationReason::ParentCancelled => "parentCancelled",
        SubagentTerminationReason::AdminInterrupted { .. } => "adminInterrupted",
        SubagentTerminationReason::Stalled { .. } => "stalled",
        SubagentTerminationReason::BridgeBroken => "bridgeBroken",
        SubagentTerminationReason::Failed { .. } => "failed",
        _ => "failed",
    }
}

fn topology_kind_label(kind: &TopologyKind) -> &'static str {
    match kind {
        TopologyKind::CoordinatorWorker => "coordinator_worker",
        TopologyKind::PeerToPeer => "peer_to_peer",
        TopologyKind::RoleRouted => "role_routed",
        TopologyKind::Custom(_) => "custom",
        _ => "custom",
    }
}

fn member_leave_reason_label(reason: &MemberLeaveReason) -> &'static str {
    match reason {
        MemberLeaveReason::GoalAchieved => "goal_achieved",
        MemberLeaveReason::QuotaExceeded => "quota_exceeded",
        MemberLeaveReason::Interrupted => "interrupted",
        MemberLeaveReason::Error(_) => "error",
        MemberLeaveReason::Removed => "removed",
        MemberLeaveReason::StalledRemoved => "stalled_removed",
        _ => "error",
    }
}

fn routing_policy_label(policy: &RoutingPolicyKind) -> &'static str {
    match policy {
        RoutingPolicyKind::Direct => "direct",
        RoutingPolicyKind::Role => "role",
        RoutingPolicyKind::Broadcast => "broadcast",
        RoutingPolicyKind::Coordinator => "coordinator",
        RoutingPolicyKind::Custom(_) => "custom",
        _ => "custom",
    }
}

fn team_termination_reason_label(reason: &TeamTerminationReason) -> &'static str {
    match reason {
        TeamTerminationReason::Completed => "completed",
        TeamTerminationReason::Cancelled => "cancelled",
        TeamTerminationReason::Error(_) => "error",
        TeamTerminationReason::MemberFailed => "member_failed",
        TeamTerminationReason::IdleTimeout => "idle_timeout",
        TeamTerminationReason::Timeout => "timeout",
        _ => "error",
    }
}

fn project_safe_tool_result_fields(
    tool_name: Option<&str>,
    result: &ToolResult,
    payload: &mut Value,
) {
    let Some(tool_name) = tool_name else {
        return;
    };
    if is_command_tool_name(tool_name) {
        if let Some(exit_code) = safe_tool_result_exit_code(result) {
            payload["exitCode"] = json!(exit_code);
        }
        if let Some(output_summary) = safe_tool_result_output_summary(result) {
            payload["outputSummary"] = json!(output_summary);
        }
        return;
    }
    if is_file_activity_tool_name(tool_name) {
        if let Some(item_count) = safe_tool_result_item_count(tool_name, result) {
            payload["itemCount"] = json!(item_count);
        }
        if is_file_edit_tool_name(tool_name) {
            if let Some(diff) = safe_tool_result_diff(result) {
                payload["diff"] = diff;
            }
        }
    }
}

fn safe_tool_result_item_count(tool_name: &str, result: &ToolResult) -> Option<u32> {
    match result {
        ToolResult::Structured(Value::Array(items)) => {
            safe_tool_result_items_count(tool_name, items)
        }
        ToolResult::Mixed(parts) => parts.iter().find_map(|part| match part {
            ToolResultPart::Structured {
                value: Value::Array(items),
                ..
            } => safe_tool_result_items_count(tool_name, items),
            _ => None,
        }),
        _ => None,
    }
}

fn safe_tool_result_items_count(tool_name: &str, items: &[Value]) -> Option<u32> {
    let count = items
        .iter()
        .filter(|item| safe_tool_result_item_is_countable(tool_name, item))
        .count();
    u32::try_from(count).ok()
}

fn safe_tool_result_item_is_countable(tool_name: &str, item: &Value) -> bool {
    let path_fields = [
        "path",
        "file",
        "filePath",
        "file_path",
        "fileName",
        "file_name",
        "targetPath",
        "target_path",
    ];
    if let Some(path) = path_fields
        .into_iter()
        .find_map(|field| item.get(field).and_then(Value::as_str))
    {
        return safe_relative_path(path).is_some();
    }
    if is_file_read_tool_name(tool_name) {
        if let Some(path) = item.as_str() {
            return safe_relative_path(path).is_some();
        }
    }
    true
}

fn safe_tool_result_exit_code(result: &ToolResult) -> Option<i32> {
    let value = structured_tool_result_value(result)?;
    value
        .get("exitCode")
        .or_else(|| value.get("exit_code"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn safe_tool_result_output_summary(result: &ToolResult) -> Option<String> {
    match result {
        ToolResult::Text(text) => safe_process_preview_text(text),
        ToolResult::Structured(value) => safe_structured_output_summary(value),
        ToolResult::Mixed(parts) => parts.iter().find_map(|part| match part {
            ToolResultPart::Text { text } => safe_process_preview_text(text),
            ToolResultPart::Structured { value, .. } => safe_structured_output_summary(value),
            ToolResultPart::Reference { summary, .. } => {
                summary.as_deref().and_then(safe_process_preview_text)
            }
            ToolResultPart::Table { rows, caption, .. } => caption
                .as_deref()
                .and_then(safe_process_preview_text)
                .or_else(|| Some(format!("{} rows", rows.len()))),
            ToolResultPart::Error { code, .. } => safe_process_preview_text(code),
            ToolResultPart::Artifact { preview, title, .. } => preview
                .as_deref()
                .filter(|text| !text.is_empty())
                .and_then(safe_process_preview_text)
                .or_else(|| safe_process_preview_text(title)),
            ToolResultPart::Blob { .. }
            | ToolResultPart::Code { .. }
            | ToolResultPart::Progress { .. } => None,
            _ => None,
        }),
        ToolResult::Blob { .. } => None,
        _ => None,
    }
}

fn safe_structured_output_summary(value: &Value) -> Option<String> {
    if let Some(text) = ["outputSummary", "summary", "stdout", "output", "text"]
        .into_iter()
        .find_map(|field| value.get(field).and_then(Value::as_str))
        .and_then(safe_process_preview_text)
    {
        return Some(text);
    }
    if let Some(stderr) = value
        .get("stderr")
        .and_then(Value::as_str)
        .and_then(safe_process_preview_text)
    {
        return Some(stderr);
    }
    value
        .as_array()
        .and_then(|items| Some(format!("{} results", items.len())))
}

fn safe_tool_result_diff(result: &ToolResult) -> Option<Value> {
    let value = structured_tool_result_value(result)?;
    let diff = value.get("diff").unwrap_or(value);
    let files = diff.get("files")?.as_array()?;
    let safe_files = files
        .iter()
        .filter_map(safe_diff_file_payload)
        .collect::<Vec<_>>();
    (!safe_files.is_empty()).then(|| json!({ "files": safe_files }))
}

fn safe_diff_file_payload(value: &Value) -> Option<Value> {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .and_then(safe_relative_path)?;
    let added_lines = value
        .get("addedLines")
        .or_else(|| value.get("added_lines"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let removed_lines = value
        .get("removedLines")
        .or_else(|| value.get("removed_lines"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let mut file = json!({
        "path": path,
        "addedLines": added_lines,
        "removedLines": removed_lines,
    });
    if let Some(preview) = value
        .get("preview")
        .and_then(Value::as_str)
        .and_then(safe_process_preview_text)
    {
        file["preview"] = json!(preview);
    }
    Some(file)
}

fn structured_tool_result_value(result: &ToolResult) -> Option<&Value> {
    match result {
        ToolResult::Structured(value) => Some(value),
        ToolResult::Mixed(parts) => parts.iter().find_map(|part| match part {
            ToolResultPart::Structured { value, .. } => Some(value),
            _ => None,
        }),
        ToolResult::Text(_) | ToolResult::Blob { .. } => None,
        _ => None,
    }
}

fn is_command_tool_name(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized == "bash" || normalized.contains("shell") || normalized.contains("execute_code")
}

fn is_file_activity_tool_name(tool_name: &str) -> bool {
    is_file_read_tool_name(tool_name)
        || is_file_search_tool_name(tool_name)
        || is_file_edit_tool_name(tool_name)
}

fn is_file_read_tool_name(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized.contains("fileread")
        || normalized.contains("read_file")
        || normalized.contains("readfile")
        || normalized == "read"
        || normalized.contains("list_dir")
        || normalized.contains("listdir")
}

fn is_file_search_tool_name(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized.contains("grep") || normalized.contains("glob") || normalized.contains("search")
}

fn is_file_edit_tool_name(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized.contains("filewrite")
        || normalized.contains("fileedit")
        || normalized.contains("apply_patch")
        || normalized == "write"
        || normalized == "edit"
}

fn safe_process_preview_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || contains_obvious_secret(trimmed) {
        return None;
    }
    let redacted = redact_unsafe_process_text(trimmed);
    Some(truncate_utf8(redacted, 1_200))
}

fn safe_relative_path(value: &str) -> Option<String> {
    let trimmed = value.trim().replace('\\', "/");
    if trimmed.is_empty()
        || trimmed.starts_with('~')
        || trimmed.starts_with('/')
        || trimmed.contains("://")
        || unsafe_url_starts_at(&trimmed, 0)
        || contains_obvious_secret(&trimmed)
        || is_windows_absolute_path(&trimmed)
    {
        return None;
    }
    let mut clean = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        if unsafe_url_starts_at(part, 0) {
            return None;
        }
        clean.push(part);
    }
    if clean
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case(".jyowo"))
    {
        return None;
    }
    (!clean.is_empty()).then(|| truncate_utf8(clean.join("/"), 1_200))
}

fn contains_obvious_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("xoxb-")
}

fn redact_unsafe_process_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if unsafe_url_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_url_token_end(value, index);
            continue;
        }
        if local_unsafe_path_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_token_end(value, index);
            continue;
        }
        let ch = value[index..]
            .chars()
            .next()
            .expect("index is within string bounds");
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_alphanumeric() && ch != '_'))
}

fn unsafe_url_starts_at(value: &str, index: usize) -> bool {
    if unsafe_opaque_url_starts_at(value, index) {
        return true;
    }

    let tail = &value[index..];
    let Some(separator) = tail.find("://") else {
        return false;
    };
    if separator == 0 {
        return false;
    }
    tail[..separator]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn unsafe_opaque_url_starts_at(value: &str, index: usize) -> bool {
    const SCHEMES: &[&str] = &["blob:", "data:", "file:", "javascript:", "mailto:"];
    let tail = &value[index..];
    ascii_token_starts_at(value, index)
        && SCHEMES.iter().any(|scheme| {
            tail.get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

fn ascii_token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_ascii_alphanumeric() && ch != '_'))
}

fn local_unsafe_path_starts_at(value: &str, index: usize) -> bool {
    let tail = &value[index..];
    if tail.starts_with("~/")
        || tail.starts_with("~\\")
        || starts_with_jyowo_path(tail)
        || starts_with_known_unix_absolute_root(tail)
    {
        return true;
    }
    token_starts_at(value, index) && (tail.starts_with('/') || is_windows_absolute_path(tail))
}

fn starts_with_jyowo_path(value: &str) -> bool {
    value
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(".jyowo"))
        && value
            .as_bytes()
            .get(6)
            .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn starts_with_known_unix_absolute_root(value: &str) -> bool {
    const ROOTS: &[&str] = &[
        "/Applications",
        "/Library",
        "/System",
        "/Users",
        "/Volumes",
        "/dev",
        "/etc",
        "/home",
        "/media",
        "/mnt",
        "/opt",
        "/private",
        "/root",
        "/run",
        "/tmp",
        "/usr",
        "/var",
    ];

    ROOTS.iter().any(|root| {
        value
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
    })
}

fn unsafe_url_token_end(value: &str, start: usize) -> usize {
    if starts_with_unsafe_opaque_scheme(value, start, "data:")
        || starts_with_unsafe_opaque_scheme(value, start, "javascript:")
    {
        return unsafe_data_url_token_end(value, start);
    }

    unsafe_token_end(value, start)
}

fn starts_with_unsafe_opaque_scheme(value: &str, start: usize, scheme: &str) -> bool {
    ascii_token_starts_at(value, start)
        && value[start..]
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

fn unsafe_data_url_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '，'
                    | '。'
                    | '；'
                    | '、'
                    | '）'
                    | '】'
                    | '」'
                    | '》'
                    | '！'
                    | '？'
            ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

fn unsafe_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | ')'
                        | ']'
                        | '}'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                        | '，'
                        | '。'
                        | '；'
                        | '、'
                        | '）'
                        | '】'
                        | '」'
                        | '》'
                        | '！'
                        | '？'
                ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

fn truncate_utf8(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
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
    if let Some((tool_use_id, run_id, tool_name)) = event.tool_context.as_ref() {
        tx.execute(
            "INSERT OR IGNORE INTO conversation_projection_tool_context (
                tenant_id, session_id, tool_use_id, run_id, tool_name
             )
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                tool_use_id.to_string(),
                run_id.to_string(),
                tool_name,
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

    if let Some((background_agent_id, run_id)) = event.background_context {
        tx.execute(
            "INSERT INTO conversation_projection_background_context (
                tenant_id, session_id, background_agent_id, run_id
             )
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(tenant_id, session_id, background_agent_id)
             DO UPDATE SET run_id = excluded.run_id",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                background_agent_id.to_string(),
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

fn tool_context_tool_name(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    tool_use_id: ToolUseId,
) -> Result<Option<String>, JournalError> {
    tx.query_row(
        "SELECT tool_name FROM conversation_projection_tool_context
         WHERE tenant_id = ?1 AND session_id = ?2 AND tool_use_id = ?3",
        params![
            tenant_id.to_string(),
            session_id.to_string(),
            tool_use_id.to_string()
        ],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|value| value.flatten())
    .map_err(journal_error)
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

fn background_context_run_id(
    tx: &Transaction<'_>,
    tenant_id: TenantId,
    session_id: SessionId,
    background_agent_id: BackgroundAgentId,
) -> Result<Option<RunId>, JournalError> {
    let run_id = tx
        .query_row(
            "SELECT run_id FROM conversation_projection_background_context
             WHERE tenant_id = ?1 AND session_id = ?2 AND background_agent_id = ?3",
            params![
                tenant_id.to_string(),
                session_id.to_string(),
                background_agent_id.to_string()
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
