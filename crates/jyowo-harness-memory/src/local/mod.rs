//! Local SQLite memory provider.
//!
//! Production default provider. Uses SQLite with FTS5 for lexical search,
//! with optional embedding vector storage for semantic retrieval.

pub mod embedding;
pub mod provider;
pub mod ranking;
pub mod schema;
pub mod schema_init;

pub use embedding::MemoryEmbeddingProvider;
pub use provider::{LocalMemoryOptions, LocalMemoryProvider};
