use harness_contracts::{MessageRole, ToolUseId};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookMessageView {
    pub role: MessageRole,
    pub text_snippet: String,
    pub tool_use_id: Option<ToolUseId>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolErrorView {
    pub message: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ModelRequestView {
    pub provider_id: String,
    pub model_id: String,
    pub message_count: u32,
    pub tool_count: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SubagentSpecView {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolDescriptorView {
    pub name: String,
    pub display_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NotificationKind {
    Info,
    Warning,
    Error,
    Custom(String),
}
