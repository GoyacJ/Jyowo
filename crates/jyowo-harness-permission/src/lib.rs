//! `jyowo-harness-permission`
//!
//! Permission brokers, rule providers, and decision handling.
//!
//! SPEC: docs/architecture/harness/crates/harness-permission.md
//! Status: permission brokers, rule checks, persistence, and fingerprints.

#![forbid(unsafe_code)]

#[cfg(feature = "auto-mode")]
pub mod aux_llm;
pub mod broker;
pub mod chain;
#[cfg(feature = "dangerous")]
pub mod dangerous;
pub mod decision;
pub mod dedup;
#[cfg(feature = "interactive")]
pub mod direct;
#[cfg(feature = "integrity")]
pub mod integrity_signer;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod persistence;
#[cfg(feature = "rule-engine")]
pub mod providers;
pub mod rule;
#[cfg(feature = "rule-engine")]
pub mod rule_engine;
#[cfg(feature = "stream")]
pub mod stream;

#[cfg(feature = "auto-mode")]
pub use aux_llm::*;
pub use broker::*;
pub use chain::*;
#[cfg(feature = "dangerous")]
pub use dangerous::*;
pub use decision::*;
pub use dedup::*;
#[cfg(feature = "interactive")]
pub use direct::*;
#[cfg(feature = "integrity")]
pub use integrity_signer::*;
#[cfg(any(test, feature = "mock"))]
pub use mock::*;
#[cfg(feature = "integrity")]
pub use persistence::{
    FileDecisionPersistence, NoopPermissionTamperEventSink, PermissionTamperEventSink,
};
#[cfg(feature = "rule-engine")]
pub use providers::*;
pub use rule::*;
#[cfg(feature = "rule-engine")]
pub use rule_engine::*;
#[cfg(feature = "stream")]
pub use stream::*;
