use chrono::{DateTime, Utc};
use harness_contracts::{AuthorizationTicketId, Decision, ToolUseId};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ExecutionError {
    #[error("permission denied for tool use {tool_use_id}: {decision:?}")]
    PermissionDenied {
        tool_use_id: ToolUseId,
        decision: Decision,
    },
    #[error("authorization ticket {ticket_id} is unknown")]
    TicketUnknown { ticket_id: AuthorizationTicketId },
    #[error("authorization ticket {ticket_id} expired at {expires_at}")]
    TicketExpired {
        ticket_id: AuthorizationTicketId,
        expires_at: DateTime<Utc>,
    },
    #[error("authorization ticket {ticket_id} was already consumed")]
    TicketConsumed { ticket_id: AuthorizationTicketId },
    #[error("authorization ticket {ticket_id} does not match the requested action")]
    TicketScopeMismatch { ticket_id: AuthorizationTicketId },
    #[error("sandbox preflight failed for backend {backend_id}: {reason}")]
    SandboxPreflightFailed { backend_id: String, reason: String },
    #[error("authorization failed: {reason}")]
    AuthorizationFailed { reason: String },
    #[error("authorization event sink failed: {reason}")]
    EventSinkFailed { reason: String },
}
