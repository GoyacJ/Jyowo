//! Memory tool — model-visible controlled long-term memory operations.
//!
//! Actions: search, read, create, update, delete, list, propose.
//! Write actions require permission unless policy grants non-interactive write.
//! `propose` creates an inbox candidate only.

use std::path::PathBuf;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolActionPlan, ToolCapability, ToolDescriptor, ToolError,
    ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

pub const MEMORY_TOOL_RUNTIME_CAPABILITY: &str = "jyowo.memory.tool_runtime";

#[must_use]
pub fn memory_tool_runtime_capability() -> ToolCapability {
    ToolCapability::Custom(MEMORY_TOOL_RUNTIME_CAPABILITY.to_owned())
}

#[derive(Debug, Clone)]
pub struct MemoryToolRuntimeRequest {
    pub action: String,
    pub input: Value,
    pub permission_context: harness_contracts::MemoryPermissionContext,
    pub tenant_id: harness_contracts::TenantId,
    pub session_id: harness_contracts::SessionId,
    pub run_id: harness_contracts::RunId,
    pub tool_use_id: harness_contracts::ToolUseId,
    pub workspace_root: PathBuf,
}

#[async_trait]
pub trait MemoryToolRuntimeCap: Send + Sync + 'static {
    async fn execute(&self, request: MemoryToolRuntimeRequest) -> Result<Value, ToolError>;
}

#[derive(Clone)]
pub struct MemoryTool {
    descriptor: ToolDescriptor,
}

impl Default for MemoryTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "memory",
                "Memory",
                "Search, read, create, update, delete, list, and propose long-term memories. Write actions (create/update/delete) require explicit user permission. propose creates a candidate for review without direct storage. Search returns records matching the query via FTS5 lexical search.",
                ToolGroup::Memory,
                false,  // not concurrency safe (mutates shared state)
                false,  // not read-only (has write actions)
                true,   // destructive (delete action)
                64_000, // budget limit
                vec![memory_tool_runtime_capability()],
                memory_tool_schema(),
            ),
        }
    }
}

fn memory_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["search", "read", "create", "update", "delete", "list", "propose"],
                "description": "The memory action to perform."
            },
            "query": {
                "type": "string",
                "description": "Search query text (for search action)."
            },
            "max_records": {
                "type": "integer",
                "minimum": 1,
                "maximum": 50,
                "default": 10,
                "description": "Maximum records to return (for search/list actions)."
            },
            "memory_id": {
                "type": "string",
                "description": "Memory record ID (for read/update/delete actions)."
            },
            "reason": {
                "type": "string",
                "description": "Reason for deletion (for delete action)."
            },
            "draft": {
                "type": "object",
                "description": "Memory record draft (for create/update/propose actions).",
                "required": ["kind", "visibility", "content"],
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["user_preference", "feedback", "project_fact", "reference", "agent_self_note"]
                    },
                    "visibility": {
                        "type": "string",
                        "enum": ["user", "tenant"],
                        "description": "Visibility scope: user or tenant."
                    },
                    "content": {
                        "type": "string",
                        "description": "Memory content text."
                    }
                }
            },
            "visibility": {
                "type": "string",
                "enum": ["user", "tenant"],
                "description": "Filter by visibility (for search/list actions)."
            },
            "include_expired": {
                "type": "boolean",
                "default": false,
                "description": "Include expired records (for list action)."
            },
            "include_deleted": {
                "type": "boolean",
                "default": false,
                "description": "Include deleted records (for list action)."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "default": 20,
                "description": "Maximum records to return (for list action)."
            },
            "cursor": {
                "type": "string",
                "description": "Pagination cursor from previous response."
            }
        }
    })
}

#[async_trait]
impl Tool for MemoryTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| ValidationError::from("action is required"))?;

        match action {
            "search" => {
                require_field(input, "query")?;
            }
            "read" | "delete" => {
                require_field(input, "memory_id")?;
            }
            "update" => {
                require_field(input, "memory_id")?;
                require_field(input, "draft")?;
                let draft = input.get("draft").unwrap();
                require_field(draft, "kind")?;
                require_field(draft, "visibility")?;
                require_field(draft, "content")?;
            }
            "create" | "propose" => {
                require_field(input, "draft")?;
                let draft = input.get("draft").unwrap();
                require_field(draft, "kind")?;
                require_field(draft, "visibility")?;
                require_field(draft, "content")?;
            }
            "list" => {} // no required fields beyond action
            _ => return Err(ValidationError::from(format!("unknown action: {action}"))),
        }
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        // All memory tool actions require permission planning for safety.
        // Read-only actions (search/read/list) may be auto-authorized by policy.
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
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        let result = match action {
            "search" => execute_search(&input, &ctx, &authorized).await,
            "read" => execute_read(&input, &ctx, &authorized).await,
            "create" => execute_create(&input, &ctx, &authorized).await,
            "update" => execute_update(&input, &ctx, &authorized).await,
            "delete" => execute_delete(&input, &ctx, &authorized).await,
            "list" => execute_list(&input, &ctx, &authorized).await,
            "propose" => execute_propose(&input, &ctx, &authorized).await,
            _ => Err(ToolError::Validation(format!("unknown action: {action}"))),
        };

        match result {
            Ok(mut value) => {
                sanitize_memory_tool_output(&mut value);
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Structured(value),
                )])))
            }
            Err(e) => Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Structured(json!({
                    "error": e.to_string(),
                    "state": "denied"
                })),
            )]))),
        }
    }
}

fn sanitize_memory_tool_output(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.remove("raw_content").is_some() && !map.contains_key("content_preview") {
                map.insert(
                    "content_preview".to_owned(),
                    Value::String("[redacted memory content]".to_owned()),
                );
            }
            if map.remove("content").is_some() && !map.contains_key("content_preview") {
                map.insert(
                    "content_preview".to_owned(),
                    Value::String("[redacted memory content]".to_owned()),
                );
            }
            for child in map.values_mut() {
                sanitize_memory_tool_output(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                sanitize_memory_tool_output(child);
            }
        }
        _ => {}
    }
}

async fn execute_search(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    let query = input["query"].as_str().unwrap_or("");
    let max_records = input
        .get("max_records")
        .and_then(Value::as_u64)
        .unwrap_or(10) as u32;

    execute_runtime(
        "search",
        json!({
            "query": query,
            "max_records": max_records,
            "visibility": input.get("visibility").cloned().unwrap_or(Value::Null)
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_read(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    let memory_id = input["memory_id"].as_str().unwrap_or("");
    execute_runtime(
        "read",
        json!({
            "memory_id": memory_id
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_create(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    execute_runtime(
        "create",
        json!({
            "draft": input["draft"].clone()
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_update(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    let memory_id = input["memory_id"].as_str().unwrap_or("");
    execute_runtime(
        "update",
        json!({
            "memory_id": memory_id,
            "draft": input["draft"].clone()
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_delete(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    let memory_id = input["memory_id"].as_str().unwrap_or("");
    let reason = input["reason"].as_str().unwrap_or("not specified");
    execute_runtime(
        "delete",
        json!({
            "memory_id": memory_id,
            "reason": reason
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_list(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(20) as u32;
    execute_runtime(
        "list",
        json!({
            "limit": limit,
            "cursor": input.get("cursor").cloned().unwrap_or(Value::Null),
            "visibility": input.get("visibility").cloned().unwrap_or(Value::Null),
            "include_expired": input.get("include_expired").and_then(Value::as_bool).unwrap_or(false),
            "include_deleted": input.get("include_deleted").and_then(Value::as_bool).unwrap_or(false)
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_propose(
    input: &Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    execute_runtime(
        "propose",
        json!({
            "draft": input["draft"].clone()
        }),
        ctx,
        authorized,
    )
    .await
}

async fn execute_runtime(
    action: &str,
    input: Value,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<Value, ToolError> {
    let runtime = ctx.capability::<dyn MemoryToolRuntimeCap>(memory_tool_runtime_capability())?;
    runtime
        .execute(MemoryToolRuntimeRequest {
            action: action.to_owned(),
            input,
            permission_context: harness_contracts::MemoryPermissionContext {
                explicit_user_instruction: true,
                action_plan_id: Some(authorized.action_plan().plan_id),
                authorization_ticket_id: Some(authorized.ticket().ticket_id),
                non_interactive_policy_grant: false,
            },
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: ctx.tool_use_id,
            workspace_root: ctx.workspace_root.clone(),
        })
        .await
}

fn require_field(input: &Value, field: &str) -> Result<(), ValidationError> {
    if input.get(field).is_none() {
        return Err(ValidationError::from(format!("{field} is required")));
    }
    Ok(())
}
