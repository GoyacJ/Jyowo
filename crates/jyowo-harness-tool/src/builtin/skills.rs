use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, ContextPatchLifecycle, ContextPatchRequest, ContextPatchSinkCap,
    ContextPatchSource, DecisionScope, DeferPolicy, NetworkAccess, PermissionSubject, SkillFilter,
    SkillInjectionId, SkillInvocationReceipt, SkillRegistryCap, SkillSummary, SkillView,
    ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolResult, WorkspaceAccess,
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
            descriptor: super::with_output_schema(
                skill_descriptor(
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
                json!({
                    "type": "array",
                    "items": serde_json::to_value(schemars::schema_for!(SkillSummary))
                        .unwrap_or_else(|_| json!({"type": "object"}))
                }),
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
        super::generic_action_plan_with_resources(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            vec![ActionResource::Skill {
                action: "list".to_owned(),
                name: None,
            }],
            ToolExecutionChannel::DirectAuthorizedRust,
        )
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
            descriptor: super::with_output_schema(
                skill_descriptor(
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
                serde_json::to_value(schemars::schema_for!(SkillView))
                    .unwrap_or_else(|_| json!({"type": "object"})),
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
        super::generic_action_plan_with_resources(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            vec![ActionResource::Skill {
                action: "view".to_owned(),
                name: input.get("name").and_then(Value::as_str).map(str::to_owned),
            }],
            ToolExecutionChannel::DirectAuthorizedRust,
        )
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

#[derive(Clone)]
pub struct SkillsRunScriptTool {
    descriptor: ToolDescriptor,
}

impl Default for SkillsRunScriptTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_output_schema(
                skill_descriptor(
                    "skills_run_script",
                    "Run skill script",
                    "Run one declared skill script through the process sandbox.",
                    DeferPolicy::AutoDefer,
                    vec![
                        ToolCapability::SkillRegistry,
                        ToolCapability::ProcessSandbox,
                    ],
                    super::object_schema(
                        &["name", "script_id"],
                        json!({
                            "name": { "type": "string" },
                            "script_id": { "type": "string" },
                            "arguments": { "type": "object" }
                        }),
                    ),
                ),
                json!({ "type": "object" }),
            ),
        }
    }
}

#[async_trait]
impl Tool for SkillsRunScriptTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        reject_unknown_script_input_fields(input)?;
        skill_name(input)?;
        script_id(input)?;
        script_arguments(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let registry = ctx.capability::<dyn SkillRegistryCap>(ToolCapability::SkillRegistry)?;
        let name = skill_name(input).map_err(validation_error)?.to_owned();
        let script_id = script_id(input).map_err(validation_error)?.to_owned();
        let arguments = script_arguments(input).map_err(validation_error)?;
        let prepared = registry
            .prepare_script(
                &ctx.agent_id,
                name.clone(),
                script_id.clone(),
                arguments.clone(),
            )
            .await?;
        validate_prepared_identity(&prepared, &name, &script_id, &arguments)?;
        if prepared.declaration.network_access != NetworkAccess::None {
            return Err(ToolError::Validation(
                "skill script network policy is unsupported".to_owned(),
            ));
        }
        let payload = script_permission_payload(&prepared);
        crate::action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::Custom {
                    kind: "skill_script".to_owned(),
                    payload: payload.clone(),
                },
                scope: DecisionScope::ExactArgs(payload),
            },
            vec![ActionResource::Skill {
                action: "run_script".to_owned(),
                name: Some(prepared.skill_name),
            }],
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::ProcessSandbox,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let plan = authorized.action_plan();
        if plan.execution_channel != ToolExecutionChannel::ProcessSandbox
            || plan.workspace_access != WorkspaceAccess::None
            || plan.network_access != NetworkAccess::None
            || !matches!(
                plan.resources.as_slice(),
                [ActionResource::Skill { action, name }]
                    if action == "run_script" && name.is_some()
            )
        {
            return Err(ToolError::PermissionDenied(
                "authorized skill script boundary is invalid".to_owned(),
            ));
        }
        let PermissionSubject::Custom { kind, payload } = &plan.subject else {
            return Err(ToolError::PermissionDenied(
                "authorized skill script permission is missing".to_owned(),
            ));
        };
        if kind != "skill_script" {
            return Err(ToolError::PermissionDenied(
                "authorized skill script permission kind is invalid".to_owned(),
            ));
        }
        let name = required_payload_string(payload, "skill_name")?;
        let script_id = required_payload_string(payload, "script_id")?;
        let arguments = payload
            .get("arguments")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| {
                ToolError::PermissionDenied(
                    "authorized skill script arguments are invalid".to_owned(),
                )
            })?;
        let registry = ctx.capability::<dyn SkillRegistryCap>(ToolCapability::SkillRegistry)?;
        let prepared = registry
            .prepare_script_authorized(
                &ctx.agent_id,
                name.to_owned(),
                script_id.to_owned(),
                arguments.clone(),
            )
            .await?;
        validate_prepared_identity(&prepared, name, script_id, &arguments)?;
        if script_permission_payload(&prepared) != *payload {
            return Err(ToolError::PermissionDenied(
                "skill script package or policy changed after authorization".to_owned(),
            ));
        }
        let sandbox = ctx
            .sandbox
            .clone()
            .ok_or_else(|| ToolError::CapabilityMissing(ToolCapability::ProcessSandbox))?;
        let result = crate::run_prepared_skill_script(prepared, sandbox, &ctx)
            .await
            .map_err(ToolError::Sandbox)?;
        Ok(final_structured(to_json(result)?))
    }
}

impl Default for SkillsInvokeTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_output_schema(
                skill_descriptor(
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
                serde_json::to_value(schemars::schema_for!(SkillInvocationReceipt))
                    .unwrap_or_else(|_| json!({"type": "object"})),
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
        super::generic_action_plan_with_resources(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            vec![ActionResource::Skill {
                action: "invoke".to_owned(),
                name: input.get("name").and_then(Value::as_str).map(str::to_owned),
            }],
            ToolExecutionChannel::DirectAuthorizedRust,
        )
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

fn script_id(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("script_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("script_id is required"))
}

fn script_arguments(input: &Value) -> Result<Value, ValidationError> {
    match input.get("arguments") {
        None => Ok(json!({})),
        Some(arguments) if arguments.is_object() => Ok(arguments.clone()),
        Some(_) => Err(ValidationError::from("arguments must be an object")),
    }
}

fn reject_unknown_script_input_fields(input: &Value) -> Result<(), ValidationError> {
    let Some(fields) = input.as_object() else {
        return Err(ValidationError::from("input must be an object"));
    };
    if let Some(field) = fields
        .keys()
        .find(|field| !matches!(field.as_str(), "name" | "script_id" | "arguments"))
    {
        return Err(ValidationError::from(format!(
            "unknown skills_run_script field: {field}"
        )));
    }
    Ok(())
}

fn validate_prepared_identity(
    prepared: &harness_contracts::SkillScriptRunPreparation,
    name: &str,
    script_id: &str,
    arguments: &Value,
) -> Result<(), ToolError> {
    if prepared.skill_name != name
        || prepared.script_id != script_id
        || prepared.arguments != *arguments
        || prepared.package_hash.trim().is_empty()
    {
        return Err(ToolError::Validation(
            "skill script preparation does not match the requested script".to_owned(),
        ));
    }
    Ok(())
}

fn script_permission_payload(prepared: &harness_contracts::SkillScriptRunPreparation) -> Value {
    json!({
        "skill_id": prepared.skill_id.0,
        "skill_name": prepared.skill_name,
        "script_id": prepared.script_id,
        "package_hash": prepared.package_hash,
        "arguments": prepared.arguments,
        "environment_keys": prepared.declaration.env_config_keys.keys().collect::<Vec<_>>(),
        "environment_config_keys": prepared.declaration.env_config_keys,
        "secret_environment_keys": prepared.declaration.secret_env_keys,
        "workspace_access": "none",
        "network_access": "none",
    })
}

fn required_payload_string<'a>(payload: &'a Value, field: &str) -> Result<&'a str, ToolError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ToolError::PermissionDenied(format!(
                "authorized skill script field `{field}` is missing"
            ))
        })
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
