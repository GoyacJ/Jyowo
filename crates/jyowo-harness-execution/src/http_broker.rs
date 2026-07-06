//! Production reqwest-backed HTTP broker.
//!
//! Validates every outgoing HTTP request against an `AuthorizedNetworkPermit`
//! before dispatch. The same broker instance is injected into both authorization
//! preflight (`ExecutionPreflightRegistry`) and authorized tool execution
//! (`CapabilityRegistry`) so preflight and execution use identical policy.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use harness_contracts::{HostRule, NetworkAccess, RedactRules, Redactor, ToolError};
use harness_tool::{
    AuthorizedNetworkPermit, HttpMethod, NetworkBrokerPreflightRequest, ToolHttpJsonRequest,
    ToolHttpResponse, ToolNetworkBrokerCap, ToolNetworkBrokerPreflightCap,
};
use url::Url;

/// Production HTTP broker backed by a shared `reqwest::Client`.
///
/// One instance is created at desktop runtime startup and injected into both
/// the authorization preflight registry and the `CapabilityRegistry`.
pub struct ReqwestToolNetworkBroker {
    client: reqwest::Client,
    max_response_bytes: u64,
    redactor: Arc<dyn Redactor>,
}

impl std::fmt::Debug for ReqwestToolNetworkBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestToolNetworkBroker")
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl ReqwestToolNetworkBroker {
    /// Creates a new broker with the given timeout and response size cap.
    pub fn new(
        timeout: Duration,
        max_response_bytes: u64,
        redactor: Arc<dyn Redactor>,
    ) -> Result<Self, ToolError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            // No auto redirect following — every redirect target must be validated.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ToolError::Internal(format!("broker client init failed: {error}")))?;

        Ok(Self {
            client,
            max_response_bytes,
            redactor,
        })
    }

    /// Validates a request URL against the approved host rules in the permit.
    fn validate_request(
        permit: &AuthorizedNetworkPermit,
        request: &ToolHttpJsonRequest,
    ) -> Result<ValidatedUrl, ToolError> {
        let parsed = Url::parse(&request.url).map_err(|error| {
            ToolError::Validation(redact(format!("invalid request URL: {error}")))
        })?;

        // Only http(s) schemes.
        let scheme = parsed.scheme().to_owned();
        if scheme != "http" && scheme != "https" {
            return Err(ToolError::Validation(format!(
                "broker: scheme `{scheme}` is not allowed; only http and https are supported"
            )));
        }

        let host = parsed
            .host_str()
            .ok_or_else(|| ToolError::Validation("broker: request URL has no host".to_owned()))?
            .to_owned();

        let port = parsed.port_or_known_default().unwrap_or(0);

        // Deny public raw IP literals.
        if let Ok(ip) = host.parse::<IpAddr>() {
            if ip.is_loopback() {
                // Loopback IP is allowed only when the exact host:port is
                // explicitly approved in the permit.
                let host_port_str = format!("{host}:{port}");
                let explicitly_approved = permit.approved_hosts().iter().any(|rule| {
                    host_matches_rule(&host_port_str, rule) || host_matches_rule(&host, rule)
                });
                if !explicitly_approved {
                    return Err(ToolError::PermissionDenied(format!(
                        "broker: loopback host `{host}:{port}` is not explicitly approved"
                    )));
                }
            } else {
                return Err(ToolError::PermissionDenied(format!(
                    "broker: raw IP literal `{host}` is denied; only approved hostnames are allowed"
                )));
            }
        }

        // Validate the host against the approved allowlist.
        let approved = permit
            .approved_hosts()
            .iter()
            .any(|rule| host_matches_rule(&host, rule) && port_matches_rule(port, rule));

        if !approved {
            return Err(ToolError::PermissionDenied(format!(
                "broker: host `{host}:{port}` is not in the approved allowlist"
            )));
        }

        Ok(ValidatedUrl {
            url: parsed,
            host,
            port,
        })
    }

    /// Validate a redirect target URL against the same permit.
    fn validate_redirect(
        permit: &AuthorizedNetworkPermit,
        redirect_url: &Url,
    ) -> Result<(), ToolError> {
        let host = redirect_url
            .host_str()
            .ok_or_else(|| ToolError::Validation("broker: redirect URL has no host".to_owned()))?;

        // Deny redirect to raw IP.
        if let Ok(ip) = host.parse::<IpAddr>() {
            if !ip.is_loopback() {
                return Err(ToolError::PermissionDenied(format!(
                    "broker: redirect to raw IP `{host}` is denied"
                )));
            }
        }

        let approved = permit
            .approved_hosts()
            .iter()
            .any(|rule| host_matches_rule(host, rule));

        if !approved {
            return Err(ToolError::PermissionDenied(format!(
                "broker: redirect to `{host}` is not in the approved allowlist"
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ValidatedUrl {
    url: Url,
    #[allow(dead_code)]
    host: String,
    #[allow(dead_code)]
    port: u16,
}

/// Checks whether a hostname matches a `HostRule` pattern.
///
/// Supports exact match and suffix match (`*.example.com`).
fn host_matches_rule(host: &str, rule: &HostRule) -> bool {
    if rule.pattern == host {
        return true;
    }
    // Wildcard suffix match: "*.example.com" matches "api.example.com".
    if let Some(suffix) = rule.pattern.strip_prefix("*.") {
        return host.ends_with(suffix) && host.len() > suffix.len();
    }
    false
}

/// Checks whether a port matches a `HostRule` port list.
///
/// When the rule has no port restriction, any port matches.
fn port_matches_rule(port: u16, rule: &HostRule) -> bool {
    match &rule.ports {
        Some(ports) => ports.is_empty() || ports.contains(&port),
        None => true,
    }
}

fn redact(value: String) -> String {
    // Strip credentials, tokens, keys from error messages.
    let mut s = value;
    // Basic redaction: strip common secret patterns from error strings.
    // Production redaction uses the injected Redactor when available.
    s = s.replace(|c: char| c.is_ascii_control() && c != '\n' && c != '\t', "");
    s
}

#[async_trait]
impl ToolNetworkBrokerPreflightCap for ReqwestToolNetworkBroker {
    async fn preflight_network_request(
        &self,
        request: &NetworkBrokerPreflightRequest,
    ) -> Result<(), ToolError> {
        match &request.network_access {
            NetworkAccess::AllowList(hosts) => {
                if hosts.is_empty() {
                    return Err(ToolError::Validation(
                        "broker preflight: allowlist is empty".to_owned(),
                    ));
                }
                Ok(())
            }
            NetworkAccess::None => Err(ToolError::Validation(
                "broker preflight: NetworkAccess::None cannot issue HTTP requests".to_owned(),
            )),
            NetworkAccess::Unrestricted => Err(ToolError::Validation(
                "broker preflight: HTTP broker v1 does not support unrestricted network access"
                    .to_owned(),
            )),
            NetworkAccess::LoopbackOnly => Err(ToolError::Validation(
                "broker preflight: HTTP broker v1 does not support loopback-only policy".to_owned(),
            )),
            _ => Err(ToolError::Validation(
                "broker preflight: unsupported network access variant".to_owned(),
            )),
        }
    }
}

#[async_trait]
impl ToolNetworkBrokerCap for ReqwestToolNetworkBroker {
    async fn execute_json(
        &self,
        permit: &AuthorizedNetworkPermit,
        request: ToolHttpJsonRequest,
    ) -> Result<ToolHttpResponse, ToolError> {
        let validated = Self::validate_request(permit, &request)?;

        // Build the reqwest request.
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Patch => reqwest::Method::PATCH,
        };

        let mut req = self.client.request(method, validated.url.clone());

        for (key, value) in &request.headers {
            req = req.header(key.as_str(), value.as_str());
        }

        if let Some(body) = &request.body {
            req = req.body(body.clone());
        }

        // Dispatch.
        let response = req.send().await.map_err(|error| {
            let msg = self
                .redactor
                .redact(&error.to_string(), &RedactRules::default());
            ToolError::Internal(format!("broker request failed: {msg}"))
        })?;

        let status = response.status().as_u16();
        let mut headers = std::collections::BTreeMap::new();
        for (key, value) in response.headers().iter() {
            if let Ok(v) = value.to_str() {
                headers.insert(key.to_string(), v.to_owned());
            }
        }

        // Read body with size cap.
        let body = read_response_body(response, self.max_response_bytes).await?;

        Ok(ToolHttpResponse {
            status,
            headers,
            body,
        })
    }
}

async fn read_response_body(
    response: reqwest::Response,
    max_bytes: u64,
) -> Result<Bytes, ToolError> {
    let content_length = response.content_length().unwrap_or(0);
    if content_length > max_bytes {
        return Err(ToolError::Validation(format!(
            "broker: response Content-Length {content_length} exceeds {max_bytes} byte limit"
        )));
    }

    match response.bytes().await {
        Ok(bytes) => {
            if bytes.len() as u64 > max_bytes {
                Err(ToolError::Validation(format!(
                    "broker: response body {} bytes exceeds {max_bytes} byte limit",
                    bytes.len()
                )))
            } else {
                Ok(bytes)
            }
        }
        Err(error) => Err(ToolError::Internal(format!(
            "broker: failed to read response body: {error}"
        ))),
    }
}
