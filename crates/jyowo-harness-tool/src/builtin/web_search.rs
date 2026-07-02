use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, DecisionScope, HostRule, NetworkAccess, PermissionSubject, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolResult, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::TryFrom;

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSearchRequest {
    pub query: String,
    pub max_results: Option<u32>,
    pub region: Option<String>,
    pub recency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub score: f64,
}

#[async_trait]
pub trait WebSearchBackend: Send + Sync + 'static {
    async fn search(&self, request: WebSearchRequest) -> Result<Vec<WebSearchResult>, ToolError>;
}

#[derive(Clone)]
pub struct WebSearchTool {
    descriptor: ToolDescriptor,
    backends: Vec<Arc<dyn WebSearchBackend>>,
}

impl WebSearchTool {
    pub fn new(backends: Vec<Arc<dyn WebSearchBackend>>) -> Self {
        Self {
            descriptor: Self::default_descriptor(),
            backends,
        }
    }

    fn default_descriptor() -> ToolDescriptor {
        super::descriptor(
            "WebSearch",
            "Web search",
            "Search the web using a configured backend.",
            ToolGroup::Network,
            true,
            true,
            false,
            32_000,
            Vec::new(),
            super::object_schema(
                &["query"],
                json!({
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1 },
                    "region": { "type": "string", "minLength": 1 },
                    "recency": {
                        "type": "string",
                        "enum": ["day", "week", "month", "year"]
                    }
                }),
            ),
        )
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self {
            descriptor: Self::default_descriptor(),
            backends: Vec::new(),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::NetworkAccess {
                    host: "web-search".to_owned(),
                    port: None,
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            vec![ActionResource::Network {
                host: "web-search".to_owned(),
                port: None,
            }],
            WorkspaceAccess::None,
            NetworkAccess::AllowList(vec![HostRule {
                pattern: "web-search".to_owned(),
                ports: None,
            }]),
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let backend = self.backends.first().ok_or_else(|| {
            ToolError::CapabilityMissing(ToolCapability::Custom("web_search_backend".to_owned()))
        })?;
        let results = backend
            .search(request(authorized.raw_input()).map_err(validation_error)?)
            .await?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(
                serde_json::to_value(results)
                    .map_err(|error| ToolError::Message(error.to_string()))?,
            ),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn request(input: &Value) -> Result<WebSearchRequest, ValidationError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("query is required"))?
        .to_owned();
    let max_results = input.get("max_results").map(max_results).transpose()?;
    let region = optional_non_empty_string(input, "region", "region must be a non-empty string")?;
    let recency = optional_non_empty_string(
        input,
        "recency",
        "recency must be one of day, week, month, year",
    )?;
    if let Some(recency) = recency.as_deref() {
        if !matches!(recency, "day" | "week" | "month" | "year") {
            return Err(ValidationError::from(
                "recency must be one of day, week, month, year",
            ));
        }
    }
    Ok(WebSearchRequest {
        query,
        max_results,
        region,
        recency,
    })
}

fn optional_non_empty_string(
    input: &Value,
    field: &str,
    error: &str,
) -> Result<Option<String>, ValidationError> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    value
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .map(Some)
        .ok_or_else(|| ValidationError::from(error))
}

fn max_results(value: &Value) -> Result<u32, ValidationError> {
    let raw = value
        .as_u64()
        .ok_or_else(|| ValidationError::from("max_results must be a positive integer"))?;
    if raw == 0 {
        return Err(ValidationError::from("max_results must be greater than 0"));
    }
    u32::try_from(raw).map_err(|_| ValidationError::from("max_results must fit in u32"))
}
