use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, OutboundUserMessage, PermissionSubject, ToolCapability, ToolDescriptor,
    ToolError, ToolGroup, ToolResult, UserMessengerCap,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct SendMessageTool {
    descriptor: ToolDescriptor,
}

impl Default for SendMessageTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "SendMessage",
                "Send message",
                "Send an outbound user message through the session channel.",
                ToolGroup::Network,
                false,
                false,
                false,
                4_000,
                vec![ToolCapability::UserMessenger],
                super::object_schema(
                    &["channel", "body"],
                    json!({
                        "channel": { "type": "string" },
                        "body": { "type": "string" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        message(input)?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::AskUser {
            subject: PermissionSubject::NetworkAccess {
                host: input
                    .get("channel")
                    .and_then(Value::as_str)
                    .unwrap_or("user-message")
                    .to_owned(),
                port: None,
            },
            scope: DecisionScope::ToolName(self.descriptor.name.clone()),
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let messenger = ctx.capability::<dyn UserMessengerCap>(ToolCapability::UserMessenger)?;
        let delivery = messenger
            .send(message(&input).map_err(validation_error)?)
            .await?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "message_id": delivery.message_id,
                "delivered": delivery.delivered
            })),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn message(input: &Value) -> Result<OutboundUserMessage, ValidationError> {
    let channel = input
        .get("channel")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("channel is required"))?
        .to_owned();
    let body = input
        .get("body")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("body is required"))?
        .to_owned();
    Ok(OutboundUserMessage { channel, body })
}
