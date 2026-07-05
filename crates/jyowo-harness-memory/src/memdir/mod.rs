use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use harness_contracts::{
    ContentHash, Event, MemoryError, MemoryId, MemoryKind, MemorySource, MemoryUpsertedEvent,
    MemoryVisibility, MemoryWriteAction, RunId, SessionId, TakesEffect, TenantId,
};
#[cfg(feature = "threat-scanner")]
use harness_contracts::{MemoryThreatDetectedEvent, ThreatAction, ThreatDirection};

#[cfg(feature = "threat-scanner")]
use crate::{threat_content_hash, MemoryThreatScanner};
use crate::{MemoryEventSink, MemoryMetric, MemoryMetricsSink};

mod fence;
mod file;
mod lock;

pub use fence::{escape_for_fence, sanitize_context, wrap_memory_context};
pub use harness_contracts::MemdirFileTag as MemdirFile;

#[derive(Clone)]
pub struct BuiltinMemory {
    root: PathBuf,
    tenant_id: TenantId,
    max_chars_memory: usize,
    max_chars_user: usize,
    section_separator: String,
    snapshot_strategy: SnapshotStrategy,
    concurrency: MemdirConcurrencyPolicy,
    write_takes_effect: TakesEffect,
    event_sink: Option<Arc<dyn MemoryEventSink>>,
    metrics_sink: Option<Arc<dyn MemoryMetricsSink>>,
    event_scope: Option<MemdirEventScope>,
    #[cfg(feature = "threat-scanner")]
    threat_scanner: Option<Arc<MemoryThreatScanner>>,
}

#[derive(Debug, Clone, Copy)]
struct MemdirEventScope {
    session_id: SessionId,
    run_id: Option<RunId>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SnapshotStrategy {
    None,
    DailyOnFirstWrite,
    BeforeEachReplace,
}

#[derive(Debug, Clone)]
pub struct MemdirSnapshot {
    pub memory: String,
    pub user: String,
    pub memory_chars: usize,
    pub user_chars: usize,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemdirWriteOutcome {
    pub bytes_written: u64,
    pub previous_hash: ContentHash,
    pub new_hash: ContentHash,
    pub snapshot_path: Option<PathBuf>,
    pub takes_effect: TakesEffect,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemdirConcurrencyPolicy {
    pub lock_timeout: Duration,
    pub retry_max: u32,
    pub retry_jitter_ms: RangeInclusive<u64>,
}

impl Default for MemdirConcurrencyPolicy {
    fn default() -> Self {
        Self {
            lock_timeout: Duration::from_secs(2),
            retry_max: 5,
            retry_jitter_ms: 20..=150,
        }
    }
}

impl BuiltinMemory {
    pub fn at(root: impl Into<PathBuf>, tenant_id: TenantId) -> Self {
        Self {
            root: root.into(),
            tenant_id,
            max_chars_memory: 16_000,
            max_chars_user: 8_000,
            section_separator: "§".to_owned(),
            snapshot_strategy: SnapshotStrategy::None,
            concurrency: MemdirConcurrencyPolicy::default(),
            write_takes_effect: TakesEffect::NextSession,
            event_sink: None,
            metrics_sink: None,
            event_scope: None,
            #[cfg(feature = "threat-scanner")]
            threat_scanner: None,
        }
    }

    #[must_use]
    pub const fn with_limits(mut self, memory_chars: usize, user_chars: usize) -> Self {
        self.max_chars_memory = memory_chars;
        self.max_chars_user = user_chars;
        self
    }

    #[must_use]
    pub const fn with_snapshot_strategy(mut self, strategy: SnapshotStrategy) -> Self {
        self.snapshot_strategy = strategy;
        self
    }

    #[must_use]
    pub fn with_concurrency_policy(mut self, policy: MemdirConcurrencyPolicy) -> Self {
        self.concurrency = policy;
        self
    }

    #[must_use]
    pub fn with_write_takes_effect(mut self, takes_effect: TakesEffect) -> Self {
        self.write_takes_effect = takes_effect;
        self
    }

    #[must_use]
    pub fn with_event_sink(mut self, sink: Arc<dyn MemoryEventSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, sink: Arc<dyn MemoryMetricsSink>) -> Self {
        self.metrics_sink = Some(sink);
        self
    }

    #[must_use]
    pub const fn with_event_scope(mut self, session_id: SessionId, run_id: Option<RunId>) -> Self {
        self.event_scope = Some(MemdirEventScope { session_id, run_id });
        self
    }

    #[cfg(feature = "threat-scanner")]
    #[must_use]
    pub fn with_threat_scanner(mut self, scanner: Arc<MemoryThreatScanner>) -> Self {
        self.threat_scanner = Some(scanner);
        self
    }

    pub async fn read_all(&self) -> Result<MemdirSnapshot, MemoryError> {
        let this = self.clone();
        let snapshot = spawn_memdir(move || file::read_all(&this)).await?;
        self.record_memdir_bytes(MemdirFile::Memory, snapshot.memory.len() as u64);
        self.record_memdir_bytes(MemdirFile::User, snapshot.user.len() as u64);
        Ok(snapshot)
    }

    pub async fn append_section(
        &self,
        file: MemdirFile,
        section: &str,
        content: &str,
    ) -> Result<MemdirWriteOutcome, MemoryError> {
        let this = self.clone();
        let section = section.to_owned();
        let content = self
            .prepare_content_for_write(content, file::Edit::Append)
            .await?;
        let section_for_write = section.clone();
        let result = spawn_memdir(move || {
            file::write_section_with_previous(
                &this,
                file,
                &section_for_write,
                &content,
                file::Edit::Append,
            )
        })
        .await?;
        let outcome = result.outcome.clone();
        if let Err(error) = self
            .emit_write_event(file, section, file::Edit::Append, outcome.clone())
            .await
        {
            self.rollback_memdir_write(file, result.previous_content)
                .await?;
            return Err(error);
        }
        Ok(outcome)
    }

    pub async fn replace_section(
        &self,
        file: MemdirFile,
        section: &str,
        content: &str,
    ) -> Result<MemdirWriteOutcome, MemoryError> {
        let this = self.clone();
        let section = section.to_owned();
        let content = self
            .prepare_content_for_write(content, file::Edit::Replace)
            .await?;
        let section_for_write = section.clone();
        let result = spawn_memdir(move || {
            file::write_section_with_previous(
                &this,
                file,
                &section_for_write,
                &content,
                file::Edit::Replace,
            )
        })
        .await?;
        let outcome = result.outcome.clone();
        if let Err(error) = self
            .emit_write_event(file, section, file::Edit::Replace, outcome.clone())
            .await
        {
            self.rollback_memdir_write(file, result.previous_content)
                .await?;
            return Err(error);
        }
        Ok(outcome)
    }

    pub async fn delete_section(
        &self,
        file: MemdirFile,
        section: &str,
    ) -> Result<MemdirWriteOutcome, MemoryError> {
        let this = self.clone();
        let section = section.to_owned();
        let section_for_write = section.clone();
        let result = spawn_memdir(move || {
            file::write_section_with_previous(
                &this,
                file,
                &section_for_write,
                "",
                file::Edit::Delete,
            )
        })
        .await?;
        let outcome = result.outcome.clone();
        if let Err(error) = self
            .emit_write_event(file, section, file::Edit::Delete, outcome.clone())
            .await
        {
            self.rollback_memdir_write(file, result.previous_content)
                .await?;
            return Err(error);
        }
        Ok(outcome)
    }

    pub(crate) fn tenant_dir(&self) -> PathBuf {
        self.root.join(self.tenant_id.to_string())
    }

    pub(crate) const fn snapshot_strategy(&self) -> SnapshotStrategy {
        self.snapshot_strategy
    }

    pub(crate) const fn concurrency(&self) -> &MemdirConcurrencyPolicy {
        &self.concurrency
    }

    pub(crate) fn write_takes_effect(&self) -> TakesEffect {
        self.write_takes_effect.clone()
    }

    pub(crate) fn separator(&self) -> &str {
        &self.section_separator
    }

    pub(crate) const fn limit_for(&self, file: MemdirFile) -> usize {
        match file {
            MemdirFile::User => self.max_chars_user,
            _ => self.max_chars_memory,
        }
    }

    #[cfg(feature = "threat-scanner")]
    async fn scan_content_before_write(
        &self,
        content: &str,
    ) -> Result<Option<String>, MemoryError> {
        let Some(scanner) = &self.threat_scanner else {
            return Ok(None);
        };

        let report = scanner.scan(content);
        self.emit_threat_events(content, &report).await;
        if report.action == ThreatAction::Block {
            let hit = report.hits.first();
            return Err(MemoryError::ThreatDetected {
                pattern_id: hit
                    .map(|hit| hit.pattern_id.clone())
                    .unwrap_or_else(|| "unknown".to_owned()),
                category: hit
                    .map(|hit| hit.category)
                    .unwrap_or(harness_contracts::ThreatCategory::PromptInjection),
                action: ThreatAction::Block,
            });
        }

        if report.action == ThreatAction::Redact {
            return Ok(report.redacted_content);
        }

        Ok(None)
    }

    async fn prepare_content_for_write(
        &self,
        content: &str,
        edit: file::Edit,
    ) -> Result<String, MemoryError> {
        if matches!(edit, file::Edit::Delete) {
            return Ok(content.to_owned());
        }

        #[cfg(feature = "threat-scanner")]
        if let Some(redacted) = self.scan_content_before_write(content).await? {
            return Ok(redacted);
        }

        Ok(content.to_owned())
    }

    async fn emit_write_event(
        &self,
        file: MemdirFile,
        section: String,
        edit: file::Edit,
        outcome: MemdirWriteOutcome,
    ) -> Result<(), MemoryError> {
        let action = action_for_edit(edit, section);
        self.record_metric(MemoryMetric::MemdirWrite {
            file,
            action: action.clone(),
            bytes_written: outcome.bytes_written,
        });
        let (Some(sink), Some(scope)) = (&self.event_sink, self.event_scope) else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required memdir audit sink is not configured".to_owned(),
            });
        };
        sink.emit_required(Event::MemoryUpserted(MemoryUpsertedEvent {
            session_id: scope.session_id,
            run_id: scope.run_id,
            memory_id: MemoryId::new(),
            kind: kind_for_file(file),
            visibility: MemoryVisibility::Tenant,
            action,
            provider_id: "builtin-memdir".to_owned(),
            source: MemorySource::UserInput,
            content_hash: outcome.new_hash,
            bytes_written: outcome.bytes_written,
            takes_effect: outcome.takes_effect,
            at: Utc::now(),
        }))
        .await
    }

    async fn rollback_memdir_write(
        &self,
        file: MemdirFile,
        previous_content: String,
    ) -> Result<(), MemoryError> {
        let this = self.clone();
        spawn_memdir(move || file::restore_content(&this, file, &previous_content)).await
    }

    #[cfg(feature = "threat-scanner")]
    async fn emit_threat_events(&self, content: &str, report: &crate::ThreatScanReport) {
        if report.hits.is_empty() {
            return;
        }
        for hit in &report.hits {
            self.record_metric(MemoryMetric::ThreatDetected {
                category: hit.category,
                action: hit.action,
            });
        }
        let (Some(sink), Some(scope)) = (&self.event_sink, self.event_scope) else {
            return;
        };
        let content_hash = threat_content_hash(content);
        for hit in &report.hits {
            sink.emit(Event::MemoryThreatDetected(MemoryThreatDetectedEvent {
                session_id: scope.session_id,
                run_id: scope.run_id,
                pattern_id: hit.pattern_id.clone(),
                category: hit.category,
                severity: hit.severity,
                action: hit.action,
                direction: ThreatDirection::OnWrite,
                provider_id: Some("builtin-memdir".to_owned()),
                content_hash: content_hash.clone(),
                at: Utc::now(),
            }))
            .await;
        }
    }

    pub(crate) fn record_metric(&self, metric: MemoryMetric) {
        if let Some(sink) = &self.metrics_sink {
            sink.record(metric);
        }
    }

    pub(crate) fn record_memdir_bytes(&self, file: MemdirFile, bytes: u64) {
        self.record_metric(MemoryMetric::MemdirBytes { file, bytes });
    }
}

fn kind_for_file(file: MemdirFile) -> MemoryKind {
    match file {
        MemdirFile::User => MemoryKind::UserPreference,
        _ => MemoryKind::ProjectFact,
    }
}

fn action_for_edit(edit: file::Edit, section: String) -> MemoryWriteAction {
    match edit {
        file::Edit::Append => MemoryWriteAction::AppendSection { section },
        file::Edit::Replace => MemoryWriteAction::ReplaceSection { section },
        file::Edit::Delete => MemoryWriteAction::DeleteSection { section },
    }
}

async fn spawn_memdir<T>(
    op: impl FnOnce() -> Result<T, MemoryError> + Send + 'static,
) -> Result<T, MemoryError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|error| MemoryError::Io(format!("memdir task failed: {error}")))?
}
