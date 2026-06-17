use std::path::Path;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct ListDirTool {
    descriptor: ToolDescriptor,
}

impl Default for ListDirTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "ListDir",
                "List directory",
                "List workspace directory entries.",
                ToolGroup::FileSystem,
                true,
                true,
                false,
                32_000,
                Vec::new(),
                super::object_schema(
                    &["path"],
                    json!({
                        "path": { "type": "string" },
                        "max_depth": { "type": "integer", "minimum": 1 },
                        "include_hidden": { "type": "boolean" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        super::workspace_path::input_path(input)?;
        max_depth(input)?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck {
        if let Ok(path) = super::workspace_path::scope_path(input, ctx) {
            if let Some(check) = super::workspace_path::dangerous_path_permission(
                input,
                ctx,
                PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                DecisionScope::PathPrefix(path),
            ) {
                return check;
            }
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
        let root = super::workspace_path::resolve_existing(&input, &ctx)?;
        let include_hidden = input
            .get("include_hidden")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_depth =
            max_depth(&input).map_err(|error| ToolError::Validation(error.to_string()))?;
        let mut entries = Vec::new();
        collect_entries(
            &root,
            &root,
            1,
            max_depth,
            include_hidden,
            &ctx,
            &mut entries,
        )?;
        entries.sort_by(|left, right| {
            left["path"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["path"].as_str().unwrap_or_default())
        });
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(Value::Array(entries)),
        )])))
    }
}

fn collect_entries(
    root: &Path,
    current: &Path,
    depth: u32,
    max_depth: u32,
    include_hidden: bool,
    ctx: &ToolContext,
    entries: &mut Vec<Value>,
) -> Result<(), ToolError> {
    for entry in
        std::fs::read_dir(current).map_err(|error| ToolError::Message(error.to_string()))?
    {
        let entry = entry.map_err(|error| ToolError::Message(error.to_string()))?;
        let path = entry.path();
        super::workspace_path::ensure_inside_workspace(&path, ctx)?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !include_hidden && file_name.starts_with('.') {
            continue;
        }
        let meta = entry
            .metadata()
            .map_err(|error| ToolError::Message(error.to_string()))?;
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let modified = meta
            .modified()
            .ok()
            .map(|time| DateTime::<Utc>::from(time).to_rfc3339());
        entries.push(json!({
            "path": relative_path,
            "kind": if meta.is_dir() { "dir" } else { "file" },
            "size": meta.len(),
            "modified": modified
        }));
        if meta.is_dir() && depth < max_depth {
            collect_entries(
                root,
                &path,
                depth + 1,
                max_depth,
                include_hidden,
                ctx,
                entries,
            )?;
        }
    }
    Ok(())
}

fn max_depth(input: &Value) -> Result<u32, ValidationError> {
    let Some(raw) = input.get("max_depth") else {
        return Ok(1);
    };
    let depth = raw
        .as_u64()
        .ok_or_else(|| ValidationError::from("max_depth must be an integer"))?;
    if depth == 0 {
        return Err(ValidationError::from("max_depth must be greater than 0"));
    }
    u32::try_from(depth).map_err(|_| ValidationError::from("max_depth is too large"))
}
