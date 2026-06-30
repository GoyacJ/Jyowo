#[cfg(feature = "builtin-toolset")]
mod bash;
#[cfg(feature = "builtin-toolset")]
mod clarify;
#[cfg(feature = "builtin-toolset")]
mod diagnostics;
#[cfg(feature = "builtin-toolset")]
mod edit;
#[cfg(feature = "programmatic-tool-calling")]
mod execute_code;
#[cfg(feature = "builtin-toolset")]
mod glob;
#[cfg(feature = "builtin-toolset")]
mod grep;
#[cfg(feature = "builtin-toolset")]
mod list_dir;
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
pub use clarify::ClarifyTool;
#[cfg(feature = "builtin-toolset")]
pub use diagnostics::{parse_cargo_diagnostics, parse_typescript_diagnostics, DiagnosticsTool};
#[cfg(feature = "builtin-toolset")]
pub use edit::FileEditTool;
#[cfg(feature = "programmatic-tool-calling")]
pub use execute_code::ExecuteCodeTool;
#[cfg(feature = "builtin-toolset")]
pub use glob::GlobTool;
#[cfg(feature = "builtin-toolset")]
pub use grep::GrepTool;
#[cfg(feature = "builtin-toolset")]
pub use list_dir::ListDirTool;
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
pub use web_fetch::{WebFetchBackend, WebFetchRequest, WebFetchResponse, WebFetchTool};
#[cfg(feature = "builtin-toolset")]
pub use web_search::{WebSearchBackend, WebSearchRequest, WebSearchResult, WebSearchTool};
#[cfg(feature = "builtin-toolset")]
pub use write::FileWriteTool;

use harness_contracts::{
    BudgetMetric, DeferPolicy, OverflowAction, ProviderRestriction, ResultBudget, ToolCapability,
    ToolDescriptor, ToolGroup, ToolOrigin, ToolProperties, ToolServiceBinding, TrustLevel,
};
use serde_json::{json, Value};

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
        search_hint: None,
        service_binding,
    }
}

fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "required": required,
        "properties": properties
    })
}
