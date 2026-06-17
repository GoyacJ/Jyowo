use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    CodeLanguage, CodeRunRequest, DecisionScope, PermissionSubject, ToolCapability, ToolDescriptor,
    ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct ExecuteCodeTool {
    descriptor: ToolDescriptor,
}

impl Default for ExecuteCodeTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
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

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck {
        if ctx.subagent_depth > 0 {
            return PermissionCheck::Denied {
                reason: "execute_code is not available from subagents".to_owned(),
            };
        }
        let script_hash = blake3::hash(source(input).unwrap_or_default().as_bytes());
        PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            scope: DecisionScope::ExecuteCodeScript {
                script_hash: *script_hash.as_bytes(),
            },
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
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
                    language: language(&input).map_err(validation_error)?,
                    source: source(&input).map_err(validation_error)?.to_owned(),
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
