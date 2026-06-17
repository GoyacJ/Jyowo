use harness_contracts::{CancelInitiator, EndReason};

use crate::InterruptCause;

#[must_use]
pub fn end_reason_for_interrupt(cause: InterruptCause) -> EndReason {
    match cause {
        InterruptCause::User => EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
        InterruptCause::Parent => EndReason::Cancelled {
            initiator: CancelInitiator::Parent,
        },
        InterruptCause::System { reason } => EndReason::Cancelled {
            initiator: CancelInitiator::System { reason },
        },
        InterruptCause::Timeout => EndReason::Interrupted,
        InterruptCause::Budget => EndReason::TokenBudgetExhausted,
    }
}
