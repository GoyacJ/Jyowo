use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, CodeLanguage, CodeRunRequest, DecisionScope, PermissionSubject, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use std::time::Duration;

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct ExecuteCodeTool {
    descriptor: ToolDescriptor,
}

impl Default for ExecuteCodeTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_long_running(
                super::with_output_schema(
                    super::descriptor(
                        "execute_code",
                        "Execute code",
                        "Run deterministic MiniLua code through the configured code runtime.",
                        ToolGroup::Shell,
                        false,
                        false,
                        true,
                        256_000,
                        vec![
                            ToolCapability::CodeRuntime,
                            ToolCapability::EmbeddedToolDispatcher,
                        ],
                        super::object_schema(
                            &["source"],
                            json!({
                                "language": { "type": "string", "enum": ["mini_lua"] },
                                "source": { "type": "string" }
                            }),
                        ),
                    ),
                    json!({
                        "type": "object",
                        "required": ["value", "stats", "embedded_steps"],
                        "properties": {
                            "value": true,
                            "stats": {
                                "type": "object",
                                "required": ["instructions", "embedded_call_count"],
                                "properties": {
                                    "instructions": { "type": "integer", "minimum": 0 },
                                    "embedded_call_count": { "type": "integer", "minimum": 0 }
                                },
                                "additionalProperties": false
                            },
                            "embedded_steps": { "type": "array" }
                        },
                        "additionalProperties": false
                    }),
                ),
                super::long_running_policy(Duration::from_secs(2), Duration::from_secs(60)),
            ),
        }
    }
}

#[async_trait]
impl Tool for ExecuteCodeTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        source(input)?;
        language(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        if ctx.subagent_depth > 0 {
            return super::generic_action_plan(
                &self.descriptor,
                input,
                ctx,
                PermissionCheck::Denied {
                    reason: "execute_code is not available from subagents".to_owned(),
                },
                ToolExecutionChannel::ProcessSandbox,
            );
        }
        let script_hash = blake3::hash(source(input).unwrap_or_default().as_bytes());
        super::generic_action_plan_with_resources(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ExecuteCodeScript {
                    script_hash: *script_hash.as_bytes(),
                },
            },
            vec![ActionResource::CodeExecution {
                language: match language(input).unwrap_or(CodeLanguage::MiniLua) {
                    CodeLanguage::MiniLua => "mini_lua".to_owned(),
                },
                script_hash: script_hash.to_hex().to_string(),
            }],
            ToolExecutionChannel::ProcessSandbox,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input();
        ensure_authorized_script_hash(&authorized, input)?;
        let runtime =
            ctx.capability::<dyn harness_contracts::CodeRuntimeCap>(ToolCapability::CodeRuntime)?;
        let dispatcher = ctx.capability::<dyn harness_contracts::EmbeddedToolDispatcherCap>(
            ToolCapability::EmbeddedToolDispatcher,
        )?;
        let result = match runtime
            .run_code(
                CodeRunRequest {
                    tenant_id: ctx.tenant_id,
                    session_id: ctx.session_id,
                    run_id: ctx.run_id,
                    tool_use_id: ctx.tool_use_id,
                    language: language(input).map_err(validation_error)?,
                    source: source(input).map_err(validation_error)?.to_owned(),
                },
                dispatcher,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let mut events = error
                    .events
                    .into_iter()
                    .map(ToolEvent::Journal)
                    .collect::<Vec<_>>();
                events.push(ToolEvent::Error(error.error));
                return Ok(Box::pin(stream::iter(events)));
            }
        };

        let mut events = result
            .events
            .into_iter()
            .map(ToolEvent::Journal)
            .collect::<Vec<_>>();
        events.push(ToolEvent::Final(ToolResult::Structured(json!({
            "value": result.value,
            "stats": {
                "instructions": result.stats.instructions,
                "embedded_call_count": result.stats.embedded_call_count
            },
            "embedded_steps": result.embedded_steps
        }))));
        Ok(Box::pin(stream::iter(events)))
    }
}

fn ensure_authorized_script_hash(
    authorized: &AuthorizedToolInput,
    input: &Value,
) -> Result<(), ToolError> {
    let DecisionScope::ExecuteCodeScript { script_hash } = authorized.action_plan().scope else {
        return Err(ToolError::PermissionDenied(
            "authorized execute_code script scope missing".to_owned(),
        ));
    };
    let actual = blake3::hash(source(input).map_err(validation_error)?.as_bytes());
    if *actual.as_bytes() != script_hash {
        return Err(ToolError::PermissionDenied(
            "authorized execute_code script hash mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn language(input: &Value) -> Result<CodeLanguage, ValidationError> {
    match input
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("mini_lua")
    {
        "mini_lua" => Ok(CodeLanguage::MiniLua),
        other => Err(ValidationError::from(format!(
            "unsupported code language: {other}"
        ))),
    }
}

fn source(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.is_empty())
        .ok_or_else(|| ValidationError::from("source is required"))
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}
