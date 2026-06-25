//! `jyowo-harness-journal`
//!
//! Event store, snapshots, JSONL/SQLite adapters, and blob metadata.
//!
//! SPEC: docs/architecture/harness/crates/harness-journal.md
//! Status: M2 L1-B `EventStore` + builtin store implementations.

#![forbid(unsafe_code)]

pub mod audit;
pub mod blob;
#[cfg(feature = "sqlite")]
pub mod conversation_read_model;
pub mod conversation_worktree_projector;
#[cfg(feature = "jsonl")]
pub mod jsonl;
#[cfg(any(test, feature = "in-memory", feature = "mock"))]
pub mod memory;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod projection;
pub mod retention;
pub mod snapshot;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod store;
pub mod version;

pub use audit::*;
pub use blob::*;
#[cfg(feature = "sqlite")]
pub use conversation_read_model::*;
pub use conversation_worktree_projector::*;
#[cfg(feature = "jsonl")]
pub use jsonl::*;
#[cfg(any(test, feature = "in-memory", feature = "mock"))]
pub use memory::*;
#[cfg(any(test, feature = "mock"))]
pub use mock::*;
pub use projection::*;
pub use retention::*;
pub use snapshot::*;
#[cfg(feature = "sqlite")]
pub use sqlite::*;
pub use store::*;
pub use version::*;
