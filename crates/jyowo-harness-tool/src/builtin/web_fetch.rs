use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, DecisionScope, HostRule, NetworkAccess, PermissionSubject, ToolActionPlan,
    ToolDescriptor, ToolError, ToolGroup, ToolResult, WorkspaceAccess,
};
use harness_permission::{DangerousPatternLibrary, PermissionCheck};
use serde_json::{json, Value};
use url::Url;

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

#[derive(Clone)]
pub struct WebFetchTool {
    descriptor: ToolDescriptor,
    backends: Vec<Arc<dyn WebFetchBackend>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFetchRequest {
    pub url: Url,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFetchResponse {
    pub url: Url,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
}

#[async_trait]
pub trait WebFetchBackend: Send + Sync + 'static {
    async fn fetch(&self, request: WebFetchRequest) -> Result<WebFetchResponse, ToolError>;
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "WebFetch",
                "Web fetch",
                "Fetch text content from an HTTP URL.",
                ToolGroup::Network,
                true,
                true,
                false,
                64_000,
                Vec::new(),
                super::object_schema(
                    &["url"],
                    json!({
                        "url": { "type": "string" },
                        "max_bytes": { "type": "integer", "minimum": 1 }
                    }),
                ),
            ),
            backends: Vec::new(),
        }
    }
}

impl WebFetchTool {
    pub fn new(backends: Vec<Arc<dyn WebFetchBackend>>) -> Self {
        Self {
            descriptor: Self::default().descriptor,
            backends,
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        url(input)?;
        max_bytes(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let parsed = url(input).ok();
        if let Some(parsed) = parsed.as_ref() {
            let library = DangerousPatternLibrary::default_all();
            if let Some(rule) = library.detect_url(parsed.as_str()) {
                let host = parsed.host_str().unwrap_or_default().to_owned();
                let port = parsed.port();
                return action_plan_from_permission_check(
                    &self.descriptor,
                    input,
                    ctx,
                    PermissionCheck::DangerousPattern {
                        kind: "url".to_owned(),
                        pattern: rule.id.clone(),
                        severity: rule.severity,
                        subject: PermissionSubject::NetworkAccess {
                            host: host.clone(),
                            port,
                        },
                        scope: DecisionScope::Category("network".to_owned()),
                    },
                    vec![ActionResource::Network {
                        host: host.clone(),
                        port: parsed.port(),
                    }],
                    WorkspaceAccess::None,
                    network_allow_list(host, port),
                );
            }
        }
        let host = parsed
            .as_ref()
            .and_then(Url::host_str)
            .unwrap_or_default()
            .to_owned();
        let port = parsed.as_ref().and_then(Url::port);
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::NetworkAccess {
                    host: host.clone(),
                    port,
                },
                scope: DecisionScope::Category("network".to_owned()),
            },
            vec![ActionResource::Network {
                host: host.clone(),
                port,
            }],
            WorkspaceAccess::None,
            network_allow_list(host, port),
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input();
        let url = url(input).map_err(validation_error)?;
        let max_bytes = max_bytes(input).map_err(validation_error)?;
        let backend = self.backends.first().ok_or_else(|| {
            ToolError::CapabilityMissing(harness_contracts::ToolCapability::Custom(
                "web_fetch_backend".to_owned(),
            ))
        })?;
        let response = backend.fetch(WebFetchRequest { url, max_bytes }).await?;
        let status = response.status;
        let final_url = response.url.to_string();
        let content_type = response.content_type;
        let mut body = response.body;
        let truncated = body.len() > max_bytes;
        if truncated {
            body = take_bytes_on_char_boundary(&body, max_bytes);
        }

        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "url": final_url,
                "status": status,
                "content_type": content_type,
                "body": body,
                "truncated": truncated
            })),
        )])))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn url(input: &Value) -> Result<Url, ValidationError> {
    let raw = input
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("url is required"))?;
    let parsed = Url::parse(raw).map_err(|error| ValidationError::from(error.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        _ => Err(ValidationError::from("url must use http or https")),
    }
}

fn max_bytes(input: &Value) -> Result<usize, ValidationError> {
    let Some(value) = input.get("max_bytes") else {
        return Ok(64_000);
    };
    let raw = value
        .as_u64()
        .ok_or_else(|| ValidationError::from("max_bytes must be a positive integer"))?;
    if raw == 0 {
        return Err(ValidationError::from("max_bytes must be greater than 0"));
    }
    usize::try_from(raw).map_err(|_| ValidationError::from("max_bytes must fit in usize"))
}

fn network_allow_list(host: String, port: Option<u16>) -> NetworkAccess {
    NetworkAccess::AllowList(vec![HostRule {
        pattern: host,
        ports: port.map(|port| vec![port]),
    }])
}

fn take_bytes_on_char_boundary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
