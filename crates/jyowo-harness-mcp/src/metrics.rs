use harness_contracts::{McpResourceUpdateKind, McpServerId, ToolsListChangedDisposition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMetricOutcome {
    Success,
    Error,
    Denied,
    Deferred,
    Cancelled,
    Throttled,
}

impl McpMetricOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::Denied => "denied",
            Self::Deferred => "deferred",
            Self::Cancelled => "cancelled",
            Self::Throttled => "throttled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMetricConnectionState {
    Connecting,
    Ready,
    Reconnecting,
    Failed,
    Closed,
}

impl McpMetricConnectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::Reconnecting => "reconnecting",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpMetric {
    OAuthRefresh {
        outcome: McpMetricOutcome,
    },
    ConnectionTotal {
        server_id: McpServerId,
        transport: String,
        outcome: McpMetricOutcome,
    },
    ConnectionState {
        server_id: McpServerId,
        state: McpMetricConnectionState,
    },
    ReconnectAttempt {
        server_id: McpServerId,
        attempt: u32,
        outcome: McpMetricOutcome,
    },
    ToolInvocation {
        server_id: McpServerId,
        outcome: McpMetricOutcome,
    },
    ToolFilterSkipped {
        server_id: McpServerId,
        reason: &'static str,
    },
    ListChanged {
        server_id: McpServerId,
        disposition: ToolsListChangedDisposition,
    },
    ResourceUpdated {
        server_id: McpServerId,
        kind: McpResourceUpdateKind,
    },
    SamplingRequested {
        outcome: McpMetricOutcome,
    },
    SamplingInputTokens {
        server_id: McpServerId,
        amount: u64,
    },
    SamplingOutputTokens {
        server_id: McpServerId,
        amount: u64,
    },
    ServerRequest {
        method: &'static str,
        outcome: McpMetricOutcome,
    },
    ServerThrottled {
        capability: &'static str,
    },
    ServerTenantIsolationRejected,
}

impl McpMetric {
    pub fn name(&self) -> &'static str {
        match self {
            Self::OAuthRefresh { .. } => "mcp_oauth_refresh_total",
            Self::ConnectionTotal { .. } => "mcp_connection_total",
            Self::ConnectionState { .. } => "mcp_connection_state",
            Self::ReconnectAttempt { .. } => "mcp_reconnect_attempts_total",
            Self::ToolInvocation { .. } => "mcp_tool_invocations_total",
            Self::ToolFilterSkipped { .. } => "mcp_tool_filter_skipped_total",
            Self::ListChanged { .. } => "mcp_list_changed_total",
            Self::ResourceUpdated { .. } => "mcp_resource_updated_total",
            Self::SamplingRequested { .. } => "mcp_sampling_requested_total",
            Self::SamplingInputTokens { .. } => "mcp_sampling_input_tokens_sum",
            Self::SamplingOutputTokens { .. } => "mcp_sampling_output_tokens_sum",
            Self::ServerRequest { .. } => "mcp_server_requests_total",
            Self::ServerThrottled { .. } => "mcp_server_throttled_total",
            Self::ServerTenantIsolationRejected => "mcp_server_tenant_isolation_rejected_total",
        }
    }
}

pub trait McpMetricsSink: Send + Sync + 'static {
    fn record(&self, metric: McpMetric);
}

#[derive(Debug, Default)]
pub struct NoopMcpMetricsSink;

impl McpMetricsSink for NoopMcpMetricsSink {
    fn record(&self, _metric: McpMetric) {}
}
