use harness_contracts::{Event, Message, SessionId, TeamId, TenantId, ToolDescriptor};
use harness_model::{CacheBreakpoint, PromptCacheStyle};

#[derive(Debug, Clone, PartialEq)]
pub struct AssembledPrompt {
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub tools_snapshot: Vec<ToolDescriptor>,
    pub cache_breakpoints: Vec<CacheBreakpoint>,
    pub tokens_estimate: u64,
    pub budget_utilization: f32,
    pub events: Vec<Event>,
}

pub trait ContextSessionView: Send + Sync {
    fn tenant_id(&self) -> TenantId {
        TenantId::SINGLE
    }

    fn session_id(&self) -> Option<SessionId> {
        None
    }

    fn user_id(&self) -> Option<String> {
        None
    }

    fn team_id(&self) -> Option<TeamId> {
        None
    }

    fn system(&self) -> Option<String>;
    fn messages(&self) -> Vec<Message>;
    fn tools_snapshot(&self) -> Vec<ToolDescriptor>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCachePolicy {
    pub style: PromptCacheStyle,
    pub max_breakpoints: usize,
    pub breakpoint_strategy: BreakpointStrategy,
}

impl Default for PromptCachePolicy {
    fn default() -> Self {
        Self {
            style: PromptCacheStyle::None,
            max_breakpoints: 0,
            breakpoint_strategy: BreakpointStrategy::SystemOnly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointStrategy {
    SystemAnd3,
    SystemOnly,
    EveryN(usize),
}
