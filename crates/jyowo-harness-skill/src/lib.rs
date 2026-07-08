//! `jyowo-harness-skill`
//!
//! Skill loading, source precedence, frontmatter, and skill tools.
//!
//! Status: M4 L2-SK implementation.

#![forbid(unsafe_code)]

pub mod error;
pub mod frontmatter;
pub mod loader;
pub mod prefetch;
pub mod registry;
pub mod rejection;
pub mod renderer;
#[cfg(feature = "threat-scanner")]
pub mod scanner;
pub mod service;
pub mod skill;
pub mod sources;

pub use harness_contracts::{
    SkillFilter, SkillInjectionId, SkillInvocationReceipt, SkillParameterInfo, SkillStatus,
    SkillSummary, SkillView,
};

pub use error::*;
pub use frontmatter::*;
pub use loader::*;
pub use prefetch::*;
pub use registry::*;
pub use rejection::*;
pub use renderer::*;
#[cfg(feature = "threat-scanner")]
pub use scanner::*;
pub use service::*;
pub use skill::*;
pub use sources::*;
