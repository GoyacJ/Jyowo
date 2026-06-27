use std::pin::Pin;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::Stream;
use harness_contracts::{Event, MessagePart, ToolDescriptor, ToolError, ToolResult};
use harness_permission::PermissionCheck;
use serde_json::Value;

use crate::{SchemaResolverContext, ToolContext, ValidationError};

pub type ToolStream = Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum ToolJournalAuthority {
    #[default]
    None,
    Clarification,
    Sandbox,
    ExecuteCode,
}

#[async_trait]
pub trait Tool: Send + Sync + 'static {
    fn descriptor(&self) -> &ToolDescriptor;

    fn input_schema(&self) -> &Value {
        &self.descriptor().input_schema
    }

    fn output_schema(&self) -> Option<&Value> {
        self.descriptor().output_schema.as_ref()
    }

    async fn resolve_schema(&self, _ctx: &SchemaResolverContext) -> Result<Value, ToolError> {
        Ok(self.input_schema().clone())
    }

    async fn validate(&self, input: &Value, ctx: &ToolContext) -> Result<(), ValidationError>;

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck;

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError>;
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ToolEvent {
    Progress(ToolProgress),
    Partial(MessagePart),
    Journal(Event),
    Final(ToolResult),
    Error(ToolError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolProgress {
    pub message: String,
    pub fraction: Option<f32>,
    pub at: DateTime<Utc>,
}

impl ToolProgress {
    pub fn now(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fraction: None,
            at: Utc::now(),
        }
    }

    pub fn with_fraction(message: impl Into<String>, fraction: f32) -> Self {
        Self {
            message: message.into(),
            fraction: Some(fraction),
            at: Utc::now(),
        }
    }
}
