//! `SQLite` `EventStore` implementation.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use harness_contracts::{
    Event, EventId, ForkReason, JournalError, JournalOffset, Redactor, SessionId, TenantId,
};
use rusqlite::{params, Connection, TransactionBehavior};
use tokio::sync::Mutex;

use crate::{
    apply_cursor, event_type, journal_error, session_end_reason, AppendMetadata, CompactionLineage,
    EventEnvelope, EventEnvelopePage, EventStore, JournalRedaction, PrunePolicy, PruneReport,
    ReplayCursor, SchemaVersion, SessionFilter, SessionSnapshot, SessionSummary,
};

pub struct SqliteEventStore {
    connection: Mutex<Connection>,
    redaction: JournalRedaction,
}

impl SqliteEventStore {
    pub async fn open(
        path: impl AsRef<Path>,
        redactor: Arc<dyn Redactor>,
    ) -> Result<Self, JournalError> {
        let connection = Connection::open(path).map_err(journal_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA busy_timeout = 5000;
                 CREATE TABLE IF NOT EXISTS events (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    offset INTEGER NOT NULL,
                    event_id TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    recorded_at TEXT NOT NULL,
                    correlation_id TEXT,
                    causation_id TEXT,
                    schema_version INTEGER NOT NULL DEFAULT 1,
                    body TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, offset)
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS idx_events_correlation
                    ON events(tenant_id, correlation_id)
                    WHERE correlation_id IS NOT NULL;
                 CREATE INDEX IF NOT EXISTS idx_events_causation
                    ON events(tenant_id, causation_id)
                    WHERE causation_id IS NOT NULL;
                 CREATE INDEX IF NOT EXISTS idx_events_recorded_at
                    ON events(tenant_id, recorded_at);
                 CREATE INDEX IF NOT EXISTS idx_events_event_id
                    ON events(tenant_id, event_id);
                 CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
                    tenant_id UNINDEXED,
                    session_id UNINDEXED,
                    event_type UNINDEXED,
                    body
                 );
                 CREATE TABLE IF NOT EXISTS snapshots (
                    tenant_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    offset INTEGER NOT NULL,
                    taken_at TEXT NOT NULL,
                    body TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, session_id, offset)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS compaction_lineage (
                    child_session TEXT PRIMARY KEY,
                    parent_session TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    linked_at TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS kv_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                 ) STRICT;",
            )
            .map_err(journal_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            redaction: JournalRedaction::new(redactor),
        })
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
            schema_version: SchemaVersion::CURRENT,
            recorded_at: harness_contracts::now(),
            payload,
        }
    }

    fn load_envelopes(
        connection: &Connection,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let mut statement = connection
            .prepare(
                "SELECT body FROM events
                 WHERE tenant_id = ?1 AND session_id = ?2
                 ORDER BY offset ASC",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(params![tenant.to_string(), session_id.to_string()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(journal_error)?;
        let mut events = Vec::new();
        for row in rows {
            let body = row.map_err(journal_error)?;
            events.push(serde_json::from_str(&body).map_err(journal_error)?);
        }
        Ok(events)
    }

    fn read_lineage(connection: &Connection) -> Result<Vec<CompactionLineage>, JournalError> {
        let mut statement = connection
            .prepare(
                "SELECT parent_session, child_session, reason, linked_at
                 FROM compaction_lineage",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(journal_error)?;
        let mut lineage = Vec::new();
        for row in rows {
            let (parent, child, reason, linked_at) = row.map_err(journal_error)?;
            lineage.push(CompactionLineage {
                parent_session: parent.parse().map_err(journal_error)?,
                child_session: child.parse().map_err(journal_error)?,
                reason: serde_json::from_str(&reason).map_err(journal_error)?,
                linked_at: chrono::DateTime::parse_from_rfc3339(&linked_at)
                    .map_err(journal_error)?
                    .with_timezone(&chrono::Utc),
            });
        }
        Ok(lineage)
    }
}

#[async_trait]
impl EventStore for SqliteEventStore {
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
        let mut connection = self.connection.lock().await;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(journal_error)?;
        let mut offset: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(offset), -1) + 1 FROM events
                 WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant.to_string(), session_id.to_string()],
                |row| row.get(0),
            )
            .map_err(journal_error)?;
        for event in events {
            let envelope = Self::envelope(
                tenant,
                session_id,
                metadata,
                JournalOffset(offset as u64),
                self.redaction.redact_event(event)?,
            );
            let body = serde_json::to_string(&envelope).map_err(journal_error)?;
            let kind = event_type(&envelope.payload)?;
            tx.execute(
                "INSERT INTO events (
                    tenant_id, session_id, offset, event_id, event_type, recorded_at,
                    correlation_id, causation_id, schema_version, body
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    tenant.to_string(),
                    session_id.to_string(),
                    offset as i64,
                    envelope.event_id.to_string(),
                    kind,
                    envelope.recorded_at.to_rfc3339(),
                    envelope.correlation_id.to_string(),
                    envelope.causation_id.map(|id| id.to_string()),
                    i64::from(envelope.schema_version.get()),
                    body
                ],
            )
            .map_err(journal_error)?;
            let rowid = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO events_fts (rowid, tenant_id, session_id, event_type, body)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    rowid,
                    tenant.to_string(),
                    session_id.to_string(),
                    event_type(&envelope.payload)?,
                    serde_json::to_string(&envelope.payload).map_err(journal_error)?
                ],
            )
            .map_err(journal_error)?;
            offset += 1;
        }
        tx.commit().map_err(journal_error)?;
        Ok(JournalOffset(offset.saturating_sub(1) as u64))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        let connection = self.connection.lock().await;
        let mut envelopes = Self::load_envelopes(&connection, tenant, session_id)?;
        apply_cursor(&mut envelopes, cursor);
        Ok(Box::pin(stream::iter(envelopes)))
    }

    async fn page_session_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        after_event_id: Option<EventId>,
        limit: usize,
    ) -> Result<EventEnvelopePage, JournalError> {
        let connection = self.connection.lock().await;
        let after_offset = match after_event_id {
            Some(event_id) => Some(
                connection
                    .query_row(
                        "SELECT offset FROM events
                         WHERE tenant_id = ?1 AND session_id = ?2 AND event_id = ?3",
                        params![
                            tenant.to_string(),
                            session_id.to_string(),
                            event_id.to_string()
                        ],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            journal_error("conversation cursor is unknown")
                        }
                        other => journal_error(other),
                    })?,
            ),
            None => None,
        };
        let limit = limit.clamp(1, 200) as i64;
        let mut statement = connection
            .prepare(
                "SELECT body FROM events
                 WHERE tenant_id = ?1
                   AND session_id = ?2
                   AND (?3 IS NULL OR offset > ?3)
                 ORDER BY offset ASC
                 LIMIT ?4",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(
                params![
                    tenant.to_string(),
                    session_id.to_string(),
                    after_offset,
                    limit
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(journal_error)?;
        let mut envelopes: Vec<EventEnvelope> = Vec::new();
        for row in rows {
            let body = row.map_err(journal_error)?;
            envelopes.push(serde_json::from_str(&body).map_err(journal_error)?);
        }
        let next_event_id = envelopes.last().map(|envelope| envelope.event_id);
        Ok(EventEnvelopePage {
            envelopes,
            next_event_id,
        })
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare(
                "SELECT body FROM events
                 WHERE tenant_id = ?1
                 ORDER BY recorded_at ASC, session_id ASC, offset ASC",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(params![tenant.to_string()], |row| row.get::<_, String>(0))
            .map_err(journal_error)?;
        let mut events = Vec::new();
        for row in rows {
            let body = row.map_err(journal_error)?;
            events.push(serde_json::from_str(&body).map_err(journal_error)?);
        }
        if let Some(after) = after {
            if let Some(position) = events
                .iter()
                .position(|envelope: &EventEnvelope| envelope.event_id == after)
            {
                events = events.into_iter().skip(position + 1).collect();
            } else {
                let after = after.to_string();
                events.retain(|envelope: &EventEnvelope| envelope.event_id.to_string() > after);
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
        let result: Result<String, rusqlite::Error> = self.connection.lock().await.query_row(
            "SELECT body FROM snapshots
             WHERE tenant_id = ?1 AND session_id = ?2
             ORDER BY offset DESC
             LIMIT 1",
            params![tenant.to_string(), session_id.to_string()],
            |row| row.get(0),
        );
        match result {
            Ok(body) => serde_json::from_str(&body).map(Some).map_err(journal_error),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(journal_error(error)),
        }
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.connection
            .lock()
            .await
            .execute(
                "INSERT INTO snapshots (tenant_id, session_id, offset, taken_at, body)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(tenant_id, session_id, offset) DO UPDATE SET
                    taken_at = excluded.taken_at,
                    body = excluded.body",
                params![
                    tenant.to_string(),
                    snapshot.session_id.to_string(),
                    snapshot.offset.0 as i64,
                    snapshot.taken_at.to_rfc3339(),
                    serde_json::to_string(&snapshot).map_err(journal_error)?
                ],
            )
            .map_err(journal_error)?;
        Ok(())
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError> {
        self.connection
            .lock()
            .await
            .execute(
                "INSERT INTO compaction_lineage (parent_session, child_session, reason, linked_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(child_session) DO UPDATE SET
                    parent_session = excluded.parent_session,
                    reason = excluded.reason,
                    linked_at = excluded.linked_at",
                params![
                    parent.to_string(),
                    child.to_string(),
                    serde_json::to_string(&reason).map_err(journal_error)?,
                    harness_contracts::now().to_rfc3339()
                ],
            )
            .map_err(journal_error)?;
        Ok(())
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        let mut connection = self.connection.lock().await;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(journal_error)?;
        let tenant_id = tenant.to_string();
        let session_id_text = session_id.to_string();
        let events_removed = tx
            .execute(
                "DELETE FROM events WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant_id, session_id_text],
            )
            .map_err(journal_error)?;
        tx.execute(
            "DELETE FROM events_fts WHERE tenant_id = ?1 AND session_id = ?2",
            params![tenant.to_string(), session_id.to_string()],
        )
        .map_err(journal_error)?;
        let snapshots_removed = tx
            .execute(
                "DELETE FROM snapshots WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant.to_string(), session_id.to_string()],
            )
            .map_err(journal_error)?;
        let lineage_removed = tx
            .execute(
                "DELETE FROM compaction_lineage
                 WHERE parent_session = ?1 OR child_session = ?1",
                params![session_id.to_string()],
            )
            .map_err(journal_error)?;
        tx.commit().map_err(journal_error)?;
        Ok(events_removed > 0 || snapshots_removed > 0 || lineage_removed > 0)
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare(
                "SELECT session_id
                 FROM events WHERE tenant_id = ?1
                 GROUP BY session_id
                 ORDER BY MIN(recorded_at) ASC",
            )
            .map_err(journal_error)?;
        let rows = statement
            .query_map(params![tenant.to_string()], |row| row.get::<_, String>(0))
            .map_err(journal_error)?;
        let mut sessions = Vec::new();
        for row in rows {
            let session_id: SessionId =
                row.map_err(journal_error)?.parse().map_err(journal_error)?;
            let events = Self::load_envelopes(&connection, tenant, session_id)?;
            if events.is_empty() {
                continue;
            }
            let created_at = events[0].recorded_at;
            if filter.since.is_some_and(|since| created_at < since) {
                continue;
            }
            let end_reason = events
                .iter()
                .filter_map(|envelope| session_end_reason(&envelope.payload))
                .find_map(|(ended_session, reason)| {
                    (ended_session == session_id).then_some(reason)
                });
            if filter
                .end_reason
                .as_ref()
                .is_some_and(|expected| end_reason.as_ref() != Some(expected))
            {
                continue;
            }
            sessions.push(SessionSummary {
                session_id,
                created_at,
                last_event_at: events.last().expect("events is not empty").recorded_at,
                event_count: events.len() as u64,
                end_reason,
                root_session: None,
            });
        }
        if filter.project_compression_tips {
            apply_lineage_projection(&mut sessions, &Self::read_lineage(&connection)?);
        }
        sessions.truncate(filter.limit as usize);
        Ok(sessions)
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        let mut sessions = self
            .list_sessions(
                tenant,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: u32::MAX,
                },
            )
            .await?;
        sessions.sort_by_key(|summary| summary.last_event_at);
        sessions.reverse();
        let keep: HashSet<_> = policy
            .keep_latest_n_sessions
            .map(|limit| {
                sessions
                    .iter()
                    .take(limit as usize)
                    .map(|summary| summary.session_id)
                    .collect()
            })
            .unwrap_or_default();
        let cutoff = harness_contracts::now()
            - chrono::Duration::from_std(policy.older_than)
                .unwrap_or_else(|_| chrono::Duration::zero());
        let candidates: Vec<_> = sessions
            .into_iter()
            .filter(|summary| {
                summary.last_event_at <= cutoff && !keep.contains(&summary.session_id)
            })
            .collect();
        let mut report = PruneReport::default();
        let mut connection = self.connection.lock().await;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(journal_error)?;
        for summary in &candidates {
            let session_id = summary.session_id.to_string();
            let (event_count, bytes): (i64, Option<i64>) = tx
                .query_row(
                    "SELECT COUNT(*), SUM(length(body)) FROM events
                     WHERE tenant_id = ?1 AND session_id = ?2",
                    params![tenant.to_string(), session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(journal_error)?;
            report.events_removed += event_count as u64;
            report.bytes_freed += bytes.unwrap_or_default() as u64;
            tx.execute(
                "DELETE FROM events WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant.to_string(), summary.session_id.to_string()],
            )
            .map_err(journal_error)?;
            tx.execute(
                "DELETE FROM events_fts WHERE tenant_id = ?1 AND session_id = ?2",
                params![tenant.to_string(), summary.session_id.to_string()],
            )
            .map_err(journal_error)?;
            if !policy.keep_snapshots {
                report.snapshots_removed += tx
                    .execute(
                        "DELETE FROM snapshots WHERE tenant_id = ?1 AND session_id = ?2",
                        params![tenant.to_string(), summary.session_id.to_string()],
                    )
                    .map_err(journal_error)? as u64;
            }
            tx.execute(
                "DELETE FROM compaction_lineage
                 WHERE parent_session = ?1 OR child_session = ?1",
                params![summary.session_id.to_string()],
            )
            .map_err(journal_error)?;
        }
        tx.commit().map_err(journal_error)?;
        Ok(report)
    }
}

fn apply_lineage_projection(sessions: &mut Vec<SessionSummary>, lineage: &[CompactionLineage]) {
    let parent_by_child: HashMap<_, _> = lineage
        .iter()
        .map(|entry| (entry.child_session, entry.parent_session))
        .collect();
    let parents: HashSet<_> = lineage.iter().map(|entry| entry.parent_session).collect();
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
