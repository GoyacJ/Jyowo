use async_trait::async_trait;
use harness_contracts::{MemoryError, MemoryId, MemoryVisibility};
use tokio::sync::Mutex;

use crate::{
    content_preview, visibility_matches, MemoryKindFilter, MemoryLifecycle, MemoryListScope,
    MemoryQuery, MemoryRecord, MemoryStore, MemorySummary, MemoryVisibilityFilter,
};

pub struct MockMemoryProvider {
    provider_id: String,
    records: Mutex<Vec<MemoryRecord>>,
}

impl MockMemoryProvider {
    #[must_use]
    pub fn new(provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            records: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl MemoryStore for MockMemoryProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        let mut records = self.records.lock().await;
        let now = chrono::Utc::now();
        for record in records
            .iter_mut()
            .filter(|record| record.tenant_id == query.tenant_id)
            .filter(|record| kind_matches(record, query.kind_filter.as_ref()))
            .filter(|record| visibility_filter_matches(record, &query.visibility_filter))
            .take(query.max_records as usize)
        {
            record.metadata.access_count = record.metadata.access_count.saturating_add(1);
            record.metadata.last_accessed_at = Some(now);
        }

        Ok(records
            .iter()
            .filter(|record| record.tenant_id == query.tenant_id)
            .filter(|record| kind_matches(record, query.kind_filter.as_ref()))
            .filter(|record| visibility_filter_matches(record, &query.visibility_filter))
            .take(query.max_records as usize)
            .cloned()
            .collect())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        let id = record.id;
        let mut records = self.records.lock().await;
        if let Some(existing) = records.iter_mut().find(|existing| existing.id == id) {
            *existing = record;
        } else {
            records.push(record);
        }

        Ok(id)
    }

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError> {
        let mut records = self.records.lock().await;
        records.retain(|record| record.id != id);
        Ok(())
    }

    async fn list(&self, scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        let records = self.records.lock().await;
        Ok(records
            .iter()
            .filter(|record| list_scope_matches(record, &scope))
            .map(summary_from_record)
            .collect())
    }
}

impl MemoryLifecycle for MockMemoryProvider {}

fn kind_matches(record: &MemoryRecord, filter: Option<&MemoryKindFilter>) -> bool {
    match filter {
        None | Some(MemoryKindFilter::Any) => true,
        Some(MemoryKindFilter::OnlyKinds(kinds)) => kinds.contains(&record.kind),
    }
}

fn visibility_filter_matches(record: &MemoryRecord, filter: &MemoryVisibilityFilter) -> bool {
    match filter {
        MemoryVisibilityFilter::EffectiveFor(actor) => {
            record.tenant_id == actor.tenant_id && visibility_matches(&record.visibility, actor)
        }
        MemoryVisibilityFilter::Exact(visibility) => &record.visibility == visibility,
    }
}

fn list_scope_matches(record: &MemoryRecord, scope: &MemoryListScope) -> bool {
    match scope {
        MemoryListScope::All => true,
        MemoryListScope::ByKind(kind) => &record.kind == kind,
        MemoryListScope::ByVisibility(visibility) => &record.visibility == visibility,
        MemoryListScope::ForActor(actor) => {
            record.tenant_id == actor.tenant_id && visibility_matches(&record.visibility, actor)
        }
    }
}

fn summary_from_record(record: &MemoryRecord) -> MemorySummary {
    MemorySummary {
        id: record.id,
        kind: record.kind.clone(),
        visibility: clone_visibility(&record.visibility),
        content_preview: content_preview(&record.content),
        metadata: record.metadata.clone(),
        updated_at: record.updated_at,
    }
}

fn clone_visibility(visibility: &MemoryVisibility) -> MemoryVisibility {
    match visibility {
        MemoryVisibility::Private { session_id } => MemoryVisibility::Private {
            session_id: *session_id,
        },
        MemoryVisibility::User { user_id } => MemoryVisibility::User {
            user_id: user_id.clone(),
        },
        MemoryVisibility::Team { team_id } => MemoryVisibility::Team { team_id: *team_id },
        MemoryVisibility::Tenant => MemoryVisibility::Tenant,
        other => other.clone(),
    }
}
