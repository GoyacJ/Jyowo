//! `jyowo-harness-context`
//!
//! Context assembly pipeline, providers, compaction, and prompt cache boundaries.
//!
//! Status: context assembly, patch sink, memory recall, and compaction runtime.

#![forbid(unsafe_code)]

pub mod buffer;
pub mod engine;
pub mod prompt;
pub mod provider;
pub mod stages;

pub use buffer::*;
pub use engine::*;
pub use harness_contracts::ContextStageId;
pub use prompt::*;
pub use provider::*;
pub use stages::*;
