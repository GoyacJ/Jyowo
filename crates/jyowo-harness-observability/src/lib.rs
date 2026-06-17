//! `jyowo-harness-observability`
//!
//! Tracing, usage accounting, replay, and redaction.
//!
//! SPEC: docs/architecture/harness/crates/harness-observability.md
//! Status: M5 observability implementation.

#![forbid(unsafe_code)]

#[cfg(feature = "redactor")]
pub mod contract;
pub mod error;
pub mod observer;
#[cfg(feature = "otel")]
pub mod otel;
#[cfg(feature = "prometheus")]
pub mod prometheus;
#[cfg(feature = "redactor")]
pub mod redactor;
#[cfg(feature = "replay")]
pub mod replay;
pub mod tracer;
pub mod usage;

#[cfg(feature = "redactor")]
pub use contract::*;
pub use error::*;
pub use observer::*;
#[cfg(feature = "otel")]
pub use otel::*;
#[cfg(feature = "prometheus")]
pub use prometheus::*;
#[cfg(feature = "redactor")]
pub use redactor::*;
#[cfg(feature = "replay")]
pub use replay::*;
pub use tracer::*;
pub use usage::*;
