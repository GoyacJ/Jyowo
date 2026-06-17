use harness_contracts::StopReason;
use harness_tool::ToolCall;

#[derive(Debug, Clone, PartialEq)]
pub enum LoopState {
    AwaitingModel,
    ProcessingToolUses { pending: Vec<ToolCall> },
    ApplyingHookResults,
    MergingContext,
    Ended(StopReason),
}
