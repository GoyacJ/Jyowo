use async_trait::async_trait;
use futures::stream::BoxStream;
use harness_contracts::{PermissionError, RuleSource, TenantId};

use crate::{PermissionRule, RuleProvider, RulesUpdated};

#[derive(Debug, Clone)]
pub struct InlineRuleProvider {
    provider_id: String,
    source: RuleSource,
    rules: Vec<PermissionRule>,
}

impl InlineRuleProvider {
    pub fn new(
        provider_id: impl Into<String>,
        source: RuleSource,
        rules: Vec<PermissionRule>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            source,
            rules,
        }
    }
}

#[async_trait]
impl RuleProvider for InlineRuleProvider {
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
        Ok(self.rules.clone())
    }

    fn watch(&self) -> Option<BoxStream<'static, RulesUpdated>> {
        None
    }
}
