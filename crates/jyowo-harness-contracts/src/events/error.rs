use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineFailedEvent {
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub error: EngineError,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnexpectedErrorEvent {
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub error: String,
    pub at: DateTime<Utc>,
}
