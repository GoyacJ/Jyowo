use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use harness_contracts::{
    AgentId, HookEventKind, HookFailureMode, McpServerId, PluginId, SkillId, SkillSourceKind,
    TrustLevel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub raw_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub allowlist_agents: Option<Vec<String>>,
    pub parameters: Vec<SkillParameter>,
    pub config: Vec<SkillConfigDecl>,
    pub scripts: Vec<SkillScriptDecl>,
    pub platforms: Vec<SkillPlatform>,
    pub prerequisites: SkillPrerequisites,
    pub hooks: Vec<SkillHookDecl>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub metadata: HashMap<String, Value>,
}

pub const DEFAULT_SKILL_SCRIPT_TIMEOUT_SECONDS: u64 = 30;
pub const MAX_SKILL_SCRIPT_TIMEOUT_SECONDS: u64 = 300;
pub const DEFAULT_SKILL_SCRIPT_STDOUT_BYTES: u64 = 64 * 1024;
pub const DEFAULT_SKILL_SCRIPT_STDERR_BYTES: u64 = 64 * 1024;
pub const DEFAULT_SKILL_SCRIPT_OUTPUT_BYTES: u64 = 128 * 1024;
pub const DEFAULT_SKILL_SCRIPT_ARTIFACT_COUNT: u64 = 16;
pub const DEFAULT_SKILL_SCRIPT_ARTIFACT_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_SKILL_SCRIPT_STREAM_BYTES: u64 = 1024 * 1024;
pub const MAX_SKILL_SCRIPT_OUTPUT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_SKILL_SCRIPT_ARTIFACT_COUNT: u64 = 128;
pub const MAX_SKILL_SCRIPT_ARTIFACT_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillScriptDecl {
    pub id: String,
    pub path: PathBuf,
    pub timeout_seconds: u64,
    pub network: SkillScriptNetworkPolicy,
    pub env: BTreeMap<String, SkillScriptEnvDecl>,
    pub max_stdout_bytes: u64,
    pub max_stderr_bytes: u64,
    pub max_output_bytes: u64,
    pub max_artifact_count: u64,
    pub max_artifact_bytes: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScriptNetworkPolicy {
    Deny,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillScriptEnvDecl {
    pub config: String,
    pub secret: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SkillParameter {
    pub name: String,
    pub param_type: SkillParamType,
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SkillParamType {
    String,
    Number,
    Boolean,
    Path,
    Url,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillConfigDecl {
    pub key: String,
    pub value_type: SkillParamType,
    pub secret: bool,
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SkillPlatform {
    Macos,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct SkillPrerequisites {
    pub env_vars: Vec<String>,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillHookDecl {
    pub id: String,
    pub events: Vec<HookEventKind>,
    pub transport: SkillHookTransport,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillHookTransport {
    Builtin(BuiltinHookKind),
    Exec(SkillHookExecSpec),
    Http(SkillHookHttpSpec),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum BuiltinHookKind {
    AuditLog,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillHookExecSpec {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub failure_mode: HookFailureMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillHookHttpSpec {
    pub url: String,
    pub timeout_ms: u64,
    pub allowlist: Vec<String>,
    pub security: SkillHookHttpSecuritySpec,
    pub failure_mode: HookFailureMode,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SkillHookHttpSecuritySpec {
    pub allowlist: Vec<String>,
    pub ssrf_guard: bool,
    pub max_redirects: usize,
    pub max_body_bytes: u64,
    pub mtls_required: bool,
}

impl Default for SkillHookHttpSecuritySpec {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            ssrf_guard: true,
            max_redirects: 0,
            max_body_bytes: 1024 * 1024,
            mtls_required: false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum SkillSource {
    Bundled,
    Workspace(PathBuf),
    User(PathBuf),
    Plugin {
        plugin_id: PluginId,
        trust: TrustLevel,
    },
    Mcp(McpServerId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillRegistration {
    pub skill: Skill,
    pub force_allowlist: Option<Vec<AgentId>>,
}

impl SkillSource {
    #[must_use]
    pub fn to_kind(&self) -> SkillSourceKind {
        match self {
            Self::Bundled => SkillSourceKind::Bundled,
            Self::Workspace(_) => SkillSourceKind::Workspace,
            Self::User(_) => SkillSourceKind::User,
            Self::Plugin { plugin_id, .. } => SkillSourceKind::Plugin(plugin_id.clone()),
            Self::Mcp(server_id) => SkillSourceKind::Mcp(server_id.clone()),
        }
    }

    #[must_use]
    pub fn trust_level(&self) -> TrustLevel {
        match self {
            Self::Bundled => TrustLevel::AdminTrusted,
            Self::Plugin { trust, .. } => *trust,
            Self::Workspace(_) | Self::User(_) | Self::Mcp(_) => TrustLevel::UserControlled,
        }
    }
}

impl SkillPlatform {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "macos" => Some(Self::Macos),
            "linux" => Some(Self::Linux),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

impl SkillParamType {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "string" => Some(Self::String),
            "number" => Some(Self::Number),
            "boolean" => Some(Self::Boolean),
            "path" => Some(Self::Path),
            "url" => Some(Self::Url),
            _ => None,
        }
    }
}
