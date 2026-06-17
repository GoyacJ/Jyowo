use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use harness_contracts::{Decision, DecisionScope, PermissionError, RuleSource, TenantId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleSnapshot {
    pub rules: Vec<PermissionRule>,
    pub generation: u64,
    pub built_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub priority: i32,
    pub scope: DecisionScope,
    pub action: RuleAction,
    pub source: RuleSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Allow,
    Deny,
    AskWithDefault(Decision),
}

#[async_trait]
pub trait RuleProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &str;

    fn source(&self) -> RuleSource;

    async fn resolve_rules(&self, tenant: TenantId)
        -> Result<Vec<PermissionRule>, PermissionError>;

    fn watch(&self) -> Option<BoxStream<'static, RulesUpdated>>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RulesUpdated {
    pub provider_id: String,
    pub tenant_id: TenantId,
    pub new_rules: Vec<PermissionRule>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverrideDecision {
    FromHook {
        handler_id: String,
        decision: Decision,
    },
}
