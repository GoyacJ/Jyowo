//! Memory tool — model-visible controlled long-term memory operations.
//!
//! Actions: search, read, create, update, delete, list, propose.
//! Write actions require permission unless policy grants non-interactive write.
//! `propose` creates an inbox candidate only.

use std::path::PathBuf;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionPlanId, ActionResource, DecisionScope, MemoryId, MemoryKind, MemoryMetadata,
    MemoryPermissionContext, MemoryPolicyDenyReason, MemoryProviderSelectionPolicy,
    MemoryRedactionSummary, MemoryTakesEffect, MemoryThreadSettings, MemoryToolArgs,
    MemoryToolDenial, MemoryToolResponse, MemoryToolState, NetworkAccess, PermissionSubject,
    ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolResult, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

pub const MEMORY_TOOL_RUNTIME_CAPABILITY: &str = "jyowo.memory.tool_runtime";

#[must_use]
pub fn memory_tool_runtime_capability() -> ToolCapability {
    ToolCapability::Custom(MEMORY_TOOL_RUNTIME_CAPABILITY.to_owned())
}

#[derive(Debug, Clone)]
pub struct MemoryToolRuntimeRequest {
    pub action: MemoryToolRuntimeAction,
    pub permission_context: MemoryPermissionContext,
    pub provider_policy: MemoryProviderSelectionPolicy,
    pub tenant_id: harness_contracts::TenantId,
    pub session_id: harness_contracts::SessionId,
    pub run_id: harness_contracts::RunId,
    pub tool_use_id: harness_contracts::ToolUseId,
    pub workspace_root: PathBuf,
    pub memory_thread_settings: Option<MemoryThreadSettings>,
}

#[async_trait]
pub trait MemoryToolRuntimeCap: Send + Sync + 'static {
    async fn execute(
        &self,
        request: MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum MemoryToolRuntimeAction {
    Search {
        query: String,
        #[serde(default = "default_search_limit")]
        max_records: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        visibility: Option<MemoryToolVisibility>,
    },
    Read {
        memory_id: MemoryId,
    },
    Create {
        draft: MemoryToolDraft,
    },
    Update {
        memory_id: MemoryId,
        draft: MemoryToolDraft,
    },
    Delete {
        memory_id: MemoryId,
        #[serde(default = "default_delete_reason")]
        reason: String,
    },
    List {
        #[serde(default = "default_list_limit")]
        limit: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        visibility: Option<MemoryToolVisibility>,
        #[serde(default)]
        include_expired: bool,
        #[serde(default)]
        include_deleted: bool,
    },
    Propose {
        draft: MemoryToolDraft,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryToolVisibility {
    User,
    Tenant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolDraft {
    pub kind: MemoryKind,
    pub visibility: MemoryToolVisibility,
    pub content: String,
    #[serde(default = "default_memory_metadata")]
    pub metadata: MemoryMetadata,
}

fn default_search_limit() -> u32 {
    10
}

fn default_list_limit() -> u32 {
    20
}

fn default_delete_reason() -> String {
    "not specified".to_owned()
}

fn default_memory_metadata() -> MemoryMetadata {
    MemoryMetadata {
        ttl: None,
        tags: Vec::new(),
        source_trust: 0.5,
    }
}

#[derive(Clone)]
pub struct MemoryTool {
    descriptor: ToolDescriptor,
}

impl Default for MemoryTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_output_schema(
                super::descriptor(
                    "memory",
                    "Memory",
                    "Search, read, create, update, delete, list, and propose long-term memories. Write actions (create/update/delete) require explicit user permission. propose creates a candidate for review without direct storage. Search returns records matching the query via FTS5 lexical search.",
                    ToolGroup::Memory,
                    false,  // not concurrency safe (mutates shared state)
                    false,  // not read-only (has write actions)
                    true,   // destructive (delete action)
                    64_000, // budget limit
                    vec![memory_tool_runtime_capability()],
                    generated_memory_tool_schema(),
                ),
                serde_json::to_value(schemars::schema_for!(MemoryToolResponse))
                    .unwrap_or_else(|_| json!({"type": "object"})),
            ),
        }
    }
}

fn generated_memory_tool_schema() -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(MemoryToolArgs))
        .unwrap_or_else(|_| json!({"type": "object"}));
    if let Some(object) = schema.as_object_mut() {
        object.insert("additionalProperties".to_owned(), json!(false));
    }
    schema
}

#[async_trait]
impl Tool for MemoryTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        parse_action(input).map_err(|error| ValidationError::from(error.to_string()))?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let action = parse_action(input)?;
        let permission = if matches!(
            action,
            MemoryToolRuntimeAction::Search { .. }
                | MemoryToolRuntimeAction::Read { .. }
                | MemoryToolRuntimeAction::List { .. }
        ) {
            PermissionCheck::Allowed
        } else {
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            }
        };

        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            permission,
            vec![memory_resource(&action)],
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
        let action = parse_action(authorized.raw_input())?;
        let result = execute_runtime(action.clone(), &ctx, &authorized).await;

        match result {
            Ok(response) => {
                let mut value = serde_json::to_value(response).map_err(|error| {
                    ToolError::Internal(format!("serialize memory tool response: {error}"))
                })?;
                sanitize_memory_tool_output(&mut value);
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Structured(value),
                )])))
            }
            Err(error) => {
                let mut value = serde_json::to_value(denied_memory_tool_response(
                    &action,
                    error,
                    Some(authorized.action_plan().plan_id),
                ))
                .map_err(|error| {
                    ToolError::Internal(format!("serialize memory tool denial: {error}"))
                })?;
                sanitize_memory_tool_output(&mut value);
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Structured(value),
                )])))
            }
        }
    }
}

fn denied_memory_tool_response(
    action: &MemoryToolRuntimeAction,
    error: ToolError,
    action_plan_id: Option<ActionPlanId>,
) -> MemoryToolResponse {
    let safe_message = error.to_string();
    MemoryToolResponse {
        action: memory_tool_action_name(action).to_owned(),
        state: MemoryToolState::Denied {
            reason: MemoryPolicyDenyReason::MissingPolicy,
        },
        memory_ids: Vec::new(),
        candidate_ids: Vec::new(),
        records: Vec::new(),
        next_cursor: None,
        action_plan_id,
        denial: Some(MemoryToolDenial {
            reason: MemoryPolicyDenyReason::MissingPolicy,
            safe_message,
            action_plan_id,
        }),
        redaction: MemoryRedactionSummary {
            redacted_count: 0,
            dropped_count: 0,
        },
        trace_id: None,
        takes_effect: MemoryTakesEffect::Never,
    }
}

fn memory_tool_action_name(action: &MemoryToolRuntimeAction) -> &'static str {
    match action {
        MemoryToolRuntimeAction::Search { .. } => "search",
        MemoryToolRuntimeAction::Read { .. } => "read",
        MemoryToolRuntimeAction::Create { .. } => "create",
        MemoryToolRuntimeAction::Update { .. } => "update",
        MemoryToolRuntimeAction::Delete { .. } => "delete",
        MemoryToolRuntimeAction::List { .. } => "list",
        MemoryToolRuntimeAction::Propose { .. } => "propose",
    }
}

fn memory_resource(action: &MemoryToolRuntimeAction) -> ActionResource {
    let subject = match action {
        MemoryToolRuntimeAction::Search { query, .. } => Some(format!("query:{query}")),
        MemoryToolRuntimeAction::Read { memory_id }
        | MemoryToolRuntimeAction::Update { memory_id, .. }
        | MemoryToolRuntimeAction::Delete { memory_id, .. } => Some(memory_id.to_string()),
        MemoryToolRuntimeAction::Create { draft } | MemoryToolRuntimeAction::Propose { draft } => {
            Some(format!("{:?}:{:?}", draft.visibility, draft.kind))
        }
        MemoryToolRuntimeAction::List { visibility, .. } => visibility
            .as_ref()
            .map(|visibility| format!("visibility:{visibility:?}")),
    };
    ActionResource::Memory {
        action: memory_tool_action_name(action).to_owned(),
        subject,
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

async fn execute_runtime(
    action: MemoryToolRuntimeAction,
    ctx: &ToolContext,
    authorized: &AuthorizedToolInput,
) -> Result<MemoryToolResponse, ToolError> {
    let runtime = ctx.capability::<dyn MemoryToolRuntimeCap>(memory_tool_runtime_capability())?;
    runtime
        .execute(MemoryToolRuntimeRequest {
            action,
            permission_context: MemoryPermissionContext {
                explicit_user_instruction: false,
                include_raw_content: false,
                action_plan_id: Some(authorized.action_plan().plan_id),
                authorization_ticket_id: Some(authorized.ticket().ticket_id()),
                non_interactive_policy_grant: false,
            },
            provider_policy: MemoryProviderSelectionPolicy::PolicySelected,
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: ctx.tool_use_id,
            workspace_root: ctx.workspace_root.clone(),
            memory_thread_settings: ctx.memory_thread_settings.clone(),
        })
        .await
}

fn parse_action(input: &Value) -> Result<MemoryToolRuntimeAction, ToolError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ToolError::Validation(format!("invalid memory tool input: {error}")))
}
