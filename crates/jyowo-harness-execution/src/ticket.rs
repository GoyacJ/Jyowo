use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use harness_contracts::{
    ActionPlanHash, AuthorizationTicketId, RunId, SessionId, TenantId, ToolUseId,
};
use parking_lot::Mutex;

use crate::ExecutionError;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthorizationTicketClaims {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub action_plan_hash: ActionPlanHash,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthorizationTicket {
    pub id: AuthorizationTicketId,
    pub claims: AuthorizationTicketClaims,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
struct TicketRecord {
    ticket: AuthorizationTicket,
    consumed: bool,
}

#[derive(Debug)]
pub struct TicketLedger {
    ttl: Duration,
    tickets: Mutex<HashMap<AuthorizationTicketId, TicketRecord>>,
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
            ttl,
            tickets: Mutex::new(HashMap::new()),
        }
    }

    pub fn mint(
        &self,
        claims: AuthorizationTicketClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthorizationTicket, ExecutionError> {
        let ttl = chrono::Duration::from_std(self.ttl).map_err(|error| {
            ExecutionError::EventSinkFailed {
                reason: format!("invalid ticket ttl: {error}"),
            }
        })?;
        let ticket = AuthorizationTicket {
            id: AuthorizationTicketId::new(),
            claims,
            issued_at: now,
            expires_at: now + ttl,
        };
        self.tickets.lock().insert(
            ticket.id,
            TicketRecord {
                ticket: ticket.clone(),
                consumed: false,
            },
        );
        Ok(ticket)
    }

    pub fn consume(
        &self,
        ticket_id: AuthorizationTicketId,
        claims: &AuthorizationTicketClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthorizationTicket, ExecutionError> {
        let mut tickets = self.tickets.lock();
        let Some(record) = tickets.get_mut(&ticket_id) else {
            return Err(ExecutionError::TicketUnknown { ticket_id });
        };
        if record.consumed {
            return Err(ExecutionError::TicketConsumed { ticket_id });
        }
        if now > record.ticket.expires_at {
            return Err(ExecutionError::TicketExpired {
                ticket_id,
                expires_at: record.ticket.expires_at,
            });
        }
        if &record.ticket.claims != claims {
            return Err(ExecutionError::TicketScopeMismatch { ticket_id });
        }

        record.consumed = true;
        Ok(record.ticket.clone())
    }

    pub fn revoke(&self, ticket_id: AuthorizationTicketId) {
        self.tickets.lock().remove(&ticket_id);
    }
}
