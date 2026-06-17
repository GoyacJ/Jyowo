use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct FileWriteTool {
    descriptor: ToolDescriptor,
}

impl Default for FileWriteTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "FileWrite",
                "File write",
                "Overwrite a workspace file.",
                ToolGroup::FileSystem,
                false,
                false,
                true,
                64_000,
                Vec::new(),
                super::object_schema(
                    &["path", "content"],
                    json!({
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        super::workspace_path::input_path(input)?;
        content(input)?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck {
        let content = content(input).unwrap_or_default();
        let scoped_path = match super::workspace_path::scope_path(input, ctx) {
            Ok(path) => path,
            Err(error) => {
                return PermissionCheck::Denied {
                    reason: error.to_string(),
                };
            }
        };
        if let Some(check) = super::workspace_path::dangerous_path_permission(
            input,
            ctx,
            PermissionSubject::FileWrite {
                path: scoped_path.clone(),
                bytes_preview: content.as_bytes().iter().copied().take(512).collect(),
            },
            DecisionScope::PathPrefix(scoped_path),
        ) {
            return check;
        }
        let path = match super::workspace_path::resolve_writable(input, ctx) {
            Ok(path) => path,
            Err(error) => {
                return PermissionCheck::Denied {
                    reason: error.to_string(),
                };
            }
        };
        PermissionCheck::AskUser {
            subject: PermissionSubject::FileWrite {
                path: path.clone(),
                bytes_preview: content.as_bytes().iter().copied().take(512).collect(),
            },
            scope: DecisionScope::PathPrefix(path),
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let path = super::workspace_path::resolve_writable(&input, &ctx)?;
        let content = content(&input).map_err(validation_error)?;
        std::fs::write(&path, content).map_err(|error| ToolError::Message(error.to_string()))?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "path": path,
                "bytes": content.len()
            })),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn content(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("content is required"))
}
