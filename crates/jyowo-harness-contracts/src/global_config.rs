//! Stable persisted config DTOs for global and project configuration storage.
//!
//! These types are the canonical serialization schema for config files under
//! `~/.jyowo/config/` and `<workspace>/.jyowo/config/`. Desktop-shell DTOs may
//! wrap them for IPC camelCase shape, but the persisted schema is owned
//! here.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Global non-secret skill configuration stored in
/// `~/.jyowo/config/skill-config.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillConfigDocument {
    pub version: u32,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillConfigEntry>,
}

impl SkillConfigDocument {
    pub const CURRENT_VERSION: u32 = 1;
}

impl Default for SkillConfigDocument {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            skills: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillConfigEntry {
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
    #[serde(default)]
    pub secrets: BTreeMap<String, SkillSecretMetadata>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillSecretMetadata {
    pub configured: bool,
}

/// A provider profile definition stored in `~/.jyowo/config/provider-profiles.json`.
/// Secrets are stored separately in `provider-secrets.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_defaults: Option<ProviderProfileDefaults>,
    pub model_descriptor: ProviderProfileModelDescriptor,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProfileDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_settings: BTreeMap<String, crate::ToolRuntimeSettings>,
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
            tool_settings: BTreeMap::new(),
            context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionDefaultsValidationError {
    AgentTeamsRequireSubagents,
    BackgroundAgentsRequireSubagents,
}

impl std::fmt::Display for ExecutionDefaultsValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentTeamsRequireSubagents => {
                formatter.write_str("agent teams require subagents to be enabled")
            }
            Self::BackgroundAgentsRequireSubagents => {
                formatter.write_str("background agents require subagents to be enabled")
            }
        }
    }
}

impl std::error::Error for ExecutionDefaultsValidationError {}

pub fn validate_execution_defaults_dependencies(
    record: &ExecutionDefaultsRecord,
) -> Result<(), ExecutionDefaultsValidationError> {
    if record.agent_teams_enabled && !record.subagents_enabled {
        return Err(ExecutionDefaultsValidationError::AgentTeamsRequireSubagents);
    }
    if record.background_agents_enabled && !record.subagents_enabled {
        return Err(ExecutionDefaultsValidationError::BackgroundAgentsRequireSubagents);
    }
    Ok(())
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_settings: BTreeMap<String, crate::ToolRuntimeSettings>,
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
            tool_settings: record.tool_settings,
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

/// Canonical persisted MCP server configuration.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpServerConfigRecord {
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    pub display_name: String,
    pub id: String,
    pub scope: String,
    pub transport: McpServerTransportConfig,
}

impl std::fmt::Debug for McpServerConfigRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpServerConfigRecord")
            .field("enabled", &self.enabled)
            .field("required", &self.required)
            .field("display_name", &self.display_name)
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("transport", &self.transport)
            .finish()
    }
}

/// A persisted MCP key/value pair whose value must remain redacted in diagnostics.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpNameValueRecord {
    pub key: String,
    pub value: String,
}

impl std::fmt::Debug for McpNameValueRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpNameValueRecord")
            .field("key", &self.key)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// A persisted HTTP header resolved from an environment variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpHeaderEnvRecord {
    pub key: String,
    pub env_var: String,
}

/// Canonical persisted MCP transport. In-process is deserializable for fail-closed
/// migration of old files, but is rejected by [`validate_persisted_mcp_server`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum McpServerTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<McpNameValueRecord>,
        #[serde(default)]
        inherit_env: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token_env_var: Option<String>,
        #[serde(default)]
        headers: Vec<McpNameValueRecord>,
        #[serde(default)]
        headers_from_env: Vec<McpHeaderEnvRecord>,
    },
    InProcess,
}

impl std::fmt::Debug for McpServerTransportConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio {
                command,
                args,
                env,
                inherit_env,
                working_dir,
            } => formatter
                .debug_struct("Stdio")
                .field("command", command)
                .field("args", args)
                .field("env", env)
                .field("inherit_env", inherit_env)
                .field("working_dir", working_dir)
                .finish(),
            Self::Http {
                url,
                bearer_token_env_var,
                headers,
                headers_from_env,
            } => formatter
                .debug_struct("Http")
                .field("url", url)
                .field("bearer_token_env_var", bearer_token_env_var)
                .field("headers", headers)
                .field("headers_from_env", headers_from_env)
                .finish(),
            Self::InProcess => formatter.write_str("InProcess"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PersistedMcpValidationError {
    #[error("invalid persisted MCP server identity")]
    Identity,
    #[error("invalid persisted MCP server scope")]
    Scope,
    #[error("invalid persisted MCP stdio transport")]
    Stdio,
    #[error("invalid persisted MCP HTTP transport")]
    Http,
    #[error("persisted MCP server cannot use in-process transport")]
    InProcess,
}

pub fn validate_persisted_mcp_server_identity(
    record: &McpServerConfigRecord,
) -> Result<(), PersistedMcpValidationError> {
    if record.display_name.trim().is_empty()
        || record.display_name.len() > 256
        || record.display_name.contains('\0')
        || !valid_mcp_server_id(&record.id)
    {
        return Err(PersistedMcpValidationError::Identity);
    }
    Ok(())
}

pub fn validate_persisted_mcp_server(
    record: &McpServerConfigRecord,
) -> Result<(), PersistedMcpValidationError> {
    validate_persisted_mcp_server_identity(record)?;
    if !matches!(record.scope.as_str(), "agent" | "global" | "session") {
        return Err(PersistedMcpValidationError::Scope);
    }
    validate_persisted_mcp_transport(&record.transport)
}

pub fn validate_persisted_mcp_transport(
    transport: &McpServerTransportConfig,
) -> Result<(), PersistedMcpValidationError> {
    match transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => {
            if command.trim().is_empty()
                || command.len() > 4096
                || command.contains('\0')
                || args.len() > 64
                || args
                    .iter()
                    .any(|arg| arg.trim().is_empty() || arg.len() > 4096 || arg.contains('\0'))
                || env.len() > 64
                || inherit_env.len() > 128
            {
                return Err(PersistedMcpValidationError::Stdio);
            }
            for item in env {
                if !valid_env_var_name(&item.key)
                    || item.value.len() > 4096
                    || item.value.contains('\0')
                    || mcp_name_looks_secret_bearing(&item.key)
                    || looks_like_raw_secret(&item.value)
                {
                    return Err(PersistedMcpValidationError::Stdio);
                }
            }
            if inherit_env
                .iter()
                .any(|item| !valid_env_var_name(item) || mcp_name_looks_secret_bearing(item))
            {
                return Err(PersistedMcpValidationError::Stdio);
            }
            if working_dir.as_ref().is_some_and(|directory| {
                directory.trim().is_empty() || directory.len() > 4096 || directory.contains('\0')
            }) {
                return Err(PersistedMcpValidationError::Stdio);
            }
        }
        McpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => {
            let parsed = url::Url::parse(url).map_err(|_| PersistedMcpValidationError::Http)?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || bearer_token_env_var
                    .as_ref()
                    .is_some_and(|name| !valid_env_var_name(name))
                || headers.len() > 64
                || headers_from_env.len() > 64
            {
                return Err(PersistedMcpValidationError::Http);
            }
            if parsed.query_pairs().any(|(key, value)| {
                mcp_name_looks_secret_bearing(&key) || looks_like_raw_secret(&value)
            }) {
                return Err(PersistedMcpValidationError::Http);
            }
            for header in headers {
                if !valid_http_header_name(&header.key)
                    || header.value.len() > 8192
                    || !valid_http_header_value(&header.value)
                    || mcp_http_header_is_sensitive(&header.key)
                    || looks_like_raw_secret(&header.value)
                    || mcp_header_value_looks_secret_bearing(&header.value)
                {
                    return Err(PersistedMcpValidationError::Http);
                }
            }
            for header in headers_from_env {
                if !valid_http_header_name(&header.key)
                    || !valid_env_var_name(&header.env_var)
                    || mcp_http_header_is_sensitive(&header.key)
                {
                    return Err(PersistedMcpValidationError::Http);
                }
            }
        }
        McpServerTransportConfig::InProcess => {
            return Err(PersistedMcpValidationError::InProcess);
        }
    }
    Ok(())
}

fn valid_mcp_server_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .enumerate()
            .all(|(index, character)| match character {
                'A'..='Z' | 'a'..='z' | '0'..='9' => true,
                '.' | '-' | '_' if index > 0 => true,
                _ => false,
            })
}

fn valid_env_var_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn valid_http_header_name(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn valid_http_header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || (byte >= b' ' && byte != 0x7f))
}

fn mcp_name_looks_secret_bearing(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('-', "_");
    [
        "auth",
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn mcp_http_header_is_sensitive(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie" | "proxy-authorization"
    )
}

fn mcp_header_value_looks_secret_bearing(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("bearer ")
        || normalized.starts_with("oauth ")
        || normalized.contains(" token")
        || normalized.contains("secret")
        || normalized.contains("password")
}

fn looks_like_raw_secret(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    let known_prefix = [
        "ghp_",
        "github_pat_",
        "glpat-",
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    known_prefix
        || (trimmed.len() >= 32
            && trimmed.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '_' | '-' | '.' | '=' | '/' | '+')
            }))
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
        assert!(defaults.tool_settings.is_empty());
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
            tool_settings: BTreeMap::from([(
                "WebFetch".to_owned(),
                crate::ToolRuntimeSettings {
                    timeout_ms: 45_000,
                    parameters: serde_json::json!({ "defaultMaxBytes": 128_000 }),
                },
            )]),
            context_compression_trigger_ratio: 0.75,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        };
        let json = serde_json::to_string_pretty(&record).expect("serialize");
        let parsed: ExecutionDefaultsRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.permission_mode, crate::PermissionMode::Auto);
        assert_eq!(parsed.tool_profile, crate::ToolProfile::Minimal);
        assert_eq!(parsed.tool_settings, record.tool_settings);
        assert!((parsed.context_compression_trigger_ratio - 0.75).abs() < f32::EPSILON);
        assert!(parsed.subagents_enabled);
        assert!(!parsed.agent_teams_enabled);
        assert!(parsed.background_agents_enabled);
    }

    #[test]
    fn execution_defaults_without_tool_settings_remain_compatible() {
        let parsed: ExecutionDefaultsRecord = serde_json::from_value(serde_json::json!({
            "permissionMode": "default",
            "toolProfile": "full"
        }))
        .expect("legacy execution defaults should deserialize");

        assert!(parsed.tool_settings.is_empty());
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
    fn mcp_server_record_missing_required_defaults_to_optional() {
        let record: McpServerConfigRecord = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "displayName": "Browser",
            "id": "browser",
            "scope": "global",
            "transport": {
                "kind": "stdio",
                "command": "node",
                "args": ["server.js"]
            }
        }))
        .expect("deserialize legacy MCP server record");

        assert!(!record.required);
    }

    #[test]
    fn mcp_server_record_required_policy_roundtrips() {
        let record: McpServerConfigRecord = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "required": true,
            "displayName": "Browser",
            "id": "browser",
            "scope": "global",
            "transport": {
                "kind": "stdio",
                "command": "node",
                "args": ["server.js"]
            }
        }))
        .expect("deserialize required MCP server record");

        assert!(record.required);
        let serialized = serde_json::to_value(record).expect("serialize MCP server record");
        assert_eq!(serialized["required"], true);
    }

    #[test]
    fn persisted_mcp_rejects_nul_and_http_header_controls() {
        let invalid_identity = McpServerConfigRecord {
            enabled: true,
            required: false,
            display_name: "Browser\0hidden".to_owned(),
            id: "browser".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec![],
                env: vec![],
                inherit_env: vec![],
                working_dir: None,
            },
        };
        assert_eq!(
            validate_persisted_mcp_server_identity(&invalid_identity),
            Err(PersistedMcpValidationError::Identity)
        );

        let stdio_cases = [
            McpServerTransportConfig::Stdio {
                command: "node\0hidden".to_owned(),
                args: vec![],
                env: vec![],
                inherit_env: vec![],
                working_dir: None,
            },
            McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["server\0hidden".to_owned()],
                env: vec![],
                inherit_env: vec![],
                working_dir: None,
            },
            McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec![],
                env: vec![McpNameValueRecord {
                    key: "LOG_LEVEL".to_owned(),
                    value: "info\0hidden".to_owned(),
                }],
                inherit_env: vec![],
                working_dir: None,
            },
        ];
        for transport in stdio_cases {
            assert_eq!(
                validate_persisted_mcp_transport(&transport),
                Err(PersistedMcpValidationError::Stdio)
            );
        }

        for value in [
            "first\nsecond",
            "first\rsecond",
            "value\0hidden",
            "value\u{7f}",
        ] {
            let transport = McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueRecord {
                    key: "X-Value".to_owned(),
                    value: value.to_owned(),
                }],
                headers_from_env: vec![],
            };
            assert_eq!(
                validate_persisted_mcp_transport(&transport),
                Err(PersistedMcpValidationError::Http)
            );
        }
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
