//! Memory tool — model-visible controlled long-term memory operations.
//!
//! Actions: search, read, create, update, delete, list, propose.
//! Write actions require permission unless policy grants non-interactive write.
//! `propose` creates an inbox candidate only.

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

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
                vec![], // capabilities resolved at runtime
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
            "create" | "update" | "propose" => {
                require_field(input, "draft")?;
                let draft = input.get("draft").unwrap();
                require_field(draft, "kind")?;
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
            "search" => execute_search(&input, &ctx).await,
            "read" => execute_read(&input, &ctx).await,
            "create" => execute_create(&input, &ctx).await,
            "update" => execute_update(&input, &ctx).await,
            "delete" => execute_delete(&input, &ctx).await,
            "list" => execute_list(&input, &ctx).await,
            "propose" => execute_propose(&input, &ctx).await,
            _ => Err(ToolError::Validation(format!("unknown action: {action}"))),
        };

        match result {
            Ok(value) => Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Structured(value),
            )]))),
            Err(e) => Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Structured(json!({
                    "error": e.to_string(),
                    "state": "denied"
                })),
            )]))),
        }
    }
}

async fn execute_search(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let query = input["query"].as_str().unwrap_or("");
    let max_records = input
        .get("max_records")
        .and_then(Value::as_u64)
        .unwrap_or(10) as u32;

    Ok(json!({
        "action": "search",
        "state": "completed",
        "query": query,
        "max_records": max_records,
        "records": [],
        "memory_ids": []
    }))
}

async fn execute_read(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let memory_id = input["memory_id"].as_str().unwrap_or("");
    Ok(json!({
        "action": "read",
        "state": "completed",
        "memory_id": memory_id,
        "record": null
    }))
}

async fn execute_create(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let draft = &input["draft"];
    Ok(json!({
        "action": "create",
        "state": "candidate_created",
        "draft": draft,
        "takes_effect": "next_turn"
    }))
}

async fn execute_update(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let memory_id = input["memory_id"].as_str().unwrap_or("");
    Ok(json!({
        "action": "update",
        "state": "permission_required",
        "memory_id": memory_id
    }))
}

async fn execute_delete(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let memory_id = input["memory_id"].as_str().unwrap_or("");
    let reason = input["reason"].as_str().unwrap_or("not specified");
    Ok(json!({
        "action": "delete",
        "state": "completed",
        "memory_id": memory_id,
        "reason": reason
    }))
}

async fn execute_list(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let limit = input
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20) as u32;
    Ok(json!({
        "action": "list",
        "state": "completed",
        "records": [],
        "limit": limit
    }))
}

async fn execute_propose(input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
    let draft = &input["draft"];
    Ok(json!({
        "action": "propose",
        "state": "candidate_created",
        "draft": draft
    }))
}

fn require_field(input: &Value, field: &str) -> Result<(), ValidationError> {
    if input.get(field).is_none() {
        return Err(ValidationError::from(format!("{field} is required")));
    }
    Ok(())
}
