use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct FileEditTool {
    descriptor: ToolDescriptor,
}

impl Default for FileEditTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "FileEdit",
                "File edit",
                "Replace text in a workspace file.",
                ToolGroup::FileSystem,
                false,
                false,
                true,
                64_000,
                Vec::new(),
                super::object_schema(
                    &["path", "old", "new"],
                    json!({
                        "path": { "type": "string" },
                        "old": { "type": "string" },
                        "new": { "type": "string" },
                        "replace_all": { "type": "boolean" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        super::workspace_path::input_path(input)?;
        old_text(input)?;
        new_text(input)?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck {
        let scoped_path = match super::workspace_path::scope_path(input, ctx) {
            Ok(path) => path,
            Err(error) => {
                return PermissionCheck::Denied {
                    reason: error.to_string(),
                };
            }
        };
        let bytes_preview = new_text(input)
            .unwrap_or_default()
            .as_bytes()
            .iter()
            .copied()
            .take(512)
            .collect::<Vec<_>>();
        if let Some(check) = super::workspace_path::dangerous_path_permission(
            input,
            ctx,
            PermissionSubject::FileWrite {
                path: scoped_path.clone(),
                bytes_preview: bytes_preview.clone(),
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
                bytes_preview,
            },
            scope: DecisionScope::PathPrefix(path),
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let path = super::workspace_path::resolve_writable(&input, &ctx)?;
        let old = old_text(&input).map_err(validation_error)?;
        let new = new_text(&input).map_err(validation_error)?;
        let replace_all = input
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = std::fs::read_to_string(&path)
            .map_err(|error| ToolError::Message(error.to_string()))?;
        let replacements = if replace_all {
            content.matches(old).count()
        } else {
            usize::from(content.contains(old))
        };
        let edited = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        std::fs::write(&path, edited).map_err(|error| ToolError::Message(error.to_string()))?;
        let result_path = path
            .canonicalize()
            .map_err(|error| ToolError::Message(error.to_string()))?;

        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "path": result_path,
                "replacements": replacements
            })),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn old_text(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("old")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("old is required"))
}

fn new_text(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("new")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("new is required"))
}
