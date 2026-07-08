//! `jyowo-harness-journal`
//!
//! Event store, snapshots, JSONL/SQLite adapters, and blob metadata.
//!
//! Status: M2 L1-B `EventStore` + builtin store implementations.

#![forbid(unsafe_code)]

pub mod audit;
pub mod blob;
#[cfg(feature = "sqlite")]
pub mod conversation_read_model;
pub mod conversation_worktree_projector;
pub mod evidence;
#[cfg(feature = "jsonl")]
pub mod jsonl;
#[cfg(any(test, feature = "in-memory", feature = "testing"))]
pub mod memory;
pub mod projection;
pub mod retention;
pub mod snapshot;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod store;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod version;

pub use audit::*;
pub use blob::*;
#[cfg(feature = "sqlite")]
pub use conversation_read_model::*;
pub use conversation_worktree_projector::*;
pub use evidence::*;
#[cfg(feature = "jsonl")]
pub use jsonl::*;
#[cfg(any(test, feature = "in-memory", feature = "testing"))]
pub use memory::*;
pub use projection::*;
pub use retention::*;
pub use snapshot::*;
#[cfg(feature = "sqlite")]
pub use sqlite::*;
pub use store::*;
#[cfg(any(test, feature = "testing"))]
pub use testing::*;
pub use version::*;

pub(crate) fn app_controlled_path(
    path: &std::path::Path,
) -> Result<std::path::PathBuf, harness_fs::FsError> {
    let Some(parent) = path.parent() else {
        return harness_fs::resolve_canonical_prefix(path);
    };
    let Some(file_name) = path.file_name() else {
        return harness_fs::resolve_canonical_prefix(path);
    };
    Ok(harness_fs::resolve_canonical_prefix(parent)?.join(file_name))
}
