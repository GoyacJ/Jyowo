use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, BudgetMetric, DecisionScope, HostRule, NetworkAccess, PermissionSubject,
    ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolResult, WorkspaceAccess,
};
use harness_permission::{DangerousPatternLibrary, PermissionCheck};
use serde_json::{json, Value};
use url::Url;

use crate::{
    action_plan_from_permission_check, AuthorizedNetworkPermit, AuthorizedToolInput, HttpMethod,
    Tool, ToolContext, ToolEvent, ToolHttpJsonRequest, ToolNetworkBrokerCap, ToolStream,
    ValidationError,
};

#[derive(Clone)]
pub struct WebFetchTool {
    descriptor: ToolDescriptor,
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
        }
    }
}

impl WebFetchTool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
                let port = parsed.port_or_known_default();
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
                        port,
                    }],
                    WorkspaceAccess::None,
                    network_allow_list(host, port),
                    ToolExecutionChannel::HttpBroker,
                );
            }
        }
        let host = parsed
            .as_ref()
            .and_then(Url::host_str)
            .unwrap_or_default()
            .to_owned();
        let port = parsed.as_ref().and_then(Url::port_or_known_default);
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
            ToolExecutionChannel::HttpBroker,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input();
        let url = url(input).map_err(validation_error)?;
        let max_bytes = max_bytes(input).map_err(validation_error)?;

        let permit = authorized.network_permit()?;
        let broker = ctx.capability::<dyn ToolNetworkBrokerCap>(ToolCapability::NetworkBroker)?;
        brokered_web_fetch(broker, permit, url, max_bytes).await
    }
}

async fn brokered_web_fetch(
    broker: Arc<dyn ToolNetworkBrokerCap>,
    permit: AuthorizedNetworkPermit,
    url: Url,
    max_bytes: usize,
) -> Result<ToolStream, ToolError> {
    let url_str = url.to_string();
    let max_bytes_u64 = max_bytes as u64;
    let req = ToolHttpJsonRequest {
        method: HttpMethod::Get,
        url: url_str.clone(),
        headers: BTreeMap::new(),
        body: None,
        timeout: Duration::from_secs(30),
        max_response_bytes: max_bytes_u64.min(10 * 1024 * 1024),
    };
    let resp = broker.execute_json(&permit, req).await?;
    if resp.body.len() > max_bytes {
        return Err(ToolError::ResultTooLarge {
            original: resp.body.len() as u64,
            limit: max_bytes_u64,
            metric: BudgetMetric::Bytes,
        });
    }
    let body_str = String::from_utf8_lossy(&resp.body).into_owned();

    Ok(Box::pin(stream::iter([ToolEvent::Final(
        ToolResult::Structured(json!({
            "url": url_str,
            "status": resp.status,
            "content_type": resp.headers.get("content-type").cloned(),
            "body": body_str,
            "truncated": false
        })),
    )])))
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
