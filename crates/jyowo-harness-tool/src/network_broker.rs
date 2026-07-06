//! Authorized HTTP broker contracts.
//!
//! These types live in `jyowo-harness-tool` so that production tools can depend
//! on the broker interface without creating a lower-layer cycle. The production
//! reqwest-backed transport is implemented in `jyowo-harness-execution` (Task 6).

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use harness_contracts::{
    ActionPlanHash, AuthorizationTicketId, HostRule, NetworkAccess, RunId, SessionId, TenantId,
    ToolError, ToolUseId,
};

use crate::AuthorizedTicketSummary;

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
    pub(crate) ticket: AuthorizedTicketSummary,
    pub(crate) tool_name: String,
    pub(crate) tool_use_id: ToolUseId,
    pub(crate) session_id: SessionId,
    pub(crate) run_id: RunId,
    pub(crate) network_access: NetworkAccess,
    pub(crate) approved_hosts: Vec<HostRule>,
    pub(crate) action_plan_hash: ActionPlanHash,
    pub(crate) _private: (),
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
    /// Test-only constructor. Not part of the public API; production code must
    /// obtain permits through `AuthorizedToolInput::network_permit()`.
    #[doc(hidden)]
    pub fn for_test(
        tool_name: impl Into<String>,
        tool_use_id: ToolUseId,
        session_id: SessionId,
        run_id: RunId,
        approved_hosts: Vec<HostRule>,
    ) -> Self {
        let name: String = tool_name.into();
        Self {
            ticket: AuthorizedTicketSummary {
                ticket_id: AuthorizationTicketId::new(),
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id,
                tool_use_id,
                tool_name: name.clone(),
                action_plan_hash: ActionPlanHash::default(),
                consumed_at: Utc::now(),
            },
            tool_name: name,
            tool_use_id,
            session_id,
            run_id,
            network_access: NetworkAccess::AllowList(approved_hosts.clone()),
            approved_hosts,
            action_plan_hash: ActionPlanHash::default(),
            _private: (),
        }
    }

    /// Returns the approved host rules that this permit authorizes.
    pub fn approved_hosts(&self) -> &[HostRule] {
        &self.approved_hosts
    }

    /// Returns the tool name bound to this permit.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Returns the tool use id bound to this permit.
    pub fn tool_use_id(&self) -> ToolUseId {
        self.tool_use_id
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
    /// - redirects denied unless each target is validated against the same allowlist
    /// - response body capped to `request.max_response_bytes`
    /// - error strings redacted before returning
    async fn execute_json(
        &self,
        permit: &AuthorizedNetworkPermit,
        request: ToolHttpJsonRequest,
    ) -> Result<ToolHttpResponse, ToolError>;
}
