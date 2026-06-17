use async_trait::async_trait;
use futures::stream::BoxStream;
use harness_contracts::{PermissionError, RuleSource, TenantId};

use crate::{PermissionRule, RuleProvider, RulesUpdated};

#[derive(Debug, Clone)]
pub struct AdminRuleProvider {
    rules: Vec<PermissionRule>,
}

impl AdminRuleProvider {
    pub fn new(rules: Vec<PermissionRule>) -> Self {
        Self { rules }
    }
}

#[async_trait]
impl RuleProvider for AdminRuleProvider {
    fn provider_id(&self) -> &'static str {
        "admin"
    }

    fn source(&self) -> RuleSource {
        RuleSource::Policy
    }

    async fn resolve_rules(
        &self,
        _tenant: TenantId,
    ) -> Result<Vec<PermissionRule>, PermissionError> {
        if let Some(rule) = self
            .rules
            .iter()
            .find(|rule| rule.source != RuleSource::Policy)
        {
            return Err(PermissionError::Message(format!(
                "admin provider only accepts Policy rules, got {:?} for `{}`",
                rule.source, rule.id
            )));
        }

        Ok(self.rules.clone())
    }

    fn watch(&self) -> Option<BoxStream<'static, RulesUpdated>> {
        None
    }
}
