//! Production reqwest-backed HTTP broker.
//!
//! Validates every outgoing HTTP request against an `AuthorizedNetworkPermit`
//! before dispatch. The same broker instance is injected into both authorization
//! preflight (`ExecutionPreflightRegistry`) and authorized tool execution
//! (`CapabilityRegistry`) so preflight and execution use identical policy.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use harness_contracts::{BudgetMetric, HostRule, NetworkAccess, RedactRules, Redactor, ToolError};
use harness_tool::{
    AuthorizationTicketKey, AuthorizedNetworkPermit, HttpMethod, NetworkBrokerPreflightRequest,
    ToolHttpJsonRequest, ToolHttpResponse, ToolNetworkBrokerCap, ToolNetworkBrokerPreflightCap,
    ToolWebSocketMessage, ToolWebSocketRequest, ToolWebSocketResponse,
};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::header::{HeaderName, HeaderValue},
    Message,
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
    ticket_authority_key: AuthorizationTicketKey,
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
        Self::new_with_ticket_authority(
            timeout,
            max_response_bytes,
            redactor,
            AuthorizationTicketKey::generate(),
        )
    }

    /// Creates a broker bound to the same ticket authority as the runtime
    /// `TicketLedger`.
    pub fn new_with_ticket_authority(
        timeout: Duration,
        max_response_bytes: u64,
        redactor: Arc<dyn Redactor>,
        ticket_authority_key: AuthorizationTicketKey,
    ) -> Result<Self, ToolError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .no_proxy()
            // No auto redirect following. Tools receive 3xx responses as-is.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ToolError::Internal(format!("broker client init failed: {error}")))?;

        Ok(Self {
            client,
            max_response_bytes,
            redactor,
            ticket_authority_key,
        })
    }

    /// Validates a request URL against the approved host rules in the permit.
    fn validate_request(
        permit: &AuthorizedNetworkPermit,
        request: &ToolHttpJsonRequest,
    ) -> Result<ValidatedUrl, ToolError> {
        Self::validate_url(permit, &request.url, &["http", "https"])
    }

    fn validate_websocket_request(
        permit: &AuthorizedNetworkPermit,
        request: &ToolWebSocketRequest,
    ) -> Result<ValidatedUrl, ToolError> {
        Self::validate_url(permit, &request.url, &["ws", "wss"])
    }

    fn validate_url(
        permit: &AuthorizedNetworkPermit,
        url: &str,
        allowed_schemes: &[&str],
    ) -> Result<ValidatedUrl, ToolError> {
        let approved_hosts = permit_approved_hosts(permit)?;
        let parsed = Url::parse(url).map_err(|error| {
            ToolError::Validation(redact(format!("invalid request URL: {error}")))
        })?;

        let scheme = parsed.scheme().to_owned();
        if !allowed_schemes.contains(&scheme.as_str()) {
            return Err(ToolError::Validation(format!(
                "broker: scheme `{scheme}` is not allowed"
            )));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(ToolError::PermissionDenied(
                "broker: URL userinfo credentials are not allowed".to_owned(),
            ));
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
                let explicitly_approved = approved_hosts.iter().any(|rule| {
                    rule.pattern == host
                        && rule
                            .ports
                            .as_ref()
                            .is_some_and(|ports| ports.contains(&port))
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
        let approved = approved_hosts
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
    // Wildcard suffix match: "*.example.com" matches "api.example.com",
    // but not "badexample.com" or the bare "example.com".
    if let Some(suffix) = rule.pattern.strip_prefix("*.") {
        return host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1);
    }
    false
}

/// Checks whether a port matches a `HostRule` port list.
///
/// HTTP broker v1 requires an explicit effective port in every allowlist rule.
fn port_matches_rule(port: u16, rule: &HostRule) -> bool {
    match &rule.ports {
        Some(ports) => !ports.is_empty() && ports.contains(&port),
        None => false,
    }
}

fn permit_approved_hosts(permit: &AuthorizedNetworkPermit) -> Result<&[HostRule], ToolError> {
    match permit.network_access() {
        NetworkAccess::AllowList(hosts) if !hosts.is_empty() => Ok(hosts.as_slice()),
        NetworkAccess::AllowList(_) => Err(ToolError::PermissionDenied(
            "broker: authorized network permit has an empty allowlist".to_owned(),
        )),
        _ => Err(ToolError::PermissionDenied(
            "broker: authorized network permit is not an allowlist permit".to_owned(),
        )),
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
                if hosts.iter().any(|rule| match &rule.ports {
                    Some(ports) => ports.is_empty(),
                    None => true,
                }) {
                    return Err(ToolError::Validation(
                        "broker preflight: every allowlist rule must include at least one explicit port"
                            .to_owned(),
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
        if !permit.verify_ticket_authority(&self.ticket_authority_key) {
            return Err(ToolError::PermissionDenied(
                "broker: authorization ticket proof is invalid".to_owned(),
            ));
        }
        let validated = Self::validate_request(permit, &request)?;

        // Build the reqwest request.
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Patch => reqwest::Method::PATCH,
        };

        let mut req = self
            .client
            .request(method, validated.url.clone())
            .timeout(request.timeout);

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
        let response_cap = self.max_response_bytes.min(request.max_response_bytes);
        let body = read_response_body(response, response_cap).await?;

        Ok(ToolHttpResponse {
            status,
            headers,
            body,
        })
    }

    async fn execute_websocket(
        &self,
        permit: &AuthorizedNetworkPermit,
        request: ToolWebSocketRequest,
    ) -> Result<ToolWebSocketResponse, ToolError> {
        if !permit.verify_ticket_authority(&self.ticket_authority_key) {
            return Err(ToolError::PermissionDenied(
                "broker: authorization ticket proof is invalid".to_owned(),
            ));
        }
        let validated = Self::validate_websocket_request(permit, &request)?;
        if request.max_response_messages == 0 {
            return Err(ToolError::Validation(
                "broker: max_response_messages must be greater than zero".to_owned(),
            ));
        }
        if request.total_timeout.is_zero() {
            return Err(ToolError::Validation(
                "broker: total_timeout must be greater than zero".to_owned(),
            ));
        }
        let started_at = Instant::now();

        let mut ws_request = validated
            .url
            .as_str()
            .into_client_request()
            .map_err(|error| {
                ToolError::Validation(redact(format!("invalid WebSocket request: {error}")))
            })?;
        for (key, value) in &request.headers {
            let name = HeaderName::from_bytes(key.as_bytes()).map_err(|error| {
                ToolError::Validation(redact(format!("invalid WebSocket header name: {error}")))
            })?;
            if websocket_header_is_broker_owned(&name) {
                return Err(ToolError::Validation(format!(
                    "broker: WebSocket header `{}` is managed by the broker and cannot be overridden",
                    name.as_str()
                )));
            }
            let value = HeaderValue::from_str(value).map_err(|error| {
                ToolError::Validation(redact(format!("invalid WebSocket header value: {error}")))
            })?;
            ws_request.headers_mut().insert(name, value);
        }

        let (mut websocket, _) = tokio::time::timeout(
            request.timeout.min(request.total_timeout),
            tokio_tungstenite::connect_async(ws_request),
        )
        .await
        .map_err(|_| ToolError::Internal("broker WebSocket connect timed out".to_owned()))?
        .map_err(|error| {
            let msg = self
                .redactor
                .redact(&error.to_string(), &RedactRules::default());
            ToolError::Internal(format!("broker WebSocket connect failed: {msg}"))
        })?;

        let mut next_to_send = 0_usize;
        if !request.send_next_after_each_response {
            while next_to_send < request.text_messages.len() {
                websocket
                    .send(Message::Text(
                        request.text_messages[next_to_send].clone().into(),
                    ))
                    .await
                    .map_err(|error| {
                        let msg = self
                            .redactor
                            .redact(&error.to_string(), &RedactRules::default());
                        ToolError::Internal(format!("broker WebSocket send failed: {msg}"))
                    })?;
                next_to_send += 1;
            }
        }

        let response_cap = self.max_response_bytes.min(request.max_response_bytes);
        let mut observed_bytes = 0_u64;
        let mut messages = Vec::new();
        while messages.len() < request.max_response_messages {
            let remaining = request
                .total_timeout
                .checked_sub(started_at.elapsed())
                .ok_or_else(|| {
                    ToolError::Internal("broker WebSocket exchange timed out".to_owned())
                })?;
            let read_timeout = request.timeout.min(remaining);
            let Some(message) = tokio::time::timeout(read_timeout, websocket.next())
                .await
                .map_err(|_| ToolError::Internal("broker WebSocket read timed out".to_owned()))?
            else {
                break;
            };
            let message = message.map_err(|error| {
                let msg = self
                    .redactor
                    .redact(&error.to_string(), &RedactRules::default());
                ToolError::Internal(format!("broker WebSocket read failed: {msg}"))
            })?;

            let terminates = match message {
                Message::Text(text) => {
                    observed_bytes = add_websocket_bytes(observed_bytes, text.len(), response_cap)?;
                    let text = text.to_string();
                    let terminates = request
                        .text_response_terminators
                        .iter()
                        .any(|needle| text.contains(needle));
                    messages.push(ToolWebSocketMessage::Text(text));
                    terminates
                }
                Message::Binary(bytes) => {
                    observed_bytes =
                        add_websocket_bytes(observed_bytes, bytes.len(), response_cap)?;
                    messages.push(ToolWebSocketMessage::Binary(Bytes::from(bytes.to_vec())));
                    false
                }
                Message::Close(_) => break,
                Message::Ping(payload) => {
                    websocket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| {
                            let msg = self
                                .redactor
                                .redact(&error.to_string(), &RedactRules::default());
                            ToolError::Internal(format!("broker WebSocket pong failed: {msg}"))
                        })?;
                    false
                }
                Message::Pong(_) | Message::Frame(_) => false,
            };

            if terminates {
                break;
            }

            if request.send_next_after_each_response && next_to_send < request.text_messages.len() {
                websocket
                    .send(Message::Text(
                        request.text_messages[next_to_send].clone().into(),
                    ))
                    .await
                    .map_err(|error| {
                        let msg = self
                            .redactor
                            .redact(&error.to_string(), &RedactRules::default());
                        ToolError::Internal(format!("broker WebSocket send failed: {msg}"))
                    })?;
                next_to_send += 1;
            }
        }

        if messages.len() >= request.max_response_messages {
            return Err(ToolError::Message(
                "broker WebSocket response message limit exceeded".to_owned(),
            ));
        }

        Ok(ToolWebSocketResponse { messages })
    }
}

fn add_websocket_bytes(current: u64, next: usize, max_bytes: u64) -> Result<u64, ToolError> {
    let next_total = current.saturating_add(next as u64);
    if next_total > max_bytes {
        return Err(ToolError::ResultTooLarge {
            original: next_total,
            limit: max_bytes,
            metric: BudgetMetric::Bytes,
        });
    }
    Ok(next_total)
}

fn websocket_header_is_broker_owned(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "connection"
            | "upgrade"
            | "sec-websocket-key"
            | "sec-websocket-version"
            | "sec-websocket-protocol"
            | "sec-websocket-extensions"
            | "sec-websocket-accept"
    )
}

async fn read_response_body(
    mut response: reqwest::Response,
    max_bytes: u64,
) -> Result<Bytes, ToolError> {
    let content_length = response.content_length().unwrap_or(0);
    if content_length > max_bytes {
        return Err(ToolError::ResultTooLarge {
            original: content_length,
            limit: max_bytes,
            metric: BudgetMetric::Bytes,
        });
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        ToolError::Internal(format!("broker: failed to read response body: {error}"))
    })? {
        let next_len = body.len() as u64 + chunk.len() as u64;
        if next_len > max_bytes {
            return Err(ToolError::ResultTooLarge {
                original: next_len,
                limit: max_bytes,
                metric: BudgetMetric::Bytes,
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}
