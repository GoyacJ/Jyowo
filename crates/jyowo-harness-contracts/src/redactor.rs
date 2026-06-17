//! Redaction contract.
//!
//! SPEC: docs/architecture/harness/api-contracts.md §18.2

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait Redactor: Send + Sync + 'static {
    fn redact(&self, input: &str, rules: &RedactRules) -> String;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct RedactRules {
    pub scope: RedactScope,
    pub replacement: String,
    pub pattern_set: RedactPatternSet,
}

impl Default for RedactRules {
    fn default() -> Self {
        Self {
            scope: RedactScope::EventBody,
            replacement: "[REDACTED]".to_owned(),
            pattern_set: RedactPatternSet::Default,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactScope {
    All,
    TraceOnly,
    EventBody,
    LogOnly,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactPatternSet {
    Default,
    AllBuiltins,
    Only(Vec<RedactPatternKind>),
    None,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactPatternKind {
    ApiKey,
    BearerToken,
    PrivateKey,
    OAuthCode,
    DatabaseUrl,
    PrivateIp,
    Email,
    Custom(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.to_owned()
    }
}
