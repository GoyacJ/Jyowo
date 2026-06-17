//! `jyowo-harness-mcp`
//!
//! MCP clients, transports, server adapter, OAuth, and elicitation.
//!
//! SPEC: docs/architecture/harness/crates/harness-mcp.md
//! Status: MCP client transports, registry injection, notifications, and server adapter.

#![forbid(unsafe_code)]

pub mod client;
mod client_auth;
pub mod elicitation;
pub mod error;
pub mod jsonrpc;
pub mod metrics;
#[cfg(feature = "oauth")]
pub mod oauth;
pub mod reconnect;
pub mod registry;
pub mod sampling;
#[cfg(feature = "server-adapter")]
pub mod server;
pub mod transport;
pub mod transports;
pub mod types;
pub mod wrapper;

pub use client::*;
pub use elicitation::*;
pub use error::*;
pub use jsonrpc::*;
pub use metrics::*;
#[cfg(feature = "oauth")]
pub use oauth::*;
pub use reconnect::*;
pub use registry::*;
pub use sampling::*;
#[cfg(feature = "server-adapter")]
pub use server::*;
pub use transport::*;
#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse",
    feature = "in-process"
))]
pub use transports::*;
pub use types::*;
pub use wrapper::*;
