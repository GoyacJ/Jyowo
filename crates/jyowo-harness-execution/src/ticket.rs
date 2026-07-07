use std::time::Duration;

use chrono::{DateTime, Utc};
use harness_contracts::AuthorizationTicketId;
use harness_tool::{AuthorizedTicketSummary, TicketLedgerError};

use crate::ExecutionError;

pub use harness_tool::{AuthorizationTicket, AuthorizationTicketClaims, AuthorizationTicketKey};

#[derive(Debug)]
pub struct TicketLedger {
    inner: harness_tool::TicketLedger,
}

impl Default for TicketLedger {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

impl TicketLedger {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: harness_tool::TicketLedger::new(ttl),
        }
    }

    #[must_use]
    pub fn with_authority_key(ttl: Duration, authority_key: AuthorizationTicketKey) -> Self {
        Self {
            inner: harness_tool::TicketLedger::with_authority_key(ttl, authority_key),
        }
    }

    #[must_use]
    pub fn authority_key(&self) -> AuthorizationTicketKey {
        self.inner.authority_key()
    }

    pub fn mint(
        &self,
        claims: AuthorizationTicketClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthorizationTicket, ExecutionError> {
        self.inner.mint(claims, now).map_err(ExecutionError::from)
    }

    pub fn consume(
        &self,
        ticket_id: AuthorizationTicketId,
        claims: &AuthorizationTicketClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthorizedTicketSummary, ExecutionError> {
        self.inner
            .consume(ticket_id, claims, now)
            .map_err(ExecutionError::from)
    }

    pub fn revoke(&self, ticket_id: AuthorizationTicketId) {
        self.inner.revoke(ticket_id);
    }
}

impl From<TicketLedgerError> for ExecutionError {
    fn from(error: TicketLedgerError) -> Self {
        match error {
            TicketLedgerError::InvalidTtl { reason } => Self::EventSinkFailed {
                reason: format!("invalid ticket ttl: {reason}"),
            },
            TicketLedgerError::Unknown { ticket_id } => Self::TicketUnknown { ticket_id },
            TicketLedgerError::Expired {
                ticket_id,
                expires_at,
            } => Self::TicketExpired {
                ticket_id,
                expires_at,
            },
            TicketLedgerError::Consumed { ticket_id } => Self::TicketConsumed { ticket_id },
            TicketLedgerError::ScopeMismatch { ticket_id } => {
                Self::TicketScopeMismatch { ticket_id }
            }
        }
    }
}
