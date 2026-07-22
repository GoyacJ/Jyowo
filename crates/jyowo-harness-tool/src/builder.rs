use crate::{RegistrationError, Tool, ToolRegistry};

#[cfg(feature = "builtin-toolset")]
use crate::ToolJournalAuthority;

#[derive(Default)]
pub enum BuiltinToolset {
    #[default]
    Default,
    Clarification,
    Empty,
    Shell,
    Skills,
    Custom(Vec<Box<dyn Tool>>),
}

#[derive(Default)]
pub struct ToolRegistryBuilder {
    builtin_toolset: BuiltinToolset,
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_builtin_toolset(mut self, builtin_toolset: BuiltinToolset) -> Self {
        self.builtin_toolset = builtin_toolset;
        self
    }

    #[must_use]
    pub fn with_tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn build(self) -> Result<ToolRegistry, RegistrationError> {
        let registry = ToolRegistry::empty();

        match self.builtin_toolset {
            BuiltinToolset::Default => {
                #[cfg(feature = "builtin-toolset")]
                {
                    registry.register(Box::<crate::builtin::FileReadTool>::default())?;
                    registry.register(Box::<crate::builtin::FileEditTool>::default())?;
                    registry.register(Box::<crate::builtin::FileWriteTool>::default())?;
                    registry.register(Box::<crate::builtin::ListDirTool>::default())?;
                    registry.register(Box::<crate::builtin::GrepTool>::default())?;
                    registry.register(Box::<crate::builtin::GlobTool>::default())?;
                    registry.register(Box::<crate::builtin::ReadBlobTool>::default())?;
                    registry.register(Box::<crate::builtin::GitStatusTool>::default())?;
                    registry.register(Box::<crate::builtin::GitDiffTool>::default())?;
                    registry.register(Box::<crate::builtin::GitShowTool>::default())?;
                    registry.register(Box::<crate::builtin::GitLogTool>::default())?;
                    registry.register(Box::<crate::builtin::GitStageTool>::default())?;
                    registry.register(Box::<crate::builtin::GitCommitTool>::default())?;
                    registry.register(Box::<crate::builtin::GitBranchTool>::default())?;
                    registry.register(Box::<crate::builtin::GitPullTool>::default())?;
                    registry.register(Box::<crate::builtin::GitPushTool>::default())?;
                    registry.register(Box::<crate::builtin::WorktreeTool>::default())?;
                    registry.register(Box::<crate::builtin::SessionTool>::default())?;
                    registry.register(Box::<crate::builtin::ArtifactTool>::default())?;
                    registry.register(Box::<crate::builtin::BrowserUseTool>::default())?;
                    registry.register(Box::<crate::builtin::BrowserDevToolsTool>::default())?;
                    registry.register(Box::<crate::builtin::ComputerUseTool>::default())?;
                    registry.register(Box::<crate::builtin::ImageGenerationTool>::default())?;
                    registry.register(Box::<crate::builtin::NotebookEditTool>::default())?;
                    registry.register(Box::<crate::builtin::LspTool>::default())?;
                    registry.register(Box::<crate::builtin::AutomationTool>::default())?;
                    registry.register(Box::<crate::builtin::WorkflowTool>::default())?;
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::BashTool>::default(),
                        ToolJournalAuthority::Sandbox,
                    )?;
                    registry.register(Box::<crate::builtin::WebFetchTool>::default())?;
                    registry.register(Box::<crate::builtin::WebSearchTool>::default())?;
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::DiagnosticsTool>::default(),
                        ToolJournalAuthority::Sandbox,
                    )?;
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::ProcessStartTool>::default(),
                        ToolJournalAuthority::Sandbox,
                    )?;
                    registry.register(Box::<crate::builtin::ProcessReadTool>::default())?;
                    registry.register(Box::<crate::builtin::ProcessStopTool>::default())?;
                    registry.register(Box::<crate::builtin::AskUserQuestionTool>::default())?;
                    registry.register(Box::<crate::builtin::SendMessageTool>::default())?;
                    registry.register(Box::<crate::builtin::TodoTool>::default())?;
                    registry.register(Box::<crate::builtin::MemoryTool>::default())?;
                    registry.register(Box::<crate::builtin::TaskStopTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsListTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsViewTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsInvokeTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsRunScriptTool>::default())?;
                    #[cfg(feature = "programmatic-tool-calling")]
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::ExecuteCodeTool>::default(),
                        ToolJournalAuthority::ExecuteCode,
                    )?;
                    #[cfg(feature = "minimax-tools")]
                    {
                        registry
                            .register(Box::<crate::builtin::MiniMaxTextToImageTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxImageToImageTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxTextToVideoTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxImageToVideoTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxFirstLastFrameToVideoTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxSubjectReferenceVideoTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxVideoGenerationQueryTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxVideoTemplateTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxVideoTemplateQueryTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxTextToSpeechTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxTextToSpeechWsTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxTextToSpeechAsyncTool>::default(),
                        )?;
                        registry.register(Box::<
                            crate::builtin::MiniMaxTextToSpeechAsyncQueryTool,
                        >::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxVoiceCloneTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxVoiceDesignTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxListVoicesTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxDeleteVoiceTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxLyricsGenerationTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxMusicGenerationTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxMusicCoverPreprocessTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxFileUploadTool>::default())?;
                        registry.register(Box::<crate::builtin::MiniMaxFileListTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxFileRetrieveTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxFileRetrieveContentTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxFileDeleteTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxModelsListTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxModelRetrieveTool>::default())?;
                        registry.register(Box::<crate::builtin::MiniMaxResponsesTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxResponsesInputTokensTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxTextChatCompletionTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxAnthropicMessagesTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxAnthropicCountTokensTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::MiniMaxAnthropicModelsListTool>::default(),
                        )?;
                        registry.register(Box::<
                            crate::builtin::MiniMaxAnthropicModelRetrieveTool,
                        >::default())?;
                        registry
                            .register(Box::<crate::builtin::MiniMaxVideoDownloadTool>::default())?;
                    }
                    #[cfg(feature = "gemini-tools")]
                    {
                        registry.register(Box::<crate::builtin::GeminiModelsListTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiModelGetTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::GeminiTokensCountTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiFileUploadTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiFileListTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiFileGetTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiFileDeleteTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::GeminiCachedContentCreateTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::GeminiCachedContentGetTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::GeminiCachedContentListTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::GeminiCachedContentDeleteTool>::default(),
                        )?;
                        registry.register(Box::<crate::builtin::GeminiEmbeddingTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::GeminiEmbeddingBatchTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::GeminiBatchCreateTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiBatchGetTool>::default())?;
                        registry.register(Box::<crate::builtin::GeminiBatchListTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::GeminiBatchCancelTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::GeminiImageGenerationTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::GeminiVideoGenerationTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::GeminiVideoGenerationQueryTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::GeminiTextToSpeechTool>::default())?;
                    }
                    #[cfg(feature = "seedance-tools")]
                    {
                        registry.register(Box::<crate::builtin::SeedanceTextToVideo>::default())?;
                        registry.register(Box::<crate::builtin::SeedanceImageToVideo>::default())?;
                        registry.register(
                            Box::<crate::builtin::SeedanceVideoGenerationQueryTool>::default(),
                        )?;
                    }
                    #[cfg(feature = "zhipu-tools")]
                    {
                        registry
                            .register(Box::<crate::builtin::ZhipuImageGenerationTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::ZhipuImageGenerationAsyncTool>::default(),
                        )?;
                        registry.register(
                            Box::<crate::builtin::ZhipuImageGenerationQueryTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::ZhipuVideoGenerationTool>::default())?;
                        registry.register(
                            Box::<crate::builtin::ZhipuVideoGenerationQueryTool>::default(),
                        )?;
                        registry
                            .register(Box::<crate::builtin::ZhipuTextToSpeechTool>::default())?;
                        registry
                            .register(Box::<crate::builtin::ZhipuSpeechToTextTool>::default())?;
                    }
                }
            }
            BuiltinToolset::Clarification => {
                #[cfg(feature = "builtin-toolset")]
                {
                    registry.register(Box::<crate::builtin::AskUserQuestionTool>::default())?;
                }
                #[cfg(not(feature = "builtin-toolset"))]
                {
                    return Err(RegistrationError::InvalidDescriptor(
                        "clarification tools feature is not enabled".to_owned(),
                    ));
                }
            }
            BuiltinToolset::Empty => {}
            BuiltinToolset::Shell => {
                #[cfg(feature = "builtin-toolset")]
                {
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::BashTool>::default(),
                        ToolJournalAuthority::Sandbox,
                    )?;
                }
                #[cfg(not(feature = "builtin-toolset"))]
                {
                    return Err(RegistrationError::InvalidDescriptor(
                        "shell tools feature is not enabled".to_owned(),
                    ));
                }
            }
            BuiltinToolset::Skills => {
                register_skill_tools(&registry)?;
            }
            BuiltinToolset::Custom(tools) => {
                for tool in tools {
                    registry.register(tool)?;
                }
            }
        }

        for tool in self.tools {
            registry.register(tool)?;
        }

        Ok(registry)
    }
}

pub use crate::registry::{
    provider_service_adapter_availability_from_snapshot, tool_service_bindings_from_snapshot,
};

fn register_skill_tools(registry: &ToolRegistry) -> Result<(), RegistrationError> {
    #[cfg(any(feature = "builtin-toolset", feature = "skill-tools"))]
    {
        registry.register(Box::<crate::builtin::SkillsListTool>::default())?;
        registry.register(Box::<crate::builtin::SkillsViewTool>::default())?;
        registry.register(Box::<crate::builtin::SkillsInvokeTool>::default())?;
        registry.register(Box::<crate::builtin::SkillsRunScriptTool>::default())?;
        Ok(())
    }
    #[cfg(not(any(feature = "builtin-toolset", feature = "skill-tools")))]
    {
        let _ = registry;
        Err(RegistrationError::InvalidDescriptor(
            "skill tools feature is not enabled".to_owned(),
        ))
    }
}
