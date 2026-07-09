use std::path::Path;

use async_trait::async_trait;
use futures::stream;
use globset::{Glob, GlobSetBuilder};
use harness_contracts::{
    ActionResource, DecisionScope, NetworkAccess, PermissionSubject, ToolActionPlan,
    ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{
    action_plan_from_permission_check, authorized_file_path, AuthorizedFileResourceKind,
    AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError,
};

#[derive(Clone)]
pub struct GlobTool {
    descriptor: ToolDescriptor,
}

impl Default for GlobTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_output_schema(
                super::descriptor(
                    "Glob",
                    "Glob",
                    "Find files by glob pattern.",
                    ToolGroup::Search,
                    true,
                    true,
                    false,
                    32_000,
                    Vec::new(),
                    super::object_schema(
                        &["path", "pattern"],
                        json!({
                            "path": { "type": "string" },
                            "pattern": { "type": "string" },
                            "include_hidden": { "type": "boolean" }
                        }),
                    ),
                ),
                json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["path"],
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "additionalProperties": false
                    }
                }),
            ),
        }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        super::workspace_path::input_path(input)?;
        pattern(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let path = match super::workspace_path::resolve_existing(input, ctx) {
            Ok(path) => path,
            Err(error) => return Err(error),
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
            return action_plan_from_permission_check(
                &self.descriptor,
                input,
                ctx,
                check,
                vec![ActionResource::FileRead { path }],
                WorkspaceAccess::ReadOnly,
                NetworkAccess::None,
                ToolExecutionChannel::DirectAuthorizedRust,
            );
        }
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::PathPrefix(path.clone()),
            },
            vec![ActionResource::FileRead { path }],
            WorkspaceAccess::ReadOnly,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let root = authorized_file_path(&authorized, AuthorizedFileResourceKind::Read)?;
        super::workspace_path::ensure_inside_workspace(&root, &ctx)?;
        let input = authorized.raw_input();
        let include_hidden = input
            .get("include_hidden")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut builder = GlobSetBuilder::new();
        builder.add(
            Glob::new(pattern(&input).map_err(validation_error)?)
                .map_err(|error| ToolError::Validation(format!("invalid glob pattern: {error}")))?,
        );
        let matcher = builder
            .build()
            .map_err(|error| ToolError::Validation(error.to_string()))?;

        let mut matches = Vec::new();
        collect_matches(&root, &root, &ctx, include_hidden, &matcher, &mut matches)?;
        matches.sort_by(|left, right| {
            left["path"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["path"].as_str().unwrap_or_default())
        });

        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(Value::Array(matches)),
        )])))
    }
}

fn collect_matches(
    root: &Path,
    dir: &Path,
    ctx: &ToolContext,
    include_hidden: bool,
    matcher: &globset::GlobSet,
    out: &mut Vec<Value>,
) -> Result<(), ToolError> {
    for entry in std::fs::read_dir(dir).map_err(|error| ToolError::Message(error.to_string()))? {
        let entry = entry.map_err(|error| ToolError::Message(error.to_string()))?;
        let path = entry.path();
        super::workspace_path::ensure_inside_workspace(&path, ctx)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_matches(root, &path, ctx, include_hidden, matcher, out)?;
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if matcher.is_match(relative) {
            out.push(json!({
                "path": relative.to_string_lossy().replace('\\', "/")
            }));
        }
    }
    Ok(())
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn pattern(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("pattern")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("pattern is required"))
}
