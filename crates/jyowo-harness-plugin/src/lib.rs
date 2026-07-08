//! `jyowo-harness-plugin`
//!
//! Plugin manifests, runtime loading, signer trust, and capability handles.
//!
//! Status: M5 plugin implementation.

#![forbid(unsafe_code)]

pub mod capability;
pub mod cargo_extension;
#[cfg(feature = "dynamic-load")]
pub mod dynamic_load;
pub mod error;
pub mod loader;
pub mod manifest;
pub mod plugin;
pub mod registry;
pub mod signer;
pub mod sources;
#[cfg(feature = "wasm-runtime")]
pub mod wasm_runtime;

pub use capability::*;
pub use cargo_extension::*;
#[cfg(feature = "dynamic-load")]
pub use dynamic_load::*;
pub use error::*;
pub use loader::*;
pub use manifest::*;
pub use plugin::*;
pub use registry::*;
pub use signer::*;
pub use sources::*;
#[cfg(feature = "wasm-runtime")]
pub use wasm_runtime::*;
