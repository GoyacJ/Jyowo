//! `jyowo-harness-engine`
//!
//! Single-agent loop, interruption, iteration budgets, and grace calls.
//!
//! SPEC: docs/architecture/harness/crates/harness-engine.md
//! Status: M5 engine runner and main loop.

#![forbid(unsafe_code)]

pub(crate) mod capability_assembly;
pub mod end_reason;
pub mod engine;
pub mod interrupt;
pub mod result_inject;
pub mod runner;
pub mod state;
pub mod turn;
pub mod turn_assembly;

pub use end_reason::*;
pub use engine::*;
pub use interrupt::*;
pub use runner::*;
pub use state::*;

#[cfg(feature = "subagent-tool")]
pub use engine::EngineBoundSubagentFactory;
