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

/// A rendered skill context delivery was resolved and is ready to be assembled.
///
/// The rendered body is deliberately excluded. Recovery re-renders `reference`
/// and compares the result with `body_hash` before attempting delivery again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillContextPreparedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub delivery_key: String,
    pub reference: ConversationContextReference,
    pub body_hash: ContentHash,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillContextAssembledEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub delivery_key: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillContextProviderAcceptedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub delivery_key: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillContextConsumedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub delivery_key: String,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    #[test]
    fn prepared_skill_context_serializes_reference_and_hash_without_body() {
        let event = Event::SkillContextPrepared(SkillContextPreparedEvent {
            session_id: SessionId::new(),
            run_id: RunId::new(),
            delivery_key: "task:queue:1:0".into(),
            reference: ConversationContextReference::Skill {
                version: CURRENT_CONTEXT_REFERENCE_VERSION,
                skill_id: SkillId("user/review".into()),
                label: "Review".into(),
                parameters: BTreeMap::from([("language".into(), json!("rust"))]),
                source: Some(SkillSourceKind::User),
            },
            body_hash: ContentHash([7; 32]),
            at: now(),
        });

        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["type"], "skill_context_prepared");
        assert_eq!(value["reference"]["kind"], "skill");
        assert_eq!(value["reference"]["parameters"]["language"], "rust");
        assert_eq!(value["reference"]["source"], "user");
        assert!(value.get("body").is_none());
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), event);
    }

    #[test]
    fn prepared_skill_context_rejects_a_rendered_body_field() {
        let value = json!({
            "type": "skill_context_prepared",
            "session_id": SessionId::new(),
            "run_id": RunId::new(),
            "delivery_key": "delivery",
            "reference": {
                "kind": "skill",
                "version": CURRENT_CONTEXT_REFERENCE_VERSION,
                "skillId": "user/review",
                "label": "Review",
                "parameters": {},
                "source": "user"
            },
            "body_hash": vec![0; 32],
            "body": "must not persist",
            "at": now()
        });

        assert!(serde_json::from_value::<Event>(value).is_err());
    }
}
