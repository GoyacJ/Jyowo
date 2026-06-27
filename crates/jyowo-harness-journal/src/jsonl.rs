//! JSONL `EventStore` implementation.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fs2::FileExt;
use futures::stream::{self, BoxStream};
use harness_contracts::{
    Event, EventId, ForkReason, JournalError, JournalOffset, Redactor, SessionId, TenantId,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    apply_cursor, journal_error, session_end_reason, AppendMetadata, CompactionLineage,
    EventEnvelope, EventEnvelopePage, EventStore, JournalRedaction, PrunePolicy, PruneReport,
    ReplayCursor, SchemaVersion, SessionFilter, SessionSnapshot, SessionSummary,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum FsyncPolicy {
    Always,
    EveryNAppends(usize),
    Periodic(Duration),
    Never,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct JsonlRotationPolicy {
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct JsonlReadPolicy {
    pub tolerate_partial_tail: bool,
    pub tolerate_invalid_lines: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct JsonlOptions {
    pub fsync: FsyncPolicy,
    pub rotation: JsonlRotationPolicy,
    pub read: JsonlReadPolicy,
}

impl Default for JsonlOptions {
    fn default() -> Self {
        Self {
            fsync: FsyncPolicy::Never,
            rotation: JsonlRotationPolicy {
                max_bytes: 100 * 1024 * 1024,
            },
            read: JsonlReadPolicy {
                tolerate_partial_tail: true,
                tolerate_invalid_lines: false,
            },
        }
    }
}

pub struct JsonlEventStore {
    root: PathBuf,
    options: JsonlOptions,
    redaction: JournalRedaction,
    write_lock: Mutex<()>,
    snapshots: Mutex<HashMap<(TenantId, SessionId), SessionSnapshot>>,
}

impl JsonlEventStore {
    pub async fn open(
        root: impl AsRef<Path>,
        redactor: Arc<dyn Redactor>,
    ) -> Result<Self, JournalError> {
        Self::open_with_options(root, redactor, JsonlOptions::default()).await
    }

    pub async fn open_with_options(
        root: impl AsRef<Path>,
        redactor: Arc<dyn Redactor>,
        options: JsonlOptions,
    ) -> Result<Self, JournalError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(journal_error)?;
        Ok(Self {
            root,
            options,
            redaction: JournalRedaction::new(redactor),
            write_lock: Mutex::new(()),
            snapshots: Mutex::new(HashMap::new()),
        })
    }

    fn path(&self, tenant: TenantId, session_id: SessionId) -> PathBuf {
        self.tenant_dir(tenant).join(format!("{session_id}.jsonl"))
    }

    fn batch_path(&self, tenant: TenantId, session_id: SessionId, offset: u64) -> PathBuf {
        self.tenant_dir(tenant)
            .join(format!("{session_id}.{offset}.jsonl"))
    }

    fn batch_temp_path(&self, tenant: TenantId, session_id: SessionId, offset: u64) -> PathBuf {
        self.tenant_dir(tenant)
            .join(format!("{session_id}.{offset}.tmp"))
    }

    fn tenant_dir(&self, tenant: TenantId) -> PathBuf {
        self.root.join(tenant.to_string())
    }

    fn lock_path(&self, tenant: TenantId, session_id: SessionId) -> PathBuf {
        self.tenant_dir(tenant).join(format!("{session_id}.lock"))
    }

    fn snapshot_path(&self, tenant: TenantId, session_id: SessionId) -> PathBuf {
        self.tenant_dir(tenant)
            .join("snapshots")
            .join(format!("{session_id}.json"))
    }

    fn lineage_path(&self, tenant: TenantId) -> PathBuf {
        self.tenant_dir(tenant).join("_compaction_lineage.jsonl")
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
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let mut envelopes = Vec::new();
        for path in self.segment_paths(tenant, session_id)? {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(journal_error(error)),
            };
            let mut lines = BufReader::new(file).lines().peekable();
            while let Some(line) = lines.next() {
                let line = line.map_err(journal_error)?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<EventEnvelope>(&line) {
                    Ok(envelope) => envelopes.push(envelope),
                    Err(_) if lines.peek().is_none() && self.options.read.tolerate_partial_tail => {
                    }
                    Err(_) if self.options.read.tolerate_invalid_lines => {}
                    Err(error) => return Err(journal_error(error)),
                }
            }
        }
        envelopes.sort_by_key(|envelope| envelope.offset);
        Ok(envelopes)
    }

    fn segment_paths(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Vec<PathBuf>, JournalError> {
        let dir = self.root.join(tenant.to_string());
        let active = self.path(tenant, session_id);
        let mut paths = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries {
                let path = entry.map_err(journal_error)?.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if name.starts_with(&format!("{session_id}.")) && name.ends_with(".jsonl") {
                    paths.push(path);
                }
            }
        }
        paths.sort_by_key(|path| self.segment_path_sort_key(session_id, path));
        if !paths.iter().any(|path| path == &active) {
            paths.push(active);
        }
        Ok(paths)
    }

    fn segment_path_sort_key(&self, session_id: SessionId, path: &Path) -> (u8, u64, PathBuf) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return (2, u64::MAX, path.to_path_buf());
        };
        if name == format!("{session_id}.jsonl") {
            return (0, 0, path.to_path_buf());
        }
        let prefix = format!("{session_id}.");
        let Some(offset) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".jsonl"))
            .and_then(|offset| offset.parse::<u64>().ok())
        else {
            return (2, u64::MAX, path.to_path_buf());
        };
        (1, offset, path.to_path_buf())
    }

    fn session_exists(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Ok(self
            .segment_paths(tenant, session_id)?
            .iter()
            .any(|path| path.exists()))
    }

    fn tenant_ids_for_link(
        &self,
        parent: SessionId,
        child: SessionId,
    ) -> Result<Vec<TenantId>, JournalError> {
        let mut tenants = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries {
                let path = entry.map_err(journal_error)?.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Ok(tenant) = name.parse::<TenantId>() else {
                    continue;
                };
                if self.session_exists(tenant, parent)? || self.session_exists(tenant, child)? {
                    tenants.push(tenant);
                }
            }
        }
        if tenants.is_empty() {
            tenants.push(TenantId::SINGLE);
        }
        Ok(tenants)
    }

    fn read_lineage(&self, tenant: TenantId) -> Result<Vec<CompactionLineage>, JournalError> {
        let path = self.lineage_path(tenant);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(journal_error(error)),
        };
        let mut lineage = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line.map_err(journal_error)?;
            if line.trim().is_empty() {
                continue;
            }
            lineage.push(serde_json::from_str(&line).map_err(journal_error)?);
        }
        Ok(lineage)
    }

    fn write_lineage(
        &self,
        tenant: TenantId,
        lineage: &[CompactionLineage],
    ) -> Result<(), JournalError> {
        let path = self.lineage_path(tenant);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(journal_error)?;
        }
        let temp = path.with_extension("jsonl.tmp");
        {
            let mut file = File::create(&temp).map_err(journal_error)?;
            for entry in lineage {
                serde_json::to_writer(&mut file, entry).map_err(journal_error)?;
                file.write_all(b"\n").map_err(journal_error)?;
            }
        }
        fs::rename(temp, path).map_err(journal_error)
    }
}

#[async_trait]
impl EventStore for JsonlEventStore {
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
        let _guard = self.write_lock.lock().await;
        let path = self.path(tenant, session_id);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(journal_error)?;
        }
        let lock_path = self.lock_path(tenant, session_id);
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .map_err(journal_error)?;
        lock_file.lock_exclusive().map_err(journal_error)?;
        let mut offset = self
            .load_envelopes(tenant, session_id)?
            .last()
            .map_or(0, |envelope| envelope.offset.0 + 1);
        let start_offset = offset;
        let mut lines = Vec::new();
        for event in events {
            let envelope = Self::envelope(
                tenant,
                session_id,
                metadata,
                JournalOffset(offset),
                self.redaction.redact_event(event)?,
            );
            serde_json::to_writer(&mut lines, &envelope).map_err(journal_error)?;
            lines.write_all(b"\n").map_err(journal_error)?;
            offset += 1;
        }
        let batch = self.batch_path(tenant, session_id, start_offset);
        let temp = self.batch_temp_path(tenant, session_id, start_offset);
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(journal_error)?;
        file.write_all(&lines).map_err(journal_error)?;
        if matches!(self.options.fsync, FsyncPolicy::Always) {
            file.sync_all().map_err(journal_error)?;
        }
        drop(file);
        fs::rename(&temp, &batch).map_err(journal_error)?;
        Ok(JournalOffset(offset.saturating_sub(1)))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        let mut envelopes = self.load_envelopes(tenant, session_id)?;
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
        let limit = limit.clamp(1, 200);
        let loaded = self.load_envelopes(tenant, session_id)?;
        let start_index = match after_event_id {
            Some(after_event_id) => loaded
                .iter()
                .position(|envelope| envelope.event_id == after_event_id)
                .map(|index| index + 1)
                .ok_or_else(|| journal_error("conversation cursor is unknown"))?,
            None => 0,
        };
        let envelopes: Vec<_> = loaded.into_iter().skip(start_index).take(limit).collect();
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
        let mut events = Vec::new();
        let dir = self.tenant_dir(tenant);
        let mut session_ids = HashSet::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries {
                let path = entry.map_err(journal_error)?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                let session_part = stem.split('.').next().unwrap_or(stem);
                if let Ok(session_id) = session_part.parse::<SessionId>() {
                    session_ids.insert(session_id);
                }
            }
        }
        for session_id in session_ids {
            events.extend(self.load_envelopes(tenant, session_id)?);
        }
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
        if let Some(snapshot) = self
            .snapshots
            .lock()
            .await
            .get(&(tenant, session_id))
            .cloned()
        {
            return Ok(Some(snapshot));
        }
        let path = self.snapshot_path(tenant, session_id);
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(journal_error),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(journal_error(error)),
        }
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        let path = self.snapshot_path(tenant, snapshot.session_id);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(journal_error)?;
        }
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_vec(&snapshot).map_err(journal_error)?)
            .map_err(journal_error)?;
        fs::rename(temp, path).map_err(journal_error)?;
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
        for tenant in self.tenant_ids_for_link(parent, child)? {
            let entry = CompactionLineage {
                parent_session: parent,
                child_session: child,
                reason: reason.clone(),
                linked_at: harness_contracts::now(),
            };
            let path = self.lineage_path(tenant);
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(journal_error)?;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(journal_error)?;
            serde_json::to_writer(&mut file, &entry).map_err(journal_error)?;
            file.write_all(b"\n").map_err(journal_error)?;
        }
        Ok(())
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        let mut removed = false;
        for path in self.segment_paths(tenant, session_id)? {
            match fs::remove_file(path) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(journal_error(error)),
            }
        }
        match fs::remove_file(self.lock_path(tenant, session_id)) {
            Ok(()) => removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(journal_error(error)),
        }
        match fs::remove_file(self.snapshot_path(tenant, session_id)) {
            Ok(()) => removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(journal_error(error)),
        }
        if self
            .snapshots
            .lock()
            .await
            .remove(&(tenant, session_id))
            .is_some()
        {
            removed = true;
        }

        let mut lineage = self.read_lineage(tenant)?;
        let original_len = lineage.len();
        lineage.retain(|entry| {
            entry.parent_session != session_id && entry.child_session != session_id
        });
        if lineage.len() != original_len {
            self.write_lineage(tenant, &lineage)?;
            removed = true;
        }

        Ok(removed)
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        let dir = self.tenant_dir(tenant);
        let mut session_ids = HashSet::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries {
                let path = entry.map_err(journal_error)?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                let session_part = stem.split('.').next().unwrap_or(stem);
                let Ok(session_id) = session_part.parse::<SessionId>() else {
                    continue;
                };
                session_ids.insert(session_id);
            }
        }
        let mut sessions = Vec::new();
        for session_id in session_ids {
            let events = self.load_envelopes(tenant, session_id)?;
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
            apply_lineage_projection(&mut sessions, &self.read_lineage(tenant)?);
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
        let mut removed_sessions = HashSet::new();
        for summary in &candidates {
            removed_sessions.insert(summary.session_id);
            report.events_removed += summary.event_count;
            for path in self.segment_paths(tenant, summary.session_id)? {
                if let Ok(meta) = fs::metadata(&path) {
                    report.bytes_freed += meta.len();
                }
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(journal_error(error)),
                }
            }
            if !policy.keep_snapshots {
                let snapshot = self.snapshot_path(tenant, summary.session_id);
                match fs::remove_file(snapshot) {
                    Ok(()) => report.snapshots_removed += 1,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(journal_error(error)),
                }
                self.snapshots
                    .lock()
                    .await
                    .remove(&(tenant, summary.session_id));
            }
        }
        if !removed_sessions.is_empty() {
            let mut lineage = self.read_lineage(tenant)?;
            lineage.retain(|entry| {
                !removed_sessions.contains(&entry.parent_session)
                    && !removed_sessions.contains(&entry.child_session)
            });
            self.write_lineage(tenant, &lineage)?;
        }
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
