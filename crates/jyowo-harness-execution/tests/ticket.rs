use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    ActionPlanHash, AuthorizationTicketId, RunId, SessionId, TenantId, ToolUseId,
};
use harness_execution::{AuthorizationTicketClaims, ExecutionError, TicketLedger};

#[test]
fn ticket_ledger_consumes_ticket_exactly_once() {
    let ledger = TicketLedger::new(Duration::from_secs(60));
    let claims = ticket_claims();
    let ticket = ledger.mint(claims.clone(), Utc::now()).unwrap();

    let consumed = ledger.consume(ticket.id, &claims, Utc::now()).unwrap();

    assert_eq!(consumed.ticket_id(), ticket.id);
    assert!(consumed.verify_authority(&ledger.authority_key()));
    assert!(matches!(
        ledger.consume(ticket.id, &claims, Utc::now()).unwrap_err(),
        ExecutionError::TicketConsumed { .. }
    ));
}

#[test]
fn ticket_ledger_rejects_unknown_expired_and_mismatched_tickets() {
    let ledger = TicketLedger::new(Duration::from_secs(1));
    let claims = ticket_claims();
    let ticket = ledger.mint(claims.clone(), Utc::now()).unwrap();

    assert!(matches!(
        ledger
            .consume(AuthorizationTicketId::new(), &claims, Utc::now())
            .unwrap_err(),
        ExecutionError::TicketUnknown { .. }
    ));
    for mismatched in mismatched_claims(&claims) {
        assert!(matches!(
            ledger
                .consume(ticket.id, &mismatched, Utc::now())
                .unwrap_err(),
            ExecutionError::TicketScopeMismatch { .. }
        ));
    }
    assert!(matches!(
        ledger
            .consume(
                ticket.id,
                &claims,
                ticket.expires_at + Duration::from_secs(1)
            )
            .unwrap_err(),
        ExecutionError::TicketExpired { .. }
    ));
}

fn mismatched_claims(claims: &AuthorizationTicketClaims) -> Vec<AuthorizationTicketClaims> {
    let mut mismatches = Vec::new();

    let mut mismatch = claims.clone();
    mismatch.tenant_id = TenantId::SHARED;
    mismatches.push(mismatch);

    let mut mismatch = claims.clone();
    mismatch.session_id = SessionId::new();
    mismatches.push(mismatch);

    let mut mismatch = claims.clone();
    mismatch.run_id = RunId::new();
    mismatches.push(mismatch);

    let mut mismatch = claims.clone();
    mismatch.tool_use_id = ToolUseId::new();
    mismatches.push(mismatch);

    let mut mismatch = claims.clone();
    mismatch.tool_name = "different-tool".to_owned();
    mismatches.push(mismatch);

    let mut mismatch = claims.clone();
    mismatch.action_plan_hash = ActionPlanHash::from_bytes([9; 32]);
    mismatches.push(mismatch);

    mismatches
}

fn ticket_claims() -> AuthorizationTicketClaims {
    AuthorizationTicketClaims {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        action_plan_hash: ActionPlanHash::from_bytes([1; 32]),
    }
}
