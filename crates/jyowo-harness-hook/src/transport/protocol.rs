use harness_contracts::{Decision, HookError, MessageRole, TransportFailureKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    ContextPatch, ContextPatchRole, HookContext, HookEvent, HookOutcome, PreToolUseOutcome,
};

use super::{HookPayload, HookProtocolVersion};

#[derive(Serialize)]
pub(crate) struct WireRequest {
    pub protocol_version: HookProtocolVersion,
    pub event: Value,
    pub context: Value,
}

#[derive(Deserialize)]
struct WireResponse {
    protocol_version: HookProtocolVersion,
    outcome: WireOutcome,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireOutcome {
    Continue,
    Block { reason: String },
    PreToolUse(WirePreToolUseOutcome),
    RewriteInput(Value),
    OverridePermission(Decision),
    AddContext(WireContextPatch),
    Transform(Value),
}

#[derive(Deserialize)]
struct WirePreToolUseOutcome {
    #[serde(default)]
    rewrite_input: Option<Value>,
    #[serde(default)]
    override_permission: Option<Decision>,
    #[serde(default)]
    additional_context: Option<WireContextPatch>,
    #[serde(default)]
    block: Option<String>,
}

#[derive(Deserialize)]
struct WireContextPatch {
    role: WireContextPatchRole,
    content: String,
    apply_to_next_turn_only: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireContextPatchRole {
    SystemAppend,
    UserPrefix,
    UserSuffix,
    AssistantHint,
}

pub(crate) fn encode_request(payload: &HookPayload, version: HookProtocolVersion) -> WireRequest {
    WireRequest {
        protocol_version: version,
        event: event_to_json(&payload.event),
        context: context_to_json(&payload.ctx),
    }
}

pub(crate) fn decode_response(
    body: &[u8],
    expected: HookProtocolVersion,
) -> Result<HookOutcome, HookError> {
    let response: WireResponse = serde_json::from_slice(body)
        .map_err(|error| HookError::ProtocolParse(error.to_string()))?;
    if response.protocol_version != expected {
        return Err(HookError::Transport {
            kind: TransportFailureKind::ProtocolVersionMismatch,
            detail: "hook protocol version mismatch".to_owned(),
        });
    }

    Ok(match response.outcome {
        WireOutcome::Continue => HookOutcome::Continue,
        WireOutcome::Block { reason } => HookOutcome::Block { reason },
        WireOutcome::PreToolUse(outcome) => HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: outcome.rewrite_input,
            override_permission: outcome.override_permission,
            additional_context: outcome.additional_context.map(Into::into),
            block: outcome.block,
        }),
        WireOutcome::RewriteInput(value) => HookOutcome::RewriteInput(value),
        WireOutcome::OverridePermission(decision) => HookOutcome::OverridePermission(decision),
        WireOutcome::AddContext(patch) => HookOutcome::AddContext(patch.into()),
        WireOutcome::Transform(value) => HookOutcome::Transform(value),
    })
}

fn event_to_json(event: &HookEvent) -> Value {
    match event {
        HookEvent::UserPromptSubmit { run_id, input } => json!({
            "kind": "user_prompt_submit",
            "run_id": run_id,
            "input": input,
        }),
        HookEvent::PreToolUse {
            tool_use_id,
            tool_name,
            input,
        } => json!({
            "kind": "pre_tool_use",
            "tool_use_id": tool_use_id,
            "tool_name": tool_name,
            "input": input,
        }),
        HookEvent::PostToolUse {
            tool_use_id,
            result,
        } => json!({
            "kind": "post_tool_use",
            "tool_use_id": tool_use_id,
            "result": result,
        }),
        HookEvent::PostToolUseFailure { tool_use_id, error } => json!({
            "kind": "post_tool_use_failure",
            "tool_use_id": tool_use_id,
            "error": { "message": error.message },
        }),
        HookEvent::PermissionRequest {
            request_id,
            subject,
            detail,
        } => json!({
            "kind": "permission_request",
            "request_id": request_id,
            "subject": subject,
            "detail": detail,
        }),
        HookEvent::SessionStart { session_id } => json!({
            "kind": "session_start",
            "session_id": session_id,
        }),
        HookEvent::Setup { workspace_root } => json!({
            "kind": "setup",
            "workspace_root": workspace_root,
        }),
        HookEvent::SessionEnd { session_id, reason } => json!({
            "kind": "session_end",
            "session_id": session_id,
            "reason": reason,
        }),
        HookEvent::SubagentStart { subagent_id, spec } => json!({
            "kind": "subagent_start",
            "subagent_id": subagent_id,
            "spec": { "name": spec.name, "description": spec.description },
        }),
        HookEvent::SubagentStop {
            subagent_id,
            status,
        } => json!({
            "kind": "subagent_stop",
            "subagent_id": subagent_id,
            "status": status,
        }),
        HookEvent::Notification { kind, body } => json!({
            "kind": "notification",
            "notification_kind": format!("{kind:?}"),
            "body": body,
        }),
        HookEvent::PreLlmCall {
            run_id,
            request_view,
        } => json!({
            "kind": "pre_llm_call",
            "run_id": run_id,
            "request_view": {
                "provider_id": request_view.provider_id,
                "model_id": request_view.model_id,
                "message_count": request_view.message_count,
                "tool_count": request_view.tool_count,
            },
        }),
        HookEvent::PostLlmCall { run_id, usage } => json!({
            "kind": "post_llm_call",
            "run_id": run_id,
            "usage": usage,
        }),
        HookEvent::PreApiRequest {
            request_id,
            endpoint,
        } => json!({
            "kind": "pre_api_request",
            "request_id": request_id,
            "endpoint": endpoint,
        }),
        HookEvent::PostApiRequest { request_id, status } => json!({
            "kind": "post_api_request",
            "request_id": request_id,
            "status": status,
        }),
        HookEvent::TransformToolResult {
            tool_use_id,
            result,
        } => json!({
            "kind": "transform_tool_result",
            "tool_use_id": tool_use_id,
            "result": result,
        }),
        HookEvent::TransformTerminalOutput { tool_use_id, raw } => json!({
            "kind": "transform_terminal_output",
            "tool_use_id": tool_use_id,
            "raw_utf8_lossy": String::from_utf8_lossy(raw),
        }),
        HookEvent::Elicitation {
            mcp_server_id,
            schema,
        } => json!({
            "kind": "elicitation",
            "mcp_server_id": mcp_server_id,
            "schema": schema,
        }),
        HookEvent::PreToolSearch {
            tool_use_id,
            query,
            query_kind,
        } => json!({
            "kind": "pre_tool_search",
            "tool_use_id": tool_use_id,
            "query": query,
            "query_kind": query_kind,
        }),
        HookEvent::PostToolSearchMaterialize {
            tool_use_id,
            materialized,
            backend,
            cache_impact,
        } => json!({
            "kind": "post_tool_search_materialize",
            "tool_use_id": tool_use_id,
            "materialized": materialized,
            "backend": backend,
            "cache_impact": cache_impact,
        }),
    }
}

fn context_to_json(ctx: &HookContext) -> Value {
    let current_tool_descriptor = ctx.view.current_tool_descriptor().map(|descriptor| {
        json!({
            "name": descriptor.name,
            "display_name": descriptor.display_name,
            "description": descriptor.description,
        })
    });
    let recent_messages: Vec<_> = ctx
        .view
        .recent_messages(8)
        .into_iter()
        .map(|message| {
            json!({
                "role": message_role(message.role),
                "text_snippet": message.text_snippet,
                "tool_use_id": message.tool_use_id,
            })
        })
        .collect();

    json!({
        "tenant_id": ctx.tenant_id,
        "session_id": ctx.session_id,
        "run_id": ctx.run_id,
        "turn_index": ctx.turn_index,
        "correlation_id": ctx.correlation_id,
        "causation_id": ctx.causation_id,
        "trust_level": ctx.trust_level,
        "permission_mode": ctx.permission_mode,
        "interactivity": ctx.interactivity,
        "at": ctx.at,
        "workspace_root": ctx.view.workspace_root(),
        "recent_messages": recent_messages,
        "current_tool_descriptor": current_tool_descriptor,
        "replay_mode": format!("{:?}", ctx.replay_mode).to_lowercase(),
    })
}

fn message_role(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
        _ => "unknown",
    }
}

impl From<WirePreToolUseOutcome> for PreToolUseOutcome {
    fn from(value: WirePreToolUseOutcome) -> Self {
        Self {
            rewrite_input: value.rewrite_input,
            override_permission: value.override_permission,
            additional_context: value.additional_context.map(Into::into),
            block: value.block,
        }
    }
}

impl From<WireContextPatch> for ContextPatch {
    fn from(value: WireContextPatch) -> Self {
        Self {
            role: value.role.into(),
            content: value.content,
            apply_to_next_turn_only: value.apply_to_next_turn_only,
        }
    }
}

impl From<WireContextPatchRole> for ContextPatchRole {
    fn from(value: WireContextPatchRole) -> Self {
        match value {
            WireContextPatchRole::SystemAppend => Self::SystemAppend,
            WireContextPatchRole::UserPrefix => Self::UserPrefix,
            WireContextPatchRole::UserSuffix => Self::UserSuffix,
            WireContextPatchRole::AssistantHint => Self::AssistantHint,
        }
    }
}
