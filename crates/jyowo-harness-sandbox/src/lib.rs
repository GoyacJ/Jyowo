//! `jyowo-harness-sandbox`
//!
//! Sandbox backend contracts and built-in local/noop backends.
//!
//! SPEC: docs/architecture/harness/crates/harness-sandbox.md
//! Status: M2 L1-C sandbox lane.

#![forbid(unsafe_code)]

pub mod backend;
#[cfg(feature = "code-runtime")]
pub mod code_sandbox;
pub mod cwd;
#[cfg(feature = "docker")]
pub mod docker;
#[cfg(feature = "local")]
pub mod local;
#[cfg(feature = "noop")]
pub mod noop;
pub mod policy;
#[cfg(any(feature = "docker", feature = "ssh"))]
mod process;
pub mod routing;
pub mod skill_script;
#[cfg(feature = "ssh")]
pub mod ssh;
pub mod wrapped_command;

pub use backend::*;
#[cfg(feature = "code-runtime")]
pub use code_sandbox::*;
pub use cwd::*;
#[cfg(feature = "docker")]
pub use docker::*;
#[cfg(feature = "local")]
pub use local::*;
#[cfg(feature = "noop")]
pub use noop::*;
pub use policy::*;
pub use routing::*;
pub use skill_script::*;
#[cfg(feature = "ssh")]
pub use ssh::*;
pub use wrapped_command::*;
