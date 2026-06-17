#[cfg(feature = "in-process")]
mod in_process;
#[cfg(any(feature = "exec", feature = "http"))]
mod protocol;

#[cfg(feature = "exec")]
mod exec;
#[cfg(feature = "http")]
mod http;

use async_trait::async_trait;
use harness_contracts::HookError;
use serde::{Deserialize, Serialize};

use crate::{HookContext, HookEvent, HookOutcome};

#[cfg(feature = "exec")]
pub use exec::*;
#[cfg(feature = "http")]
pub use http::*;

#[derive(Clone)]
pub struct HookPayload {
    pub event: HookEvent,
    pub ctx: HookContext,
}

pub type HookOutput = Result<HookOutcome, HookError>;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookProtocolVersion {
    V1,
}

#[async_trait]
pub trait HookTransport: Send + Sync + 'static {
    async fn invoke(&self, payload: HookPayload) -> HookOutput;
}

#[cfg(feature = "in-process")]
pub use in_process::*;
