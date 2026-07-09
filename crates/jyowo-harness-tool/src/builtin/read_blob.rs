use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    ActionResource, BlobReaderCap, BlobRef, DecisionScope, NetworkAccess,
    OffloadedBlobAuthorizerCap, PermissionSubject, ToolActionPlan, ToolCapability, ToolDescriptor,
    ToolError, ToolExecutionChannel, ToolGroup, ToolResult, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

const DEFAULT_READ_LIMIT: usize = 64_000;

#[derive(Clone)]
pub struct ReadBlobTool {
    descriptor: ToolDescriptor,
}

impl Default for ReadBlobTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_output_schema(
                super::descriptor(
                    "ReadBlob",
                    "Read blob",
                    "Read a previously offloaded tool result blob.",
                    ToolGroup::Meta,
                    true,
                    true,
                    false,
                    64_000,
                    vec![
                        ToolCapability::BlobReader,
                        ToolCapability::OffloadedBlobAuthorizer,
                    ],
                    super::object_schema(
                        &["blob_ref"],
                        json!({
                            "blob_ref": { "type": "object" },
                            "offset": { "type": "integer", "minimum": 0 },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": DEFAULT_READ_LIMIT
                            }
                        }),
                    ),
                ),
                super::text_output_schema(),
            ),
        }
    }
}

#[async_trait]
impl Tool for ReadBlobTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        blob_ref(input)?;
        read_window(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let blob_ref = blob_ref(input).map_err(|error| ToolError::Validation(error.to_string()))?;
        action_plan_from_permission_check(
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
            vec![ActionResource::BlobRead { blob_ref }],
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input();
        let blob_ref = blob_ref(input).map_err(validation_error)?;
        let window = read_window(input).map_err(validation_error)?;
        let authorizer = ctx.capability::<dyn OffloadedBlobAuthorizerCap>(
            ToolCapability::OffloadedBlobAuthorizer,
        )?;
        authorizer
            .authorize_offloaded_blob(ctx.tenant_id, ctx.session_id, ctx.run_id, blob_ref.clone())
            .await?;
        let reader = ctx.capability::<dyn BlobReaderCap>(ToolCapability::BlobReader)?;
        let mut blob_stream = reader
            .read_blob(ctx.tenant_id, blob_ref)
            .await
            .map_err(|error| ToolError::Message(error.to_string()))?;
        let mut bytes = Vec::with_capacity(window.limit);
        let mut remaining_skip = window.offset;
        let mut remaining_take = window.limit;
        while remaining_take > 0 {
            let Some(chunk) = blob_stream.next().await else {
                break;
            };
            if remaining_skip >= chunk.len() {
                remaining_skip -= chunk.len();
                continue;
            }
            let start = remaining_skip;
            remaining_skip = 0;
            let available = &chunk[start..];
            let take = available.len().min(remaining_take);
            bytes.extend_from_slice(&available[..take]);
            remaining_take -= take;
        }
        let text =
            String::from_utf8(bytes).map_err(|error| ToolError::Message(error.to_string()))?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text(text),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn blob_ref(input: &Value) -> Result<BlobRef, ValidationError> {
    serde_json::from_value(
        input
            .get("blob_ref")
            .cloned()
            .ok_or_else(|| ValidationError::from("blob_ref is required"))?,
    )
    .map_err(|error| ValidationError::from(error.to_string()))
}

struct ReadWindow {
    offset: usize,
    limit: usize,
}

fn read_window(input: &Value) -> Result<ReadWindow, ValidationError> {
    let offset =
        optional_usize(input, "offset", "offset must be a non-negative integer")?.unwrap_or(0);
    let limit = optional_usize(input, "limit", "limit must be a positive integer")?
        .unwrap_or(DEFAULT_READ_LIMIT);
    if limit == 0 {
        return Err(ValidationError::from("limit must be greater than 0"));
    }
    if limit > DEFAULT_READ_LIMIT {
        return Err(ValidationError::from(format!(
            "limit must be <= {DEFAULT_READ_LIMIT}"
        )));
    }
    Ok(ReadWindow { offset, limit })
}

fn optional_usize(
    input: &Value,
    field: &str,
    type_error: &str,
) -> Result<Option<usize>, ValidationError> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    let raw = value
        .as_u64()
        .ok_or_else(|| ValidationError::from(type_error))?;
    let parsed =
        usize::try_from(raw).map_err(|_| ValidationError::from(format!("{field} is too large")))?;
    Ok(Some(parsed))
}
