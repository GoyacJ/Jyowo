use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use futures::StreamExt;
use harness_contracts::{PermissionError, RuleSource, TenantId};
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::{PermissionRule, RuleProvider, RulesUpdated};

#[derive(Debug)]
pub struct InMemoryRuleProvider {
    provider_id: String,
    source: RuleSource,
    rules: Arc<RwLock<Vec<PermissionRule>>>,
    updates: broadcast::Sender<RulesUpdated>,
}

impl InMemoryRuleProvider {
    pub fn new(
        provider_id: impl Into<String>,
        source: RuleSource,
        rules: Vec<PermissionRule>,
    ) -> Self {
        let (updates, _receiver) = broadcast::channel(16);
        Self {
            provider_id: provider_id.into(),
            source,
            rules: Arc::new(RwLock::new(rules)),
            updates,
        }
    }

    pub fn replace_rules(&self, rules: Vec<PermissionRule>) {
        *self.rules.write() = rules.clone();
        let _ = self.updates.send(RulesUpdated {
            provider_id: self.provider_id.clone(),
            tenant_id: TenantId::SHARED,
            new_rules: rules,
            at: Utc::now(),
        });
    }
}

#[async_trait]
impl RuleProvider for InMemoryRuleProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn source(&self) -> RuleSource {
        self.source
    }

    async fn resolve_rules(
        &self,
        _tenant: TenantId,
    ) -> Result<Vec<PermissionRule>, PermissionError> {
        Ok(self.rules.read().clone())
    }

    fn watch(&self) -> Option<BoxStream<'static, RulesUpdated>> {
        Some(
            BroadcastStream::new(self.updates.subscribe())
                .filter_map(|update| async move { update.ok() })
                .boxed(),
        )
    }
}
