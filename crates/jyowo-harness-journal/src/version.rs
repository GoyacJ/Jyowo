//! Event schema migration interfaces.
//!
//! SPEC: docs/architecture/harness/event-schema.md §7.2.1

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use harness_contracts::{
    CausationId, CorrelationId, Event, EventId, ForkReason, JournalError, JournalOffset, RunId,
    SessionId, TenantId,
};
use serde::{Deserialize, Serialize};

use crate::{
    journal_error, AppendMetadata, EventStore, PrunePolicy, PruneReport, ReplayCursor,
    SessionFilter, SessionSnapshot, SessionSummary,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    pub const CURRENT: Self = Self(1);

    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub offset: JournalOffset,
    pub event_id: EventId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub run_id: Option<RunId>,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
    pub schema_version: SchemaVersion,
    pub recorded_at: DateTime<Utc>,
    pub payload: Event,
}

#[allow(clippy::wrong_self_convention)]
pub trait EventMigrator: Send + Sync + 'static {
    fn from_version(&self) -> SchemaVersion;

    fn to_version(&self) -> SchemaVersion;

    fn migrate(&self, envelope: EventEnvelope) -> Result<EventEnvelope, JournalError>;
}

pub struct MigratorChain {
    migrators: Vec<Box<dyn EventMigrator>>,
}

impl MigratorChain {
    pub fn new(migrators: Vec<Box<dyn EventMigrator>>) -> Self {
        let mut seen = HashSet::new();
        for migrator in &migrators {
            let edge = (migrator.from_version(), migrator.to_version());
            assert!(seen.insert(edge), "duplicate schema migrator edge");
        }
        Self { migrators }
    }

    pub fn find_path(
        &self,
        from: SchemaVersion,
        to: SchemaVersion,
    ) -> Option<Vec<&dyn EventMigrator>> {
        if from == to {
            return Some(Vec::new());
        }

        let mut visited = HashSet::from([from]);
        let mut queue = VecDeque::from([(from, Vec::<usize>::new())]);

        while let Some((version, path)) = queue.pop_front() {
            for (idx, migrator) in self.migrators.iter().enumerate() {
                if migrator.from_version() != version {
                    continue;
                }

                let next = migrator.to_version();
                let mut next_path = path.clone();
                next_path.push(idx);

                if next == to {
                    return Some(
                        next_path
                            .into_iter()
                            .map(|edge_idx| self.migrators[edge_idx].as_ref())
                            .collect(),
                    );
                }

                if visited.insert(next) {
                    queue.push_back((next, next_path));
                }
            }
        }

        None
    }
}

pub struct VersionedEventStore<S: EventStore> {
    pub inner: S,
    pub migrators: Arc<MigratorChain>,
    pub strict: bool,
}

impl<S: EventStore> VersionedEventStore<S> {
    pub fn builder(inner: S) -> VersionedEventStoreBuilder<S> {
        VersionedEventStoreBuilder {
            inner,
            migrators: Vec::new(),
            strict: true,
        }
    }

    fn migrate_envelope(
        &self,
        envelope: EventEnvelope,
    ) -> Result<Option<EventEnvelope>, JournalError> {
        if envelope.schema_version == SchemaVersion::CURRENT {
            return Ok(Some(envelope));
        }
        let Some(path) = self
            .migrators
            .find_path(envelope.schema_version, SchemaVersion::CURRENT)
        else {
            if self.strict {
                return Err(journal_error(format!(
                    "migration path missing: {} -> {}",
                    envelope.schema_version.get(),
                    SchemaVersion::CURRENT.get()
                )));
            }
            return Ok(None);
        };

        let mut migrated = envelope;
        for migrator in path {
            migrated = migrator.migrate(migrated)?;
        }
        Ok(Some(migrated))
    }

    fn migrate_envelopes(
        &self,
        envelopes: Vec<EventEnvelope>,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let mut migrated = Vec::with_capacity(envelopes.len());
        for envelope in envelopes {
            if let Some(envelope) = self.migrate_envelope(envelope)? {
                migrated.push(envelope);
            }
        }
        Ok(migrated)
    }
}

pub struct VersionedEventStoreBuilder<S: EventStore> {
    inner: S,
    migrators: Vec<Box<dyn EventMigrator>>,
    strict: bool,
}

impl<S: EventStore> VersionedEventStoreBuilder<S> {
    #[must_use]
    pub fn with_migrator(mut self, migrator: impl EventMigrator) -> Self {
        self.migrators.push(Box::new(migrator));
        self
    }

    #[must_use]
    pub const fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    pub fn build(self) -> VersionedEventStore<S> {
        VersionedEventStore {
            inner: self.inner,
            migrators: Arc::new(MigratorChain::new(self.migrators)),
            strict: self.strict,
        }
    }
}

#[async_trait]
impl<S: EventStore> EventStore for VersionedEventStore<S> {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.inner.append(tenant, session_id, events).await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.inner
            .append_with_metadata(tenant, session_id, metadata, events)
            .await
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        let envelopes: Vec<_> = self
            .inner
            .read_envelopes(tenant, session_id, cursor)
            .await?
            .collect()
            .await;
        Ok(Box::pin(stream::iter(self.migrate_envelopes(envelopes)?)))
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        self.migrate_envelopes(self.inner.query_after(tenant, after, limit).await?)
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        self.inner.prune(tenant, policy).await
    }
}
