use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ContextPatchLifecycle, ContextPatchRequest, ContextPatchSinkCap, ContextPatchSource,
    DeferPolicy, SkillFilter, SkillInjectionId, SkillInvocationReceipt, SkillRegistryCap,
    ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct SkillsListTool {
    descriptor: ToolDescriptor,
}

impl Default for SkillsListTool {
    fn default() -> Self {
        Self {
            descriptor: skill_descriptor(
                "skills_list",
                "List skills",
                "List available skills by metadata.",
                DeferPolicy::AlwaysLoad,
                vec![ToolCapability::SkillRegistry],
                super::object_schema(
                    &[],
                    json!({
                        "tag": { "type": "string" },
                        "category": { "type": "string" },
                        "include_prerequisite_missing": { "type": "boolean" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for SkillsListTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        super::generic_action_plan(&self.descriptor, input, ctx, PermissionCheck::Allowed)
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let registry = ctx.capability::<dyn SkillRegistryCap>(ToolCapability::SkillRegistry)?;
        let summaries =
            registry.list_summaries(&ctx.agent_id, skill_filter(authorized.raw_input()));
        Ok(final_structured(to_json(summaries)?))
    }
}

#[derive(Clone)]
pub struct SkillsViewTool {
    descriptor: ToolDescriptor,
}

impl Default for SkillsViewTool {
    fn default() -> Self {
        Self {
            descriptor: skill_descriptor(
                "skills_view",
                "View skill",
                "View one skill with parameters, config keys, and optional full body.",
                DeferPolicy::AutoDefer,
                vec![ToolCapability::SkillRegistry],
                super::object_schema(
                    &["name"],
                    json!({
                        "name": { "type": "string" },
                        "full": { "type": "boolean" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for SkillsViewTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        skill_name(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        super::generic_action_plan(&self.descriptor, input, ctx, PermissionCheck::Allowed)
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let registry = ctx.capability::<dyn SkillRegistryCap>(ToolCapability::SkillRegistry)?;
        let input = authorized.raw_input();
        let name = skill_name(input).map_err(validation_error)?;
        let full = input.get("full").and_then(Value::as_bool).unwrap_or(false);
        let view = registry
            .view(&ctx.agent_id, name, full)
            .ok_or_else(|| ToolError::Validation(format!("skill not visible: {name}")))?;
        Ok(final_structured(to_json(view)?))
    }
}

#[derive(Clone)]
pub struct SkillsInvokeTool {
    descriptor: ToolDescriptor,
}

impl Default for SkillsInvokeTool {
    fn default() -> Self {
        Self {
            descriptor: skill_descriptor(
                "skills_invoke",
                "Invoke skill",
                "Render a skill and return an injection receipt without repeating the body.",
                DeferPolicy::AutoDefer,
                vec![
                    ToolCapability::SkillRegistry,
                    ToolCapability::ContextPatchSink,
                ],
                super::object_schema(
                    &["name"],
                    json!({
                        "name": { "type": "string" },
                        "params": { "type": "object" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for SkillsInvokeTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        skill_name(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        super::generic_action_plan(&self.descriptor, input, ctx, PermissionCheck::Allowed)
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let registry = ctx.capability::<dyn SkillRegistryCap>(ToolCapability::SkillRegistry)?;
        let patch_sink =
            ctx.capability::<dyn ContextPatchSinkCap>(ToolCapability::ContextPatchSink)?;
        let input = authorized.raw_input();
        let name = skill_name(input).map_err(validation_error)?.to_owned();
        let params = input.get("params").cloned().unwrap_or_else(|| json!({}));
        let rendered = registry.render(&ctx.agent_id, name.clone(), params).await?;
        let injection_id = SkillInjectionId(format!("skill:{}:{}", name, ctx.tool_use_id));
        let bytes_injected = rendered.content.len() as u64;
        patch_sink
            .push_patch(ContextPatchRequest {
                tenant_id: ctx.tenant_id,
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                source: ContextPatchSource::SkillInjection {
                    skill_id: rendered.skill_id.clone(),
                    skill_name: rendered.skill_name.clone(),
                    injection_id: injection_id.clone(),
                    tool_use_id: ctx.tool_use_id,
                    consumed_config_keys: rendered.consumed_config_keys.clone(),
                },
                body: rendered.content,
                lifecycle: ContextPatchLifecycle::Transient,
            })
            .await?;
        let receipt = SkillInvocationReceipt {
            skill_name: rendered.skill_name,
            injection_id,
            bytes_injected,
            consumed_config_keys: rendered.consumed_config_keys,
        };
        Ok(final_structured(to_json(receipt)?))
    }
}

fn skill_descriptor(
    name: &str,
    display_name: &str,
    description: &str,
    defer_policy: DeferPolicy,
    required_capabilities: Vec<ToolCapability>,
    input_schema: Value,
) -> ToolDescriptor {
    let mut descriptor = super::descriptor(
        name,
        display_name,
        description,
        ToolGroup::Meta,
        true,
        true,
        false,
        32_000,
        required_capabilities,
        input_schema,
    );
    descriptor.properties.defer_policy = defer_policy;
    descriptor
}

fn skill_filter(input: &Value) -> SkillFilter {
    SkillFilter {
        tag: input
            .get("tag")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned),
        category: input
            .get("category")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned),
        include_prerequisite_missing: input
            .get("include_prerequisite_missing")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn skill_name(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("name is required"))
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn final_structured(value: Value) -> ToolStream {
    Box::pin(stream::iter([ToolEvent::Final(ToolResult::Structured(
        value,
    ))]))
}

fn to_json(value: impl serde::Serialize) -> Result<Value, ToolError> {
    serde_json::to_value(value).map_err(|error| ToolError::Message(error.to_string()))
}
