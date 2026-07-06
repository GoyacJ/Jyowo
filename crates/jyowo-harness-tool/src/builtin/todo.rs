use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, TodoItem, TodoStoreCap, ToolActionPlan, ToolCapability,
    ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct TodoTool {
    descriptor: ToolDescriptor,
}

impl Default for TodoTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "Todo",
                "Todo",
                "Update the run todo list.",
                ToolGroup::Memory,
                false,
                false,
                false,
                32_000,
                vec![ToolCapability::TodoStore],
                super::object_schema(
                    &["items"],
                    json!({
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["content", "status"],
                                "properties": {
                                    "content": { "type": "string" },
                                    "status": { "type": "string" }
                                }
                            }
                        }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        items(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        super::generic_action_plan(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let store = ctx.capability::<dyn TodoStoreCap>(ToolCapability::TodoStore)?;
        let items = todo_items(authorized.raw_input()).map_err(validation_error)?;
        let count = items.len();
        store
            .replace_todos(ctx.tenant_id, ctx.session_id, ctx.run_id, items)
            .await?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "accepted": true,
                "items": count
            })),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn items(input: &Value) -> Result<&Vec<Value>, ValidationError> {
    input
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| ValidationError::from("items is required"))
}

fn todo_items(input: &Value) -> Result<Vec<TodoItem>, ValidationError> {
    items(input)?
        .iter()
        .map(|item| {
            Ok(TodoItem {
                content: item
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ValidationError::from("item.content is required"))?
                    .to_owned(),
                status: item
                    .get("status")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ValidationError::from("item.status is required"))?
                    .to_owned(),
            })
        })
        .collect()
}
