use async_trait::async_trait;
use futures::stream;
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

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let content = content(input).unwrap_or_default();
        let path = match super::workspace_path::resolve_writable(input, ctx) {
            Ok(path) => path,
            Err(error) => return Err(error),
        };
        if let Some(check) = super::workspace_path::dangerous_path_permission(
            input,
            ctx,
            PermissionSubject::FileWrite {
                path: path.clone(),
                bytes_preview: content.as_bytes().iter().copied().take(512).collect(),
            },
            DecisionScope::PathPrefix(path.clone()),
        ) {
            return action_plan_from_permission_check(
                &self.descriptor,
                input,
                ctx,
                check,
                vec![ActionResource::FileWrite {
                    path,
                    content_hash: content_hash(content),
                }],
                WorkspaceAccess::ReadWrite {
                    allowed_writable_subpaths: Vec::new(),
                },
                NetworkAccess::None,
                ToolExecutionChannel::DirectAuthorizedRust,
            );
        }
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::FileWrite {
                    path: path.clone(),
                    bytes_preview: content.as_bytes().iter().copied().take(512).collect(),
                },
                scope: DecisionScope::PathPrefix(path.clone()),
            },
            vec![ActionResource::FileWrite {
                path,
                content_hash: content_hash(content),
            }],
            WorkspaceAccess::ReadWrite {
                allowed_writable_subpaths: Vec::new(),
            },
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let path = authorized_file_path(&authorized, AuthorizedFileResourceKind::Write)?;
        let content = content(authorized.raw_input()).map_err(validation_error)?;
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

fn content_hash(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}
