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
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::BashTool>::default(),
                        ToolJournalAuthority::Sandbox,
                    )?;
                    registry.register(Box::<crate::builtin::WebFetchTool>::default())?;
                    registry.register(Box::<crate::builtin::WebSearchTool>::default())?;
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::ClarifyTool>::default(),
                        ToolJournalAuthority::Clarification,
                    )?;
                    registry.register(Box::<crate::builtin::SendMessageTool>::default())?;
                    registry.register(Box::<crate::builtin::TodoTool>::default())?;
                    registry.register(Box::<crate::builtin::TaskStopTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsListTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsViewTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsInvokeTool>::default())?;
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
                    }
                }
            }
            BuiltinToolset::Clarification => {
                #[cfg(feature = "builtin-toolset")]
                {
                    registry.register_with_journal_authority(
                        Box::<crate::builtin::ClarifyTool>::default(),
                        ToolJournalAuthority::Clarification,
                    )?;
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

fn register_skill_tools(registry: &ToolRegistry) -> Result<(), RegistrationError> {
    #[cfg(any(feature = "builtin-toolset", feature = "skill-tools"))]
    {
        registry.register(Box::<crate::builtin::SkillsListTool>::default())?;
        registry.register(Box::<crate::builtin::SkillsViewTool>::default())?;
        registry.register(Box::<crate::builtin::SkillsInvokeTool>::default())?;
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
