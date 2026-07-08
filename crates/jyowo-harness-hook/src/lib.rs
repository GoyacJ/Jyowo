//! `jyowo-harness-hook`
//!
//! Hook registry, dispatcher, transports, failure modes, and transaction semantics.
//!
//! Status: M3-T10 hook contract and replay coverage.

#![forbid(unsafe_code)]

pub mod context;
pub mod dispatcher;
pub mod event;
pub mod handler;
pub mod outcome;
pub mod registry;
pub mod transport;
pub mod views;

pub use context::*;
pub use dispatcher::*;
pub use event::*;
pub use handler::*;
pub use outcome::*;
pub use registry::*;
pub use transport::*;
pub use views::*;

pub use harness_contracts::{
    HookEventKind, HookFailureMode, HookOutcomeDiscriminant, InconsistentReason,
};
