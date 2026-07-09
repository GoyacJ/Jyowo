//! Stable persisted config DTOs for global and project configuration storage.
//!
//! These types are the canonical serialization schema for config files under
//! `~/.jyowo/config/` and `<workspace>/.jyowo/config/`. Desktop-shell DTOs may
//! wrap them for IPC camelCase shape, but the persisted schema is owned
//! here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A provider profile definition stored in `~/.jyowo/config/provider-profiles.json`.
/// Secrets are stored separately in `provider-secrets.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProfileDefinition {
    pub id: String,
    pub display_name: String,
    pub provider_id: String,
    pub model_id: String,
    pub protocol: crate::ModelProtocol,
    #[serde(default, skip_serializing_if = "crate::ModelRequestOptions::is_empty")]
    pub model_options: crate::ModelRequestOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub model_descriptor: ProviderProfileModelDescriptor,
}

/// Model descriptor embedded in a provider profile definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProfileModelDescriptor {
    pub protocol: crate::ModelProtocol,
    pub context_window: u32,
    pub display_name: String,
    pub lifecycle: ProviderProfileModelLifecycle,
    pub max_output_tokens: u32,
    pub model_id: String,
    pub provider_id: String,
    pub conversation_capability: ProviderProfileConversationCapability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_semantics: Option<ProviderRuntimeSemanticsDescriptor>,
}

/// Private model runtime behavior stored with provider profiles.
///
/// This is persisted config, not public catalog metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderRuntimeSemanticsDescriptor {
    pub protocol: crate::ModelProtocol,
    pub tool_protocol: String,
    pub reasoning_protocol: ProviderRuntimeReasoningProtocolDescriptor,
    pub streaming_protocol: String,
    pub cache_protocol: String,
    pub media_protocol: String,
    pub output_protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_continuation_dialect: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum ProviderRuntimeReasoningProtocolDescriptor {
    None,
    PublicThinking,
    PublicSummary,
    ProviderPrivateReplay {
        continuation_kind: String,
        required_for_assistant_tool_replay: bool,
    },
}

/// Conversation capability embedded in a provider profile definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProfileConversationCapability {
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub streaming: bool,
    pub tool_calling: bool,
    pub reasoning: bool,
    pub prompt_cache: bool,
    pub structured_output: bool,
}

/// Model lifecycle embedded in a provider profile definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProviderProfileModelLifecycle {
    Stable,
    Preview,
    Retiring { retirement_date: String },
}

/// Global provider secrets, stored in `~/.jyowo/config/provider-secrets.json`.
/// Each entry maps a `config_id` to its secret material.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderSecretsRecord {
    #[serde(default)]
    pub entries: Vec<ProviderSecretEntry>,
}

/// A single provider secret entry keyed by `config_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderSecretEntry {
    pub config_id: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_quota_api_key: Option<String>,
}

/// Redacted metadata for a provider secret. Safe for frontend display and IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretMetadata {
    pub config_id: String,
    pub has_api_key: bool,
    pub has_official_quota_api_key: bool,
}

/// Global provider/model selection for no-workspace conversations.
/// Stored in `~/.jyowo/config/provider-selection.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderSelectionRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_config_id: Option<String>,
}

/// Global execution defaults. Stored in `~/.jyowo/config/execution-defaults.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionDefaultsRecord {
    #[serde(default = "default_permission_mode")]
    pub permission_mode: crate::PermissionMode,
    #[serde(default = "default_tool_profile_full")]
    pub tool_profile: crate::ToolProfile,
    #[serde(default = "default_context_compression_trigger_ratio")]
    pub context_compression_trigger_ratio: f32,
    #[serde(default)]
    pub subagents_enabled: bool,
    #[serde(default)]
    pub agent_teams_enabled: bool,
    #[serde(default)]
    pub background_agents_enabled: bool,
}

fn default_permission_mode() -> crate::PermissionMode {
    crate::PermissionMode::Default
}

fn default_context_compression_trigger_ratio() -> f32 {
    0.8
}

fn default_tool_profile_full() -> crate::ToolProfile {
    crate::ToolProfile::Full
}

impl Default for ExecutionDefaultsRecord {
    fn default() -> Self {
        Self {
            permission_mode: crate::PermissionMode::Default,
            tool_profile: crate::ToolProfile::Full,
            context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        }
    }
}

/// Project execution overrides. Stored in `<workspace>/.jyowo/config/execution-overrides.json`.
///
/// Missing fields inherit from global defaults. `Some` fields explicitly override.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionOverridesRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<crate::PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_profile: Option<crate::ToolProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_compression_trigger_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagents_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_teams_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_agents_enabled: Option<bool>,
}

impl From<ExecutionDefaultsRecord> for ExecutionOverridesRecord {
    fn from(record: ExecutionDefaultsRecord) -> Self {
        Self {
            permission_mode: Some(record.permission_mode),
            tool_profile: Some(record.tool_profile),
            context_compression_trigger_ratio: Some(record.context_compression_trigger_ratio),
            subagents_enabled: Some(record.subagents_enabled),
            agent_teams_enabled: Some(record.agent_teams_enabled),
            background_agents_enabled: Some(record.background_agents_enabled),
        }
    }
}

/// A global MCP preset definition stored in `~/.jyowo/config/mcp-presets.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpPresetRecord {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub transport: McpPresetTransport,
}

/// Transport configuration for an MCP preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum McpPresetTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        working_dir: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: Vec<McpPresetHeader>,
        #[serde(default)]
        headers_from_env: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bearer_token_env_var: Option<String>,
    },
}

/// A header entry in an MCP preset HTTP transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpPresetHeader {
    pub key: String,
    pub value: String,
}

/// Global skill enabled selection stored in `~/.jyowo/config/skills.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillSelectionRecord {
    #[serde(default)]
    pub enabled: Vec<String>,
}

/// Project plugin enabled selection stored in `<workspace>/.jyowo/config/plugins.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginSelectionRecord {
    #[serde(default)]
    pub allow_project_plugins: bool,
    #[serde(default)]
    pub enabled: Vec<String>,
}

/// Project agent profile selection stored in `<workspace>/.jyowo/config/agent-profile-selection.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentProfileSelectionRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_secrets_record_serde_roundtrip() {
        let record = ProviderSecretsRecord {
            entries: vec![
                ProviderSecretEntry {
                    config_id: "config-1".to_owned(),
                    api_key: "sk-secret".to_owned(),
                    official_quota_api_key: Some("quota-key".to_owned()),
                },
                ProviderSecretEntry {
                    config_id: "config-2".to_owned(),
                    api_key: "sk-another".to_owned(),
                    official_quota_api_key: None,
                },
            ],
        };
        let json = serde_json::to_string_pretty(&record).expect("serialize");
        let parsed: ProviderSecretsRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].config_id, "config-1");
        assert_eq!(parsed.entries[0].api_key, "sk-secret");
        assert_eq!(
            parsed.entries[0].official_quota_api_key,
            Some("quota-key".to_owned())
        );
    }

    #[test]
    fn provider_profile_definition_rejects_unknown_fields() {
        let json = r#"{
            "id": "p1",
            "displayName": "My Profile",
            "providerId": "openai",
            "modelId": "gpt-5",
            "protocol": "openai",
            "modelDescriptor": {
                "protocol": "openai",
                "contextWindow": 128000,
                "displayName": "GPT-5",
                "lifecycle": { "kind": "Stable" },
                "maxOutputTokens": 16384,
                "modelId": "gpt-5",
                "providerId": "openai",
                "conversationCapability": {
                    "inputModalities": ["text"],
                    "outputModalities": ["text"],
                    "contextWindow": 128000,
                    "maxOutputTokens": 16384,
                    "streaming": true,
                    "toolCalling": true,
                    "reasoning": true,
                    "promptCache": false,
                    "structuredOutput": true
                }
            },
            "extraField": "should fail"
        }"#;
        let result = serde_json::from_str::<ProviderProfileDefinition>(json);
        assert!(result.is_err());
    }

    #[test]
    fn execution_defaults_record_default_values() {
        let defaults = ExecutionDefaultsRecord::default();
        assert_eq!(defaults.permission_mode, crate::PermissionMode::Default);
        assert_eq!(defaults.tool_profile, crate::ToolProfile::Full);
        assert!((defaults.context_compression_trigger_ratio - 0.8).abs() < f32::EPSILON);
        assert!(!defaults.subagents_enabled);
        assert!(!defaults.agent_teams_enabled);
        assert!(!defaults.background_agents_enabled);
    }

    #[test]
    fn execution_defaults_record_serde_roundtrip() {
        let record = ExecutionDefaultsRecord {
            permission_mode: crate::PermissionMode::Auto,
            tool_profile: crate::ToolProfile::Minimal,
            context_compression_trigger_ratio: 0.75,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        };
        let json = serde_json::to_string_pretty(&record).expect("serialize");
        let parsed: ExecutionDefaultsRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.permission_mode, crate::PermissionMode::Auto);
        assert_eq!(parsed.tool_profile, crate::ToolProfile::Minimal);
        assert!((parsed.context_compression_trigger_ratio - 0.75).abs() < f32::EPSILON);
        assert!(parsed.subagents_enabled);
        assert!(!parsed.agent_teams_enabled);
        assert!(parsed.background_agents_enabled);
    }

    #[test]
    fn skill_selection_record_defaults_to_empty() {
        let record = SkillSelectionRecord::default();
        assert!(record.enabled.is_empty());
    }

    #[test]
    fn mcp_preset_record_serde_roundtrip() {
        let preset = McpPresetRecord {
            id: "browser".to_owned(),
            display_name: "Browser".to_owned(),
            description: "A browser MCP server".to_owned(),
            transport: McpPresetTransport::Http {
                url: "http://localhost:3000".to_owned(),
                headers: vec![],
                headers_from_env: vec![],
                bearer_token_env_var: None,
            },
        };
        let json = serde_json::to_string_pretty(&preset).expect("serialize");
        let parsed: McpPresetRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, "browser");
        assert!(matches!(parsed.transport, McpPresetTransport::Http { .. }));
    }

    #[test]
    fn provider_selection_record_defaults_to_no_selection() {
        let record = ProviderSelectionRecord::default();
        assert_eq!(record.default_config_id, None);
    }

    #[test]
    fn provider_secret_metadata_is_safe_for_display() {
        let metadata = ProviderSecretMetadata {
            config_id: "config-1".to_owned(),
            has_api_key: true,
            has_official_quota_api_key: false,
        };
        let json = serde_json::to_string(&metadata).expect("serialize");
        // Must not contain raw secret values
        assert!(!json.contains("sk-"));
        assert!(json.contains("hasApiKey"));
        assert!(json.contains("hasOfficialQuotaApiKey"));
    }
}
