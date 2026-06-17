use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, RunCancellerCap, ToolCapability, ToolDescriptor, ToolError,
    ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct TaskStopTool {
    descriptor: ToolDescriptor,
}

impl Default for TaskStopTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "TaskStop",
                "Task stop",
                "Request graceful stop for the current run.",
                ToolGroup::Agent,
                false,
                false,
                false,
                1_000,
                vec![ToolCapability::RunCanceller],
                super::object_schema(
                    &["reason"],
                    json!({
                        "reason": { "type": "string" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for TaskStopTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        reason(input)?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            scope: DecisionScope::ToolName(self.descriptor.name.clone()),
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let canceller = ctx.capability::<dyn RunCancellerCap>(ToolCapability::RunCanceller)?;
        let reason = reason(&input).map_err(validation_error)?.to_owned();
        canceller
            .request_stop(ctx.tenant_id, ctx.session_id, ctx.run_id, reason.clone())
            .await?;
        ctx.interrupt.interrupt();
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "stopped": true,
                "reason": reason
            })),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn reason(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("reason")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("reason is required"))
}
