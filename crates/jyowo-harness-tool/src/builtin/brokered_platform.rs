use std::path::PathBuf;

use async_trait::async_trait;
use futures::{future::BoxFuture, stream};
use harness_contracts::{
    AgentId, NetworkAccess, PermissionSubject, RunId, SessionId, TenantId, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolDescriptorMetadata, ToolError, ToolExecutionChannel,
    ToolGroup, ToolIntegrationSource, ToolResult, ToolRiskLevel, ToolUseId, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

pub const BROKERED_PLATFORM_RUNTIME_CAPABILITY: &str = "jyowo.builtin.brokered_platform_runtime";
pub const BROWSER_RUNTIME_CAPABILITY: &str = "jyowo.builtin.browser_runtime";

#[must_use]
pub fn brokered_platform_runtime_capability() -> ToolCapability {
    ToolCapability::Custom(BROKERED_PLATFORM_RUNTIME_CAPABILITY.to_owned())
}

#[must_use]
pub fn browser_runtime_capability() -> ToolCapability {
    ToolCapability::Custom(BROWSER_RUNTIME_CAPABILITY.to_owned())
}

#[derive(Debug, Clone)]
pub struct BrokeredPlatformRuntimeRequest {
    pub tool_name: String,
    pub input: Value,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: AgentId,
    pub tool_use_id: ToolUseId,
    pub workspace_root: PathBuf,
    pub project_workspace_root: Option<PathBuf>,
}

pub trait BrokeredPlatformRuntimeCap: Send + Sync + 'static {
    fn execute(
        &self,
        request: BrokeredPlatformRuntimeRequest,
    ) -> BoxFuture<'static, Result<Value, ToolError>>;
}

#[derive(Clone)]
struct BrokeredPlatformTool {
    descriptor: ToolDescriptor,
    network_access: NetworkAccess,
    runtime_capability: ToolCapability,
}

macro_rules! brokered_platform_tool {
    (
        $name:ident,
        $tool_name:literal,
        $display:literal,
        $description:literal,
        $group:expr,
        $read_only:expr,
        $risk:expr,
        $effect:literal,
        [$($alias:literal),+ $(,)?],
        [$($family:literal),+ $(,)?],
        $network:expr,
        $runtime_capability:expr,
        $schema:expr
    ) => {
        #[derive(Clone)]
        pub struct $name {
            inner: BrokeredPlatformTool,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    inner: BrokeredPlatformTool::new(
                        $tool_name,
                        $display,
                        $description,
                        $group,
                        $read_only,
                        $risk,
                        $effect,
                        &[$($alias),+],
                        &[$($family),+],
                        $network,
                        $runtime_capability,
                        $schema,
                    ),
                }
            }
        }

        #[async_trait]
        impl Tool for $name {
            fn descriptor(&self) -> &ToolDescriptor {
                self.inner.descriptor()
            }

            async fn validate(&self, input: &Value, ctx: &ToolContext) -> Result<(), ValidationError> {
                self.inner.validate(input, ctx).await
            }

            async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
                self.inner.plan(input, ctx).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                self.inner.execute_authorized(authorized, ctx).await
            }
        }
    };
}

brokered_platform_tool!(
    WorktreeTool,
    "Worktree",
    "Worktree",
    "Broker worktree operations such as list, create, switch, and delete through the host runtime.",
    ToolGroup::Worktree,
    false,
    ToolRiskLevel::Medium,
    "mutates_worktree",
    ["worktree", "git worktree"],
    ["worktree"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": { "type": "string" },
            "path": { "type": "string" },
            "branch": { "type": "string" },
            "base": { "type": "string" }
        })
    )
);

brokered_platform_tool!(
    SessionTool,
    "Session",
    "Session",
    "Broker local, worktree, or cloud thread operations through the host runtime.",
    ToolGroup::Session,
    false,
    ToolRiskLevel::Medium,
    "mutates_session",
    ["session", "thread", "codex thread"],
    ["session", "thread"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": { "type": "string" },
            "thread_id": { "type": "string" },
            "message": { "type": "string" },
            "title": { "type": "string" }
        })
    )
);

brokered_platform_tool!(
    ArtifactTool,
    "Artifact",
    "Artifact",
    "Broker artifact create, update, read, and export operations through the host runtime.",
    ToolGroup::Artifact,
    false,
    ToolRiskLevel::Medium,
    "mutates_artifact",
    ["artifact"],
    ["artifact"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": { "type": "string" },
            "artifact_id": { "type": "string" },
            "content": {},
            "format": { "type": "string" }
        })
    )
);

brokered_platform_tool!(
    BrowserUseTool,
    "BrowserUse",
    "Browser Use",
    "Broker interactive browser navigation, inspection, and capture through the host runtime.",
    ToolGroup::Browser,
    false,
    ToolRiskLevel::Medium,
    "external_interaction",
    ["browser", "browser use", "web browser"],
    ["browser"],
    NetworkAccess::Unrestricted,
    browser_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": {
                "type": "string",
                "enum": [
                    "open", "goto", "snapshot", "find", "click", "dblclick", "fill",
                    "type", "press", "hover", "select", "upload", "screenshot", "pdf",
                    "back", "forward", "reload", "tab_list", "tab_new", "tab_close",
                    "tab_select", "mousemove", "mousedown", "mouseup", "mousewheel",
                    "console", "requests", "request", "run_code"
                ]
            },
            "url": { "type": "string" },
            "target": { "type": "string" },
            "selector": { "type": "string" },
            "text": { "type": "string" },
            "key": { "type": "string" },
            "value": { "type": "string" },
            "button": { "type": "string" },
            "path": { "type": "string" },
            "expression": { "type": "string" },
            "index": { "type": "integer", "minimum": 0 },
            "x": { "type": "number" },
            "y": { "type": "number" },
            "delta_x": { "type": "number" },
            "delta_y": { "type": "number" },
            "width": { "type": "integer", "minimum": 1 },
            "height": { "type": "integer", "minimum": 1 }
        })
    )
);

brokered_platform_tool!(
    BrowserDevToolsTool,
    "BrowserDevTools",
    "Browser DevTools",
    "Inspect browser pages, console output, network traffic, screenshots, and performance through the host runtime.",
    ToolGroup::Browser,
    false,
    ToolRiskLevel::Medium,
    "external_interaction",
    ["browser devtools", "chrome devtools", "devtools"],
    ["browser", "devtools"],
    NetworkAccess::Unrestricted,
    browser_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": {
                "type": "string",
                "enum": [
                    "list_pages", "select_page", "new_page", "close_page", "navigate_page",
                    "take_snapshot", "take_screenshot", "evaluate_script",
                    "list_console_messages", "get_console_message", "list_network_requests",
                    "get_network_request", "performance_start_trace", "performance_stop_trace",
                    "performance_analyze_insight", "lighthouse_audit"
                ]
            },
            "args": {
                "type": "object",
                "additionalProperties": true
            }
        })
    )
);

brokered_platform_tool!(
    ComputerUseTool,
    "ComputerUse",
    "Computer Use",
    "Broker desktop computer-use actions through the host runtime.",
    ToolGroup::Computer,
    false,
    ToolRiskLevel::High,
    "external_interaction",
    ["computer", "computer use", "desktop"],
    ["computer"],
    NetworkAccess::Unrestricted,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": { "type": "string" },
            "target": { "type": "string" },
            "text": { "type": "string" },
            "x": { "type": "number" },
            "y": { "type": "number" }
        })
    )
);

brokered_platform_tool!(
    ImageGenerationTool,
    "ImageGeneration",
    "Image Generation",
    "Broker image generation or editing through the host runtime.",
    ToolGroup::Image,
    false,
    ToolRiskLevel::Medium,
    "generates_image",
    ["image generation", "imagegen", "image edit"],
    ["image", "generation"],
    NetworkAccess::Unrestricted,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["prompt"],
        json!({
            "prompt": { "type": "string" },
            "image": { "type": "string" },
            "size": { "type": "string" }
        })
    )
);

brokered_platform_tool!(
    NotebookEditTool,
    "NotebookEdit",
    "Notebook Edit",
    "Broker notebook read and edit operations through the host runtime.",
    ToolGroup::Notebook,
    false,
    ToolRiskLevel::Medium,
    "mutates_notebook",
    ["notebook", "notebook edit"],
    ["notebook"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["path"],
        json!({
            "path": { "type": "string" },
            "cell_id": { "type": "string" },
            "source": { "type": "string" },
            "operation": { "type": "string" }
        })
    )
);

brokered_platform_tool!(
    LspTool,
    "LSP",
    "LSP",
    "Broker language server operations such as diagnostics, symbols, definitions, and references.",
    ToolGroup::Lsp,
    true,
    ToolRiskLevel::Low,
    "reads_code_intelligence",
    ["lsp", "language server"],
    ["lsp", "code_intelligence"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["action"],
        json!({
            "action": { "type": "string" },
            "path": { "type": "string" },
            "symbol": { "type": "string" },
            "line": { "type": "integer" },
            "character": { "type": "integer" }
        })
    )
);

brokered_platform_tool!(
    AutomationTool,
    "Automation",
    "Automation",
    "Broker reminders, monitors, scheduled wakeups, and recurring automations through the host runtime.",
    ToolGroup::Automation,
    false,
    ToolRiskLevel::Medium,
    "mutates_automation",
    ["automation", "cron", "schedule wakeup", "monitor"],
    ["automation", "schedule"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(&["action"], json!({
        "action": { "type": "string" },
        "automation_id": { "type": "string" },
        "schedule": { "type": "string" },
        "prompt": { "type": "string" }
    }))
);

brokered_platform_tool!(
    WorkflowTool,
    "Workflow",
    "Workflow",
    "Broker workflow discovery and execution through the host runtime.",
    ToolGroup::Workflow,
    false,
    ToolRiskLevel::Medium,
    "runs_workflow",
    ["workflow"],
    ["workflow"],
    NetworkAccess::None,
    brokered_platform_runtime_capability(),
    brokered_schema(
        &["name"],
        json!({
            "name": { "type": "string" },
            "params": { "type": "object" }
        })
    )
);

impl BrokeredPlatformTool {
    fn new(
        name: &str,
        display_name: &str,
        description: &str,
        group: ToolGroup,
        is_read_only: bool,
        risk_level: ToolRiskLevel,
        effect: &str,
        aliases: &[&str],
        families: &[&str],
        network_access: NetworkAccess,
        runtime_capability: ToolCapability,
        input_schema: Value,
    ) -> Self {
        let mut descriptor = super::with_output_schema(
            super::descriptor(
                name,
                display_name,
                description,
                group,
                false,
                is_read_only,
                !is_read_only,
                256_000,
                vec![runtime_capability.clone()],
                input_schema,
            ),
            json!({ "type": "object" }),
        );
        descriptor.metadata = ToolDescriptorMetadata {
            aliases: aliases.iter().map(|value| (*value).to_owned()).collect(),
            families: families.iter().map(|value| (*value).to_owned()).collect(),
            platforms: vec!["codex".to_owned(), "claude_code".to_owned()],
            examples: vec![description.to_owned()],
            risk_level,
            effects: vec![effect.to_owned()],
            modalities: vec!["text".to_owned()],
            integration_source: ToolIntegrationSource::Brokered,
        };
        Self {
            descriptor,
            network_access,
            runtime_capability,
        }
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if !input.is_object() {
            return Err(ValidationError::from(
                "brokered platform tool input must be an object",
            ));
        }
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let check = if self.descriptor.properties.is_read_only {
            PermissionCheck::Allowed
        } else {
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: harness_contracts::DecisionScope::ToolName(self.descriptor.name.clone()),
            }
        };
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            check,
            Vec::new(),
            WorkspaceAccess::None,
            self.network_access.clone(),
            ToolExecutionChannel::ExternalCapability {
                capability: self.runtime_capability.clone(),
            },
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let runtime =
            ctx.capability::<dyn BrokeredPlatformRuntimeCap>(self.runtime_capability.clone())?;
        let input = authorized_brokered_input(&authorized, &self.descriptor)?;
        let value = runtime
            .execute(BrokeredPlatformRuntimeRequest {
                tool_name: self.descriptor.name.clone(),
                input,
                tenant_id: ctx.tenant_id,
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                agent_id: ctx.agent_id,
                tool_use_id: ctx.tool_use_id,
                workspace_root: ctx.workspace_root,
                project_workspace_root: ctx.project_workspace_root,
            })
            .await?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(value),
        )])))
    }
}

fn authorized_brokered_input(
    authorized: &AuthorizedToolInput,
    descriptor: &ToolDescriptor,
) -> Result<Value, ToolError> {
    let plan = authorized.action_plan();
    if plan.tool_name != descriptor.name {
        return Err(ToolError::PermissionDenied(
            "authorized plan tool mismatch".to_owned(),
        ));
    }
    let PermissionSubject::ToolInvocation { tool, input } = &plan.subject else {
        return Err(ToolError::PermissionDenied(
            "authorized brokered platform input missing".to_owned(),
        ));
    };
    if tool != &descriptor.name {
        return Err(ToolError::PermissionDenied(
            "authorized brokered platform subject mismatch".to_owned(),
        ));
    }
    Ok(input.clone())
}

fn brokered_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "required": required,
        "properties": properties,
        "additionalProperties": false
    })
}
