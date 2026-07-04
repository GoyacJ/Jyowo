//! `jyowo-harness-memory`
//!
//! Memory store and lifecycle primitives.
//!
//! SPEC: docs/architecture/harness/crates/harness-memory.md
//! Status: memory providers, stores, lifecycle, recall, and write-back contracts.

#![forbid(unsafe_code)]

pub use harness_contracts::MemdirFileTag as MemdirFile;

#[cfg(feature = "provider-registry")]
pub mod external;
#[cfg(feature = "provider-registry")]
pub mod in_memory;
pub mod lifecycle;
pub mod local;
#[cfg(feature = "builtin")]
pub mod memdir;
pub mod registry;
#[cfg(feature = "threat-scanner")]
pub mod scanner;
pub mod store;
pub mod types;

#[cfg(feature = "provider-registry")]
pub use external::*;
#[cfg(feature = "provider-registry")]
pub use in_memory::*;
pub use lifecycle::*;
pub use local::*;
#[cfg(feature = "builtin")]
pub use memdir::*;
pub use registry::*;
#[cfg(feature = "threat-scanner")]
pub use scanner::*;
pub use store::*;
pub use types::*;
