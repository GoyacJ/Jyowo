//! Authorized HTTP broker contracts.
//!
//! These types live in `jyowo-harness-tool` so that production tools can depend
//! on the broker interface without creating a lower-layer cycle. The production
//! reqwest-backed transport is implemented in `jyowo-harness-execution` (Task 6).

use std::collections::BTreeMap;
use std::fmt;
#[cfg(feature = "seedance-tools")]
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use harness_contracts::{
    ActionPlanHash, HostRule, NetworkAccess, RunId, SessionId, ToolError, ToolUseId,
};

use crate::{AuthorizationTicketKey, AuthorizedTicketSummary, AuthorizedToolInput};

/// Opaque preflight request carrying the approved network access shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkBrokerPreflightRequest {
    pub tool_name: String,
    pub tool_use_id: ToolUseId,
    pub network_access: NetworkAccess,
    pub action_plan_hash: ActionPlanHash,
}

/// Tool-layer broker preflight capability.
///
/// Registered once at runtime and injected into both authorization preflight
/// (via `ExecutionPreflightRegistry`) and authorized tool execution
/// (via `CapabilityRegistry`). Both paths MUST use the same instance.
#[async_trait]
pub trait ToolNetworkBrokerPreflightCap: Send + Sync + 'static {
    /// Validate that the requested network access is enforceable by this broker.
    ///
    /// Must fail closed when the broker cannot enforce the requested policy,
    /// when the broker is not registered, or when the request shape is invalid.
    async fn preflight_network_request(
        &self,
        request: &NetworkBrokerPreflightRequest,
    ) -> Result<(), ToolError>;
}

// ── Execution permit ──

/// An opaque authorization permit derived from `AuthorizedToolInput`.
///
/// Does not implement `Clone`. Fields are private. No public constructor exists
/// outside `AuthorizedToolInput::network_permit()`.
pub struct AuthorizedNetworkPermit {
    ticket: AuthorizedTicketSummary,
    tool_name: String,
    tool_use_id: ToolUseId,
    session_id: SessionId,
    run_id: RunId,
    network_access: NetworkAccess,
    approved_hosts: Vec<HostRule>,
    action_plan_hash: ActionPlanHash,
    _private: (),
}

impl fmt::Debug for AuthorizedNetworkPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizedNetworkPermit")
            .field("tool_name", &self.tool_name)
            .field("tool_use_id", &self.tool_use_id)
            .field("session_id", &self.session_id)
            .field("run_id", &self.run_id)
            .field("network_access", &self.network_access)
            .field("approved_host_count", &self.approved_hosts.len())
            .finish_non_exhaustive()
    }
}

impl AuthorizedNetworkPermit {
    /// Returns the approved host rules that this permit authorizes.
    pub fn approved_hosts(&self) -> &[HostRule] {
        &self.approved_hosts
    }

    /// Returns the immutable network access policy bound to this permit.
    pub fn network_access(&self) -> &NetworkAccess {
        &self.network_access
    }

    /// Returns the tool name bound to this permit.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Returns the tool use id bound to this permit.
    pub fn tool_use_id(&self) -> ToolUseId {
        self.tool_use_id
    }

    /// Returns the consumed authorization ticket bound to this permit.
    pub fn ticket(&self) -> &AuthorizedTicketSummary {
        &self.ticket
    }

    /// Returns the action plan hash bound to this permit.
    pub fn action_plan_hash(&self) -> &ActionPlanHash {
        &self.action_plan_hash
    }

    /// Verifies that this permit was derived from a ticket summary signed by
    /// the same authorization ticket authority as the runtime broker.
    pub fn verify_ticket_authority(&self, key: &AuthorizationTicketKey) -> bool {
        self.ticket.verify_authority(key)
            && self.tool_name == self.ticket.tool_name()
            && self.tool_use_id == self.ticket.tool_use_id()
            && self.session_id == self.ticket.session_id()
            && self.run_id == self.ticket.run_id()
            && self.action_plan_hash == *self.ticket.action_plan_hash()
    }
}

impl AuthorizedToolInput {
    /// Creates an opaque network permit bound to this authorized input.
    ///
    /// The permit carries the approved host rules from the action plan's
    /// `NetworkAccess::AllowList`. Returns an error when the action plan does
    /// not carry an allowlist (e.g. `NetworkAccess::None` or `Unrestricted`).
    pub fn network_permit(&self) -> Result<AuthorizedNetworkPermit, ToolError> {
        let action_plan = self.action_plan();
        let approved_hosts = match &action_plan.sandbox_policy.network {
            NetworkAccess::AllowList(hosts) => hosts.clone(),
            NetworkAccess::None => {
                return Err(ToolError::Validation(
                    "network_permit: action plan has no network access".to_owned(),
                ));
            }
            NetworkAccess::Unrestricted => {
                return Err(ToolError::Validation(
                    "network_permit: HTTP broker v1 does not support unrestricted network access"
                        .to_owned(),
                ));
            }
            NetworkAccess::LoopbackOnly => {
                return Err(ToolError::Validation(
                    "network_permit: HTTP broker v1 does not support loopback-only policy"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ToolError::Validation(
                    "network_permit: unsupported network access variant".to_owned(),
                ));
            }
        };

        let ticket = self.ticket();
        Ok(AuthorizedNetworkPermit {
            ticket: ticket.clone(),
            tool_name: ticket.tool_name().to_owned(),
            tool_use_id: ticket.tool_use_id(),
            session_id: ticket.session_id(),
            run_id: ticket.run_id(),
            network_access: action_plan.sandbox_policy.network.clone(),
            approved_hosts,
            action_plan_hash: ticket.action_plan_hash().clone(),
            _private: (),
        })
    }
}

// ── Request / response types ──

/// An HTTP JSON request issued through the authorized broker.
#[derive(Debug, Clone)]
pub struct ToolHttpJsonRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    pub max_response_bytes: u64,
}

impl Default for ToolHttpJsonRequest {
    fn default() -> Self {
        Self {
            method: HttpMethod::Post,
            url: String::new(),
            headers: BTreeMap::new(),
            body: None,
            timeout: Duration::from_secs(120),
            max_response_bytes: 10 * 1024 * 1024, // 10 MiB
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
        }
    }
}

/// The broker's response after execution.
#[derive(Debug, Clone)]
pub struct ToolHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Bytes,
}

// ── Broker execution capability ──

/// Full broker capability: preflight + authorized execution.
///
/// Extends the preflight-only trait from Task 3 so that authorization preflight
/// and production tool execution use the same interface and the same instance.
#[async_trait]
pub trait ToolNetworkBrokerCap: ToolNetworkBrokerPreflightCap {
    /// Execute an authorized HTTP request. The broker MUST validate every
    /// element of the request against the permit's immutable claims before
    /// any network dispatch:
    ///
    /// - scheme must be `http` or `https`
    /// - host and effective port must match one approved `HostRule`
    /// - public raw IP literals are denied
    /// - loopback IP literals are allowed only when the exact host:port pair is
    ///   explicitly approved
    /// - automatic redirects are not followed
    /// - response body capped to `request.max_response_bytes`
    /// - error strings redacted before returning
    async fn execute_json(
        &self,
        permit: &AuthorizedNetworkPermit,
        request: ToolHttpJsonRequest,
    ) -> Result<ToolHttpResponse, ToolError>;
}

// ── Seedance broker transport adapter ──

/// A `SeedanceHttpTransport` implementation that delegates to the authorized
/// network broker, validating every request against the approved host rules.
#[cfg(feature = "seedance-tools")]
pub struct BrokerSeedanceTransport {
    broker: Arc<dyn ToolNetworkBrokerCap>,
    permit: AuthorizedNetworkPermit,
}

#[cfg(feature = "seedance-tools")]
impl BrokerSeedanceTransport {
    pub fn new(broker: Arc<dyn ToolNetworkBrokerCap>, permit: AuthorizedNetworkPermit) -> Self {
        Self { broker, permit }
    }

    pub fn permit(&self) -> &AuthorizedNetworkPermit {
        &self.permit
    }
}

#[cfg(feature = "seedance-tools")]
#[async_trait]
impl harness_model::SeedanceHttpTransport for BrokerSeedanceTransport {
    async fn post_json(
        &self,
        url: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), harness_contracts::ModelError> {
        let req = ToolHttpJsonRequest {
            method: HttpMethod::Post,
            url: url.to_owned(),
            headers,
            body: Some(body),
            timeout: std::time::Duration::from_secs(120),
            max_response_bytes: 10 * 1024 * 1024,
        };
        self.broker
            .execute_json(&self.permit, req)
            .await
            .map(|resp| (resp.status, resp.body.to_vec()))
            .map_err(|e| harness_contracts::ModelError::ProviderUnavailable(e.to_string()))
    }

    async fn get_json(
        &self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> Result<(u16, Vec<u8>), harness_contracts::ModelError> {
        let req = ToolHttpJsonRequest {
            method: HttpMethod::Get,
            url: url.to_owned(),
            headers,
            body: None,
            timeout: std::time::Duration::from_secs(120),
            max_response_bytes: 10 * 1024 * 1024,
        };
        self.broker
            .execute_json(&self.permit, req)
            .await
            .map(|resp| (resp.status, resp.body.to_vec()))
            .map_err(|e| harness_contracts::ModelError::ProviderUnavailable(e.to_string()))
    }
}
