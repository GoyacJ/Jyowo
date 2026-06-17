use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillLoadedEvent {
    pub session_id: Option<SessionId>,
    pub skill_id: SkillId,
    pub skill_name: String,
    pub source: SkillSourceKind,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillRejectedEvent {
    pub session_id: Option<SessionId>,
    pub skill_name: Option<String>,
    pub source: SkillSourceKind,
    pub reason: SkillRejectionReason,
    pub detail: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillRejectionReason {
    ParseFrontmatter,
    NameTooLong,
    DescriptionTooLong,
    PlatformMismatch,
    ThreatDetected,
    HookTransportNotPermitted,
    Duplicate,
    InvalidConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillThreatDetectedEvent {
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub skill_id: Option<SkillId>,
    pub skill_name: Option<String>,
    pub pattern_id: String,
    pub category: ThreatCategory,
    pub severity: Severity,
    pub action: ThreatAction,
    pub content_hash: ContentHash,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillInvokedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub skill_id: SkillId,
    pub skill_name: String,
    pub injection_id: SkillInjectionId,
    pub bytes_injected: u64,
    pub consumed_config_keys: Vec<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillPrerequisiteMissingEvent {
    pub session_id: Option<SessionId>,
    pub skill_id: SkillId,
    pub skill_name: String,
    pub env_vars: Vec<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillPrerequisiteAdvisoryEvent {
    pub session_id: Option<SessionId>,
    pub skill_id: SkillId,
    pub skill_name: String,
    pub commands: Vec<String>,
    pub at: DateTime<Utc>,
}
