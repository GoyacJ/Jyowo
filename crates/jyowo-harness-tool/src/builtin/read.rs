use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct FileReadTool {
    descriptor: ToolDescriptor,
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "FileRead",
                "File read",
                "Read a UTF-8 workspace file.",
                ToolGroup::FileSystem,
                true,
                true,
                false,
                64_000,
                Vec::new(),
                super::object_schema(
                    &["path"],
                    json!({
                        "path": { "type": "string" },
                        "start_line": { "type": "integer", "minimum": 1 },
                        "end_line": { "type": "integer", "minimum": 1 }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        super::workspace_path::input_path(input)?;
        line_number(input, "start_line")?;
        line_number(input, "end_line")?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck {
        let path = match super::workspace_path::scope_path(input, ctx) {
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
            PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            DecisionScope::PathPrefix(path.clone()),
        ) {
            return check;
        }
        let path = match super::workspace_path::resolve_existing(input, ctx) {
            Ok(path) => path,
            Err(error) => {
                return PermissionCheck::Denied {
                    reason: error.to_string(),
                };
            }
        };
        PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            scope: DecisionScope::PathPrefix(path),
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let path = super::workspace_path::resolve_existing(&input, &ctx)?;
        let content =
            std::fs::read_to_string(path).map_err(|error| ToolError::Message(error.to_string()))?;
        let content = slice_lines(
            &content,
            line_number(&input, "start_line").map_err(validation_error)?,
            line_number(&input, "end_line").map_err(validation_error)?,
        );
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text(content),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn slice_lines(content: &str, start_line: Option<u64>, end_line: Option<u64>) -> String {
    let start = start_line.unwrap_or(1).max(1);
    let end = end_line.unwrap_or(u64::MAX).max(start);
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index as u64 + 1;
            (line_number >= start && line_number <= end).then_some(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn line_number(input: &Value, field: &str) -> Result<Option<u64>, ValidationError> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    let raw = value
        .as_u64()
        .ok_or_else(|| ValidationError::from(format!("{field} must be a positive integer")))?;
    if raw == 0 {
        return Err(ValidationError::from(format!(
            "{field} must be greater than 0"
        )));
    }
    Ok(Some(raw))
}
