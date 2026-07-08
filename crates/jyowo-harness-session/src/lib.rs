//! `jyowo-harness-session`
//!
//! Session lifecycle, projections, fork/reload behavior, and steering queue.
//!
//! Status: session lifecycle, projection, fork/reload, steering, and engine delegation.

#![forbid(unsafe_code)]

pub mod builder;
pub mod fork;
pub mod lifecycle;
pub mod paths;
pub mod projection;
#[cfg(feature = "hot-reload-fork")]
pub mod reload;
pub mod session;
pub mod snapshot;
#[cfg(feature = "steering")]
pub mod steering;
pub mod turn;
pub mod workspace;

pub use builder::*;
#[cfg(feature = "steering")]
pub use harness_contracts::SteeringRequest;
pub use paths::*;
pub use projection::*;
#[cfg(feature = "hot-reload-fork")]
pub use reload::*;
pub use session::*;
#[cfg(feature = "steering")]
pub use steering::*;
pub use turn::*;
pub use workspace::*;
