//! `jyowo-harness-tool`
//!
//! Tool traits, registry, execution pool, result budget, and built-in tools.
//!
//! Status: tool contracts, registry, orchestration, permissions, budgets, and builtins.

#![forbid(unsafe_code)]

pub mod builder;
#[cfg(any(feature = "builtin-toolset", feature = "skill-tools"))]
pub mod builtin;
pub mod context;
pub mod error;
pub mod network_broker;
pub mod orchestrator;
pub mod pool;
pub mod process_registry;
#[cfg(feature = "gemini-tools")]
mod provider_gemini;
pub mod provider_media;
#[cfg(feature = "minimax-tools")]
mod provider_minimax;
#[cfg(feature = "zhipu-tools")]
mod provider_zhipu;
pub mod registry;
pub mod result_budget;
pub mod skill_script;
pub mod tool;

pub use builder::*;
#[cfg(any(feature = "builtin-toolset", feature = "skill-tools"))]
pub use builtin::*;
pub use context::*;
pub use error::*;
pub use harness_contracts::ToolSearchMode;
pub use harness_permission::{
    canonical_permission_fingerprint, PermissionBroker, PermissionCheck, PermissionContext,
    PermissionRequest, PersistedDecision, RuleSnapshot,
};
pub use network_broker::*;
pub use orchestrator::*;
pub use pool::*;
pub use process_registry::*;
pub use registry::*;
pub use result_budget::*;
pub use skill_script::*;
pub use tool::*;
