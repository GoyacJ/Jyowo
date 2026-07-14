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
    pub event_sink: Arc<dyn McpEventSink>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            registry: McpRegistry::new(),
            server_ids_to_inject: Vec::new(),
            event_sink: Arc::new(NoopMcpEventSink),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpToolInjectionOutcome {
    Injected {
        server_id: McpServerId,
        tool_names: Vec<String>,
    },
    SkippedOptional {
        server_id: McpServerId,
        reason: String,
    },
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
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub source: harness_contracts::SkillSourceKind,
    pub status: harness_contracts::SkillStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillConfig {
    pub key: String,
    pub value_type: String,
    pub secret: bool,
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillScriptEnv {
    pub name: String,
    pub config_key: String,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillScript {
    pub id: String,
    pub path: String,
    pub timeout_seconds: u64,
    pub network: String,
    pub env: Vec<RuntimeSkillScriptEnv>,
    pub max_stdout_bytes: u64,
    pub max_stderr_bytes: u64,
    pub max_output_bytes: u64,
    pub max_artifact_count: u64,
    pub max_artifact_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillView {
    pub summary: RuntimeSkillSummary,
    pub parameters: Vec<RuntimeSkillParameter>,
    pub config: Vec<RuntimeSkillConfig>,
    #[serde(default)]
    pub scripts: Vec<RuntimeSkillScript>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationRunOptions {
    pub model_config_id: Option<String>,
    pub model_id: Option<String>,
    pub protocol: Option<ModelProtocol>,
    pub tool_search: ToolSearchMode,
    pub tool_profile: ToolProfile,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub context_compression_trigger_ratio: f32,
    pub model_extra: Value,
    #[serde(
        default,
        skip_serializing_if = "harness_contracts::ModelRequestOptions::is_empty"
    )]
    pub model_options: harness_contracts::ModelRequestOptions,
    pub max_iterations: u32,
    pub system_prompt_addendum: Option<String>,
    #[cfg(feature = "agents-subagent")]
    pub agent_tool_policy: Option<harness_contracts::AgentToolPolicy>,
}

impl Default for ConversationRunOptions {
    fn default() -> Self {
        Self {
            model_config_id: None,
            model_id: None,
            protocol: None,
            tool_search: ToolSearchMode::default(),
            tool_profile: ToolProfile::Full,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::NoInteractive,
            context_compression_trigger_ratio: 0.8,
            model_extra: Value::Null,
            model_options: harness_contracts::ModelRequestOptions::default(),
            max_iterations: 0,
            system_prompt_addendum: None,
            #[cfg(feature = "agents-subagent")]
            agent_tool_policy: None,
        }
    }
}

impl ConversationRunOptions {
    #[must_use]
    pub fn from_session_options(options: &SessionOptions) -> Self {
        Self {
            model_config_id: None,
            model_id: options.model_id.clone(),
            protocol: options.protocol,
            tool_search: options.tool_search.clone(),
            tool_profile: options.tool_profile.clone(),
            permission_mode: options.permission_mode,
            interactivity: options.interactivity,
            context_compression_trigger_ratio: options.context_compression_trigger_ratio,
            model_extra: options.model_extra.clone(),
            model_options: options.model_options.clone(),
            max_iterations: options.max_iterations,
            system_prompt_addendum: options.system_prompt_addendum.clone(),
            #[cfg(feature = "agents-subagent")]
            agent_tool_policy: None,
        }
    }

    #[must_use]
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    #[must_use]
    pub fn with_model_config_id(mut self, model_config_id: impl Into<String>) -> Self {
        self.model_config_id = Some(model_config_id.into());
        self
    }

    #[must_use]
    pub fn with_protocol(mut self, protocol: ModelProtocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    #[must_use]
    pub fn with_tool_profile(mut self, tool_profile: ToolProfile) -> Self {
        self.tool_profile = tool_profile;
        self
    }

    #[must_use]
    pub fn with_tool_search(mut self, tool_search: ToolSearchMode) -> Self {
        self.tool_search = tool_search;
        self
    }

    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: PermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self
    }

    #[must_use]
    pub fn with_context_compression_trigger_ratio(mut self, ratio: f32) -> Self {
        self.context_compression_trigger_ratio = ratio;
        self
    }

    #[must_use]
    pub fn with_model_options(
        mut self,
        model_options: harness_contracts::ModelRequestOptions,
    ) -> Self {
        self.model_options = model_options;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationTurnRequest {
    pub options: SessionOptions,
    pub run_options: ConversationRunOptions,
    pub input: ConversationTurnInput,
    pub permission_actor_source: Option<harness_contracts::PermissionActorSource>,
}

impl ConversationTurnRequest {
    #[must_use]
    pub fn from_prompt(
        options: SessionOptions,
        run_options: ConversationRunOptions,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            options,
            run_options,
            input: ConversationTurnInput::ask(prompt),
            permission_actor_source: None,
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
