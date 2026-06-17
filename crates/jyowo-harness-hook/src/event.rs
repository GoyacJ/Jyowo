use std::path::PathBuf;

use bytes::Bytes;
use harness_contracts::{
    CacheImpact, EndReason, HookEventKind, McpServerId, RequestId, RunId, SessionId, SubagentId,
    SubagentStatus, ToolLoadingBackendName, ToolName, ToolResult, ToolSearchQueryKind, ToolUseId,
    UsageSnapshot,
};
use serde_json::Value;

use crate::{ModelRequestView, NotificationKind, SubagentSpecView, ToolErrorView};

#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    UserPromptSubmit {
        run_id: RunId,
        input: Value,
    },
    PreToolUse {
        tool_use_id: ToolUseId,
        tool_name: String,
        input: Value,
    },
    PostToolUse {
        tool_use_id: ToolUseId,
        result: ToolResult,
    },
    PostToolUseFailure {
        tool_use_id: ToolUseId,
        error: ToolErrorView,
    },
    PermissionRequest {
        request_id: RequestId,
        subject: String,
        detail: Option<String>,
    },
    SessionStart {
        session_id: SessionId,
    },
    Setup {
        workspace_root: Option<PathBuf>,
    },
    SessionEnd {
        session_id: SessionId,
        reason: EndReason,
    },
    SubagentStart {
        subagent_id: SubagentId,
        spec: SubagentSpecView,
    },
    SubagentStop {
        subagent_id: SubagentId,
        status: SubagentStatus,
    },
    Notification {
        kind: NotificationKind,
        body: Value,
    },
    PreLlmCall {
        run_id: RunId,
        request_view: ModelRequestView,
    },
    PostLlmCall {
        run_id: RunId,
        usage: UsageSnapshot,
    },
    PreApiRequest {
        request_id: RequestId,
        endpoint: String,
    },
    PostApiRequest {
        request_id: RequestId,
        status: u16,
    },
    TransformToolResult {
        tool_use_id: ToolUseId,
        result: ToolResult,
    },
    TransformTerminalOutput {
        tool_use_id: ToolUseId,
        raw: Bytes,
    },
    Elicitation {
        mcp_server_id: McpServerId,
        schema: Value,
    },
    PreToolSearch {
        tool_use_id: ToolUseId,
        query: String,
        query_kind: ToolSearchQueryKind,
    },
    PostToolSearchMaterialize {
        tool_use_id: ToolUseId,
        materialized: Vec<ToolName>,
        backend: ToolLoadingBackendName,
        cache_impact: CacheImpact,
    },
}

impl HookEvent {
    pub fn kind(&self) -> HookEventKind {
        match self {
            Self::UserPromptSubmit { .. } => HookEventKind::UserPromptSubmit,
            Self::PreToolUse { .. } => HookEventKind::PreToolUse,
            Self::PostToolUse { .. } => HookEventKind::PostToolUse,
            Self::PostToolUseFailure { .. } => HookEventKind::PostToolUseFailure,
            Self::PermissionRequest { .. } => HookEventKind::PermissionRequest,
            Self::SessionStart { .. } => HookEventKind::SessionStart,
            Self::Setup { .. } => HookEventKind::Setup,
            Self::SessionEnd { .. } => HookEventKind::SessionEnd,
            Self::SubagentStart { .. } => HookEventKind::SubagentStart,
            Self::SubagentStop { .. } => HookEventKind::SubagentStop,
            Self::Notification { .. } => HookEventKind::Notification,
            Self::PreLlmCall { .. } => HookEventKind::PreLlmCall,
            Self::PostLlmCall { .. } => HookEventKind::PostLlmCall,
            Self::PreApiRequest { .. } => HookEventKind::PreApiRequest,
            Self::PostApiRequest { .. } => HookEventKind::PostApiRequest,
            Self::TransformToolResult { .. } => HookEventKind::TransformToolResult,
            Self::TransformTerminalOutput { .. } => HookEventKind::TransformTerminalOutput,
            Self::Elicitation { .. } => HookEventKind::Elicitation,
            Self::PreToolSearch { .. } => HookEventKind::PreToolSearch,
            Self::PostToolSearchMaterialize { .. } => HookEventKind::PostToolSearchMaterialize,
        }
    }
}
