//! HTTP transport facade.
//!
//! Streamable HTTP owns the standard MCP lifecycle. Deprecated HTTP+SSE
//! fallback is attached here by the legacy transport compatibility layer.

pub use super::streamable_http::HttpTransport;
