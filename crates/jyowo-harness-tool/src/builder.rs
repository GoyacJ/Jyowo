use crate::{RegistrationError, Tool, ToolRegistry};

#[derive(Default)]
pub enum BuiltinToolset {
    #[default]
    Default,
    Empty,
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
                    registry.register(Box::<crate::builtin::BashTool>::default())?;
                    registry.register(Box::<crate::builtin::WebFetchTool>::default())?;
                    registry.register(Box::<crate::builtin::WebSearchTool>::default())?;
                    registry.register(Box::<crate::builtin::ClarifyTool>::default())?;
                    registry.register(Box::<crate::builtin::SendMessageTool>::default())?;
                    registry.register(Box::<crate::builtin::TodoTool>::default())?;
                    registry.register(Box::<crate::builtin::TaskStopTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsListTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsViewTool>::default())?;
                    registry.register(Box::<crate::builtin::SkillsInvokeTool>::default())?;
                    #[cfg(feature = "programmatic-tool-calling")]
                    registry.register(Box::<crate::builtin::ExecuteCodeTool>::default())?;
                }
            }
            BuiltinToolset::Empty => {}
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
