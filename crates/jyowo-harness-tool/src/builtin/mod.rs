#[cfg(feature = "builtin-toolset")]
mod bash;
#[cfg(feature = "builtin-toolset")]
mod brokered_platform;
#[cfg(feature = "builtin-toolset")]
mod clarify;
#[cfg(feature = "builtin-toolset")]
mod diagnostics;
#[cfg(feature = "builtin-toolset")]
mod edit;
#[cfg(feature = "programmatic-tool-calling")]
mod execute_code;
#[cfg(feature = "builtin-toolset")]
mod git;
#[cfg(feature = "builtin-toolset")]
mod glob;
#[cfg(feature = "builtin-toolset")]
mod grep;
#[cfg(feature = "builtin-toolset")]
mod list_dir;
#[cfg(feature = "builtin-toolset")]
mod memory;
#[cfg(feature = "minimax-tools")]
mod minimax;
#[cfg(feature = "builtin-toolset")]
mod process_monitor;
#[cfg(feature = "builtin-toolset")]
mod read;
#[cfg(feature = "builtin-toolset")]
mod read_blob;
#[cfg(feature = "seedance-tools")]
mod seedance;
#[cfg(feature = "builtin-toolset")]
mod send_message;
mod skills;
#[cfg(feature = "builtin-toolset")]
mod task_stop;
#[cfg(feature = "builtin-toolset")]
mod todo;
#[cfg(feature = "builtin-toolset")]
mod web_fetch;
#[cfg(feature = "builtin-toolset")]
mod web_search;
#[cfg(feature = "builtin-toolset")]
mod workspace_path;
#[cfg(feature = "builtin-toolset")]
mod write;

#[cfg(feature = "builtin-toolset")]
pub use bash::BashTool;
#[cfg(feature = "builtin-toolset")]
pub use brokered_platform::{
    brokered_platform_runtime_capability, ArtifactTool, AutomationTool, BrokeredPlatformRuntimeCap,
    BrokeredPlatformRuntimeRequest, BrowserUseTool, ComputerUseTool, ImageGenerationTool, LspTool,
    NotebookEditTool, SessionTool, WorkflowTool, WorktreeTool,
};
#[cfg(feature = "builtin-toolset")]
pub use clarify::ClarifyTool;
#[cfg(feature = "builtin-toolset")]
pub use diagnostics::{parse_cargo_diagnostics, parse_typescript_diagnostics, DiagnosticsTool};
#[cfg(feature = "builtin-toolset")]
pub use edit::FileEditTool;
#[cfg(feature = "programmatic-tool-calling")]
pub use execute_code::ExecuteCodeTool;
#[cfg(feature = "builtin-toolset")]
pub use git::{
    GitBranchTool, GitCommitTool, GitDiffTool, GitLogTool, GitPullTool, GitPushTool, GitShowTool,
    GitStageTool, GitStatusTool,
};
#[cfg(feature = "builtin-toolset")]
pub use glob::GlobTool;
#[cfg(feature = "builtin-toolset")]
pub use grep::GrepTool;
#[cfg(feature = "builtin-toolset")]
pub use list_dir::ListDirTool;
#[cfg(feature = "builtin-toolset")]
pub use memory::{
    memory_tool_runtime_capability, MemoryTool, MemoryToolDraft, MemoryToolRuntimeAction,
    MemoryToolRuntimeCap, MemoryToolRuntimeRequest, MemoryToolVisibility,
    MEMORY_TOOL_RUNTIME_CAPABILITY,
};
#[cfg(feature = "minimax-tools")]
pub use minimax::{
    MiniMaxAnthropicCountTokensTool, MiniMaxAnthropicMessagesTool,
    MiniMaxAnthropicModelRetrieveTool, MiniMaxAnthropicModelsListTool, MiniMaxDeleteVoiceTool,
    MiniMaxFileDeleteTool, MiniMaxFileListTool, MiniMaxFileRetrieveTool, MiniMaxFileUploadTool,
    MiniMaxFirstLastFrameToVideoTool, MiniMaxImageToImageTool, MiniMaxImageToVideoTool,
    MiniMaxListVoicesTool, MiniMaxLyricsGenerationTool, MiniMaxModelRetrieveTool,
    MiniMaxModelsListTool, MiniMaxMusicCoverPreprocessTool, MiniMaxMusicGenerationTool,
    MiniMaxResponsesInputTokensTool, MiniMaxResponsesTool, MiniMaxSubjectReferenceVideoTool,
    MiniMaxTextToImageTool, MiniMaxTextToSpeechAsyncQueryTool, MiniMaxTextToSpeechAsyncTool,
    MiniMaxTextToSpeechTool, MiniMaxTextToVideoTool, MiniMaxVideoGenerationQueryTool,
    MiniMaxVideoTemplateQueryTool, MiniMaxVideoTemplateTool, MiniMaxVoiceCloneTool,
    MiniMaxVoiceDesignTool,
};
#[cfg(feature = "builtin-toolset")]
pub use process_monitor::{ProcessReadTool, ProcessStartTool, ProcessStopTool};
#[cfg(feature = "builtin-toolset")]
pub use read::FileReadTool;
#[cfg(feature = "builtin-toolset")]
pub use read_blob::ReadBlobTool;
#[cfg(feature = "seedance-tools")]
pub use seedance::{SeedanceImageToVideo, SeedanceTextToVideo, SeedanceVideoGenerationQueryTool};
#[cfg(feature = "builtin-toolset")]
pub use send_message::SendMessageTool;
pub use skills::{SkillsInvokeTool, SkillsListTool, SkillsViewTool};
#[cfg(feature = "builtin-toolset")]
pub use task_stop::TaskStopTool;
#[cfg(feature = "builtin-toolset")]
pub use todo::TodoTool;
#[cfg(feature = "builtin-toolset")]
pub use web_fetch::WebFetchTool;
#[cfg(feature = "builtin-toolset")]
pub use web_search::{
    WebSearchBackend, WebSearchRequest, WebSearchResult, WebSearchTool,
    WEB_SEARCH_BACKEND_CAPABILITY,
};
#[cfg(feature = "builtin-toolset")]
pub use write::FileWriteTool;

use harness_contracts::{
    ActionResource, BudgetMetric, DeferPolicy, LongRunningPolicy, NetworkAccess, OverflowAction,
    ProviderRestriction, ResultBudget, ToolActionPlan, ToolCapability, ToolDescriptor, ToolError,
    ToolExecutionChannel, ToolGroup, ToolOrigin, ToolProperties, ToolServiceBinding, TrustLevel,
    WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use std::time::Duration;

use crate::{action_plan_from_permission_check, ToolContext};

fn descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    group: ToolGroup,
    is_concurrency_safe: bool,
    is_read_only: bool,
    is_destructive: bool,
    budget_limit: u64,
    required_capabilities: Vec<ToolCapability>,
    input_schema: Value,
) -> ToolDescriptor {
    descriptor_with_binding(
        name,
        display_name,
        description,
        group,
        is_concurrency_safe,
        is_read_only,
        is_destructive,
        budget_limit,
        required_capabilities,
        input_schema,
        None,
    )
}

pub(super) fn descriptor_with_binding(
    name: &str,
    display_name: &str,
    description: &str,
    group: ToolGroup,
    is_concurrency_safe: bool,
    is_read_only: bool,
    is_destructive: bool,
    budget_limit: u64,
    required_capabilities: Vec<ToolCapability>,
    input_schema: Value,
    service_binding: Option<ToolServiceBinding>,
) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: display_name.to_owned(),
        description: description.to_owned(),
        category: "builtin".to_owned(),
        group,
        version: "0.1.0".to_owned(),
        input_schema,
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe,
            is_read_only,
            is_destructive,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities,
        budget: ResultBudget {
            metric: BudgetMetric::Chars,
            limit: budget_limit,
            on_overflow: OverflowAction::Offload,
            preview_head_chars: 2_000,
            preview_tail_chars: 2_000,
        },
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: Some(format!("{display_name} {description}")),
        service_binding,
        metadata: Default::default(),
    }
}

pub(super) fn result_budget(
    metric: BudgetMetric,
    limit: u64,
    on_overflow: OverflowAction,
    preview_head_chars: u32,
    preview_tail_chars: u32,
) -> ResultBudget {
    ResultBudget {
        metric,
        limit,
        on_overflow,
        preview_head_chars,
        preview_tail_chars,
    }
}

pub(super) fn with_result_budget(
    mut descriptor: ToolDescriptor,
    budget: ResultBudget,
) -> ToolDescriptor {
    descriptor.budget = budget;
    descriptor
}

pub(super) fn long_running_policy(
    stall_threshold: Duration,
    hard_timeout: Duration,
) -> LongRunningPolicy {
    LongRunningPolicy {
        stall_threshold,
        hard_timeout,
    }
}

pub(super) fn with_long_running(
    mut descriptor: ToolDescriptor,
    policy: LongRunningPolicy,
) -> ToolDescriptor {
    descriptor.properties.long_running = Some(policy);
    descriptor
}

fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "required": required,
        "properties": properties,
        "additionalProperties": false
    })
}

fn with_output_schema(mut descriptor: ToolDescriptor, output_schema: Value) -> ToolDescriptor {
    descriptor.output_schema = Some(output_schema);
    descriptor
}

#[cfg_attr(not(feature = "builtin-toolset"), allow(dead_code))]
fn text_output_schema() -> Value {
    json!({ "type": "string" })
}

#[cfg_attr(not(feature = "programmatic-tool-calling"), allow(dead_code))]
fn generic_action_plan(
    descriptor: &ToolDescriptor,
    input: &Value,
    ctx: &ToolContext,
    check: PermissionCheck,
    channel: ToolExecutionChannel,
) -> Result<ToolActionPlan, ToolError> {
    generic_action_plan_with_resources(descriptor, input, ctx, check, Vec::new(), channel)
}

fn generic_action_plan_with_resources(
    descriptor: &ToolDescriptor,
    input: &Value,
    ctx: &ToolContext,
    check: PermissionCheck,
    resources: Vec<ActionResource>,
    channel: ToolExecutionChannel,
) -> Result<ToolActionPlan, ToolError> {
    action_plan_from_permission_check(
        descriptor,
        input,
        ctx,
        check,
        resources,
        WorkspaceAccess::None,
        NetworkAccess::None,
        channel,
    )
}
