use harness_contracts::{Decision, InconsistentReason};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum HookOutcome {
    Continue,
    Block { reason: String },
    PreToolUse(PreToolUseOutcome),
    RewriteInput(Value),
    OverridePermission(Decision),
    AddContext(ContextPatch),
    Transform(Value),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreToolUseOutcome {
    pub rewrite_input: Option<Value>,
    pub override_permission: Option<Decision>,
    pub additional_context: Option<ContextPatch>,
    pub block: Option<String>,
}

impl PreToolUseOutcome {
    pub fn validate(&self) -> Result<(), InconsistentReason> {
        if self.block.is_some()
            && (self.rewrite_input.is_some()
                || self.override_permission.is_some()
                || self.additional_context.is_some())
        {
            return Err(InconsistentReason::PreToolUseBlockExclusive);
        }

        Ok(())
    }

    pub fn is_continue(&self) -> bool {
        self.rewrite_input.is_none()
            && self.override_permission.is_none()
            && self.additional_context.is_none()
            && self.block.is_none()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContextPatch {
    pub role: ContextPatchRole,
    pub content: String,
    pub apply_to_next_turn_only: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ContextPatchRole {
    SystemAppend,
    UserPrefix,
    UserSuffix,
    AssistantHint,
}
