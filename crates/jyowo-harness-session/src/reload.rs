use harness_contracts::{DeferPolicy, ForkReason, SessionError, SessionId, TenantId, ToolName};
use harness_skill::SkillRegistration;

use crate::Session;

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigDelta {
    pub tenant_id: TenantId,
    pub add_tools: Vec<AddedTool>,
    pub remove_tools: Vec<ToolName>,
    pub add_skills: Vec<SkillRegistration>,
    pub add_mcp_servers: Vec<AddedMcpServer>,
    pub remove_mcp_servers: Vec<String>,
    pub update_memory: bool,
    pub permission_rule_patch: bool,
    pub system_prompt_addendum: Option<String>,
    pub model_ref: Option<String>,
    pub tool_search_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedTool {
    pub name: ToolName,
    pub defer_policy: DeferPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedMcpServer {
    pub id: String,
    pub tools: Vec<AddedTool>,
}

pub struct ReloadOutcome {
    pub mode: ReloadMode,
    pub new_session: Option<Session>,
    pub effective_from: ReloadEffect,
    pub cache_impact: CacheImpact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadMode {
    AppliedInPlace,
    ForkedNewSession { parent: SessionId, child: SessionId },
    Rejected { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadEffect {
    NextTurn,
    NextMessage,
    Immediate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheImpact {
    NoInvalidation,
    OneShotInvalidation {
        reason: CacheInvalidationReason,
        affected_breakpoints: Vec<harness_contracts::BreakpointId>,
    },
    FullReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInvalidationReason {
    ToolsetAppended,
    SkillsAppended,
    McpServerAdded,
    MemdirContentChanged,
    SystemPromptChanged,
    ToolRemoved,
    ModelSwitched,
}

impl ConfigDelta {
    pub fn for_tenant(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            add_tools: Vec::new(),
            remove_tools: Vec::new(),
            add_skills: Vec::new(),
            add_mcp_servers: Vec::new(),
            remove_mcp_servers: Vec::new(),
            update_memory: false,
            permission_rule_patch: false,
            system_prompt_addendum: None,
            model_ref: None,
            tool_search_mode: None,
        }
    }

    #[must_use]
    pub fn with_permission_rule_patch(mut self) -> Self {
        self.permission_rule_patch = true;
        self
    }

    #[must_use]
    pub fn reload_memdir(mut self) -> Self {
        self.update_memory = true;
        self
    }

    #[must_use]
    pub fn add_tool(mut self, name: impl Into<ToolName>, defer_policy: DeferPolicy) -> Self {
        self.add_tools.push(AddedTool {
            name: name.into(),
            defer_policy,
        });
        self
    }

    #[must_use]
    pub fn remove_tool(mut self, name: impl Into<ToolName>) -> Self {
        self.remove_tools.push(name.into());
        self
    }

    #[must_use]
    pub fn add_skill(mut self, registration: SkillRegistration) -> Self {
        self.add_skills.push(registration);
        self
    }

    #[must_use]
    pub fn add_mcp_server(mut self, server: AddedMcpServer) -> Self {
        self.add_mcp_servers.push(server);
        self
    }

    #[must_use]
    pub fn with_model_ref(mut self, model_ref: impl Into<String>) -> Self {
        self.model_ref = Some(model_ref.into());
        self
    }

    #[must_use]
    pub fn with_tool_search_mode(mut self, mode: impl Into<String>) -> Self {
        self.tool_search_mode = Some(mode.into());
        self
    }
}

impl Session {
    pub async fn reload_with(&self, delta: ConfigDelta) -> Result<ReloadOutcome, SessionError> {
        if delta.tenant_id != self.tenant_id() {
            return Ok(rejected("cross-tenant reload is not allowed"));
        }
        if delta.tool_search_mode.is_some() {
            return Ok(rejected("tool_search mode is creation/fork-time only"));
        }

        if !delta.add_skills.is_empty() {
            if let Some(cap) = self.skill_reload_cap() {
                if let Err(reason) = cap.reload_skills(&delta.add_skills).await {
                    return Ok(rejected(reason));
                }
            } else {
                return Ok(rejected("skill reload capability is not installed"));
            }
        }

        let projection = self.replay_projection().await?;
        if projection.tool_uses.values().any(|record| {
            delta.remove_tools.contains(&record.tool_name)
                && record.result.is_none()
                && record.error.is_none()
        }) {
            return Ok(rejected(
                "cannot remove a tool referenced by a running tool use",
            ));
        }

        if is_destructive(&delta) {
            let child = self.fork(ForkReason::HotReload).await?;
            let child_id = child.projection().await.session_id;
            return Ok(ReloadOutcome {
                mode: ReloadMode::ForkedNewSession {
                    parent: self.session_id(),
                    child: child_id,
                },
                new_session: Some(child),
                effective_from: ReloadEffect::NextTurn,
                cache_impact: CacheImpact::FullReset,
            });
        }

        Ok(ReloadOutcome {
            mode: ReloadMode::AppliedInPlace,
            new_session: None,
            effective_from: if delta.permission_rule_patch && !has_additive_changes(&delta) {
                ReloadEffect::Immediate
            } else {
                ReloadEffect::NextTurn
            },
            cache_impact: additive_cache_impact(&delta),
        })
    }
}

fn rejected(reason: impl Into<String>) -> ReloadOutcome {
    ReloadOutcome {
        mode: ReloadMode::Rejected {
            reason: reason.into(),
        },
        new_session: None,
        effective_from: ReloadEffect::Immediate,
        cache_impact: CacheImpact::NoInvalidation,
    }
}

fn is_destructive(delta: &ConfigDelta) -> bool {
    !delta.remove_tools.is_empty()
        || !delta.remove_mcp_servers.is_empty()
        || delta.update_memory
        || delta.model_ref.is_some()
        || delta
            .system_prompt_addendum
            .as_ref()
            .is_some_and(|value| !value.is_empty())
}

fn has_additive_changes(delta: &ConfigDelta) -> bool {
    !delta.add_tools.is_empty() || !delta.add_skills.is_empty() || !delta.add_mcp_servers.is_empty()
}

fn additive_cache_impact(delta: &ConfigDelta) -> CacheImpact {
    if delta.add_tools.iter().any(is_always_loaded) {
        return one_shot(CacheInvalidationReason::ToolsetAppended);
    }
    if !delta.add_skills.is_empty() {
        return one_shot(CacheInvalidationReason::SkillsAppended);
    }
    if delta
        .add_mcp_servers
        .iter()
        .flat_map(|server| &server.tools)
        .any(is_always_loaded)
    {
        return one_shot(CacheInvalidationReason::McpServerAdded);
    }
    CacheImpact::NoInvalidation
}

fn one_shot(reason: CacheInvalidationReason) -> CacheImpact {
    CacheImpact::OneShotInvalidation {
        reason,
        affected_breakpoints: Vec::new(),
    }
}

fn is_always_loaded(tool: &AddedTool) -> bool {
    matches!(tool.defer_policy, DeferPolicy::AlwaysLoad)
}
