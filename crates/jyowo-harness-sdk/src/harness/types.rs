use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantPolicy {
    #[serde(default = "default_tenant_id")]
    pub id: TenantId,
    #[serde(default = "default_display_name")]
    pub display_name: String,
    #[serde(default)]
    pub allowed_tools: Option<HashSet<String>>,
    #[serde(default)]
    pub allowed_providers: Option<HashSet<String>>,
    #[serde(default)]
    pub max_concurrent_sessions: Option<u32>,
    #[serde(default)]
    pub event_retention_days: Option<u32>,
    #[serde(default)]
    pub allow_scoped_tenants: bool,
}

impl Default for TenantPolicy {
    fn default() -> Self {
        Self {
            id: TenantId::SINGLE,
            display_name: "default".to_owned(),
            allowed_tools: None,
            allowed_providers: None,
            max_concurrent_sessions: None,
            event_retention_days: None,
            allow_scoped_tenants: false,
        }
    }
}

#[derive(Clone)]
pub struct McpConfig {
    pub registry: McpRegistry,
    pub server_ids_to_inject: Vec<McpServerId>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            registry: McpRegistry::new(),
            server_ids_to_inject: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillParameter {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillSummary {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub source: harness_contracts::SkillSourceKind,
    pub status: harness_contracts::SkillStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillView {
    pub summary: RuntimeSkillSummary,
    pub parameters: Vec<RuntimeSkillParameter>,
    pub config_keys: Vec<String>,
    pub body_preview: String,
    pub body_full: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessOptions {
    #[serde(default = "default_workspace_root")]
    pub workspace_root: PathBuf,
    #[serde(default = "default_model_id")]
    pub model_id: String,
    #[serde(default = "default_tool_search_enabled")]
    pub tool_search_enabled: bool,
    #[serde(default)]
    pub tenant_policy: TenantPolicy,
    #[serde(default)]
    pub default_session_options: SessionOptions,
    #[serde(default)]
    pub concurrent_sessions: Option<u32>,
    #[serde(default)]
    pub enable_replay: bool,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            workspace_root: workspace_root.clone(),
            model_id: "default".to_owned(),
            tool_search_enabled: true,
            tenant_policy: TenantPolicy::default(),
            default_session_options: SessionOptions::new(workspace_root),
            concurrent_sessions: None,
            enable_replay: false,
        }
    }
}

fn default_tenant_id() -> TenantId {
    TenantId::SINGLE
}

fn default_display_name() -> String {
    "default".to_owned()
}

fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn default_model_id() -> String {
    "default".to_owned()
}

fn default_tool_search_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSession {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub message_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSessionSummary {
    pub session_id: SessionId,
    pub created_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    pub event_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationTurnRequest {
    pub options: SessionOptions,
    pub input: ConversationTurnInput,
    pub permission_mode_override: Option<PermissionMode>,
}

impl ConversationTurnRequest {
    #[must_use]
    pub fn from_prompt(options: SessionOptions, prompt: impl Into<String>) -> Self {
        Self {
            options,
            input: ConversationTurnInput::ask(prompt),
            permission_mode_override: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTurnReceipt {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub message_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationEventsPageRequest {
    pub options: SessionOptions,
    pub after_event_id: Option<EventId>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationEventsPage {
    pub events: Vec<EventEnvelope>,
    pub next_event_id: Option<EventId>,
}
