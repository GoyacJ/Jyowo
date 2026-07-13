use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(feature = "oauth")]
use std::sync::OnceLock;

use async_trait::async_trait;
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
#[cfg(feature = "websocket")]
use futures::SinkExt;
use futures::StreamExt;
use harness_contracts::{
    ManifestOriginRef, McpPromptOperation, McpResourceOperation, McpServerId, McpServerSource,
    MessagePart, ReferenceKind, Severity, TenantId, ToolActionPlan, ToolDescriptor, ToolError,
    ToolResult, ToolResultPart, ToolUseId,
};
use harness_execution::{AuthorizationContext, ExecutionError};
use harness_tool::{AuthorizedToolInput, ToolContext, ToolEvent, ToolRegistry};
#[cfg(feature = "oauth")]
use jsonwebtoken::{
    decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet},
    Algorithm, DecodingKey, Validation,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
#[cfg(feature = "websocket")]
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request as WsRequest, Response as WsResponse},
        Message as WsMessage,
    },
};

use crate::{
    authorize_mcp_prompt, authorize_mcp_resource, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    McpAuthorizationContext, McpContent, McpMetric, McpMetricOutcome, McpMetricsSink, McpPrompt,
    McpPromptMessages, McpReadResourceResult, McpResource, McpServerSpec, McpToolDescriptor,
    McpToolResult, NoopMcpEventSink, NoopMcpMetricsSink, SamplingJsonRpcHandler, SamplingPolicy,
    TransportChoice,
};

const JSONRPC_METHOD_NOT_FOUND: i32 = -32601;
const JSONRPC_INVALID_PARAMS: i32 = -32602;
const JSONRPC_INTERNAL_ERROR: i32 = -32603;
const JSONRPC_RATE_LIMITED: i32 = -32029;
const JSONRPC_UNAUTHORIZED: i32 = -32040;
const JSONRPC_TENANT_MAPPING: i32 = -32041;
#[cfg(feature = "oauth")]
const JWKS_CACHE_TTL: Duration = Duration::from_secs(300);

#[cfg(feature = "oauth")]
static JWKS_CACHE: OnceLock<Mutex<HashMap<String, CachedJwks>>> = OnceLock::new();

#[cfg(feature = "oauth")]
#[derive(Clone)]
struct CachedJwks {
    set: JwkSet,
    expires_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpServerError {
    #[error("missing tool context factory")]
    MissingToolContextFactory,
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("tenant mapping failed: {0}")]
    TenantMapping(String),
    #[error("tenant isolation rejected request tenant {request_tenant:?} for tool tenant {tool_tenant:?}")]
    TenantIsolation {
        request_tenant: TenantId,
        tool_tenant: TenantId,
    },
    #[error("rate limit exceeded")]
    RateLimited { retry_after: Duration },
    #[error("unsafe serving refused: {0}")]
    UnsafeServing(String),
    #[error("server transport not implemented: {0}")]
    UnsupportedServing(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerAuditEvent {
    TenantMappingRejected {
        reason: String,
        severity: Severity,
    },
    TenantIsolationRejected {
        request_tenant: TenantId,
        tool_tenant: TenantId,
        severity: Severity,
    },
    RateLimited {
        tenant_id: TenantId,
        capability: ExposedCapability,
        retry_after: Duration,
        severity: Severity,
    },
}

pub trait McpServerAuditSink: Send + Sync + 'static {
    fn record(&self, event: McpServerAuditEvent);
}

#[derive(Debug, Default)]
pub struct NoopMcpServerAuditSink;

impl McpServerAuditSink for NoopMcpServerAuditSink {
    fn record(&self, _event: McpServerAuditEvent) {}
}

#[async_trait]
pub trait McpServerAuthValidator: Send + Sync + 'static {
    async fn validate(&self, context: &mut McpServerRequestContext) -> Result<(), McpServerError>;
}

#[derive(Clone)]
pub enum McpServerAuth {
    None,
    StaticBearer(String),
    OAuthValidator {
        issuer: String,
        audience: String,
        jwks_url: String,
    },
    MutualTlsExternal {
        allowed_subjects: BTreeSet<String>,
        allowed_sha256_fingerprints: BTreeSet<String>,
    },
    Custom(Arc<dyn McpServerAuthValidator>),
}

impl fmt::Debug for McpServerAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::StaticBearer(_) => f.write_str("StaticBearer(<redacted>)"),
            Self::OAuthValidator {
                issuer,
                audience,
                jwks_url,
            } => f
                .debug_struct("OAuthValidator")
                .field("issuer", issuer)
                .field("audience", audience)
                .field("jwks_url", jwks_url)
                .finish(),
            Self::MutualTlsExternal {
                allowed_subjects,
                allowed_sha256_fingerprints,
            } => f
                .debug_struct("MutualTlsExternal")
                .field("allowed_subjects", allowed_subjects)
                .field("allowed_sha256_fingerprints", allowed_sha256_fingerprints)
                .finish(),
            Self::Custom(_) => f.write_str("Custom(<validator>)"),
        }
    }
}

impl McpServerAuth {
    async fn validate(&self, context: &mut McpServerRequestContext) -> Result<(), McpServerError> {
        match self {
            Self::None => Ok(()),
            Self::StaticBearer(token) => {
                let expected = format!("Bearer {token}");
                match context.header("authorization") {
                    Some(value) if value == expected => Ok(()),
                    _ => Err(McpServerError::Unauthorized(
                        "missing or invalid bearer token".to_owned(),
                    )),
                }
            }
            Self::Custom(validator) => validator.validate(context).await,
            Self::OAuthValidator {
                issuer,
                audience,
                jwks_url,
            } => validate_oauth_jwt(issuer, audience, jwks_url, context).await,
            Self::MutualTlsExternal {
                allowed_subjects,
                allowed_sha256_fingerprints,
            } => validate_external_mtls(allowed_subjects, allowed_sha256_fingerprints, context),
        }
    }

    fn allows_public_serving(&self) -> bool {
        !matches!(self, Self::None)
    }
}

fn validate_external_mtls(
    allowed_subjects: &BTreeSet<String>,
    allowed_sha256_fingerprints: &BTreeSet<String>,
    context: &McpServerRequestContext,
) -> Result<(), McpServerError> {
    if allowed_subjects.is_empty() && allowed_sha256_fingerprints.is_empty() {
        return Err(McpServerError::Unauthorized(
            "mutual tls external validation has no allowed identities".to_owned(),
        ));
    }

    let subject_allowed = context
        .verified_client_cert_subject()
        .is_some_and(|subject| allowed_subjects.contains(subject));
    let fingerprint_allowed = context
        .verified_client_cert_sha256_fingerprint()
        .is_some_and(|fingerprint| allowed_sha256_fingerprints.contains(fingerprint));

    if subject_allowed || fingerprint_allowed {
        Ok(())
    } else {
        Err(McpServerError::Unauthorized(
            "missing or untrusted mutual tls client certificate".to_owned(),
        ))
    }
}

#[cfg(feature = "oauth")]
async fn validate_oauth_jwt(
    issuer: &str,
    audience: &str,
    jwks_url: &str,
    context: &mut McpServerRequestContext,
) -> Result<(), McpServerError> {
    let token = bearer_token(context)?;
    let header = decode_header(token)
        .map_err(|_| McpServerError::Unauthorized("invalid oauth token header".to_owned()))?;
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| McpServerError::Unauthorized("oauth token missing kid".to_owned()))?;
    let mut jwks = cached_or_fetch_jwks(jwks_url, false).await?;
    let mut jwk = jwks.find(kid);
    if jwk.is_none() {
        jwks = cached_or_fetch_jwks(jwks_url, true).await?;
        jwk = jwks.find(kid);
    }
    let jwk =
        jwk.ok_or_else(|| McpServerError::Unauthorized("oauth token key not found".to_owned()))?;
    if !matches!(&jwk.algorithm, AlgorithmParameters::RSA(_)) {
        return Err(McpServerError::Unauthorized(
            "oauth token key algorithm is unsupported".to_owned(),
        ));
    }

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    let key = DecodingKey::from_jwk(jwk)
        .map_err(|_| McpServerError::Unauthorized("oauth token key is invalid".to_owned()))?;
    let data = decode::<BTreeMap<String, Value>>(token, &key, &validation)
        .map_err(|_| McpServerError::Unauthorized("oauth token validation failed".to_owned()))?;
    for (name, value) in data.claims {
        if let Some(value) = claim_value_to_string(&value) {
            context.insert_verified_claim(name, value);
        }
    }
    Ok(())
}

#[cfg(not(feature = "oauth"))]
async fn validate_oauth_jwt(
    _issuer: &str,
    _audience: &str,
    _jwks_url: &str,
    _context: &mut McpServerRequestContext,
) -> Result<(), McpServerError> {
    Err(McpServerError::Unauthorized(
        "oauth validator requires the oauth feature".to_owned(),
    ))
}

#[cfg(feature = "oauth")]
fn bearer_token(context: &McpServerRequestContext) -> Result<&str, McpServerError> {
    context
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| McpServerError::Unauthorized("missing oauth bearer token".to_owned()))
}

#[cfg(feature = "oauth")]
async fn cached_or_fetch_jwks(
    jwks_url: &str,
    force_refresh: bool,
) -> Result<JwkSet, McpServerError> {
    if !force_refresh {
        if let Some(jwks) = cached_jwks(jwks_url)? {
            return Ok(jwks);
        }
    }
    let jwks = reqwest::Client::new()
        .get(jwks_url)
        .send()
        .await
        .map_err(|_| McpServerError::Unauthorized("jwks fetch failed".to_owned()))?
        .error_for_status()
        .map_err(|_| McpServerError::Unauthorized("jwks fetch failed".to_owned()))?
        .json::<JwkSet>()
        .await
        .map_err(|_| McpServerError::Unauthorized("jwks response is invalid".to_owned()))?;
    let cache = JWKS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|error| McpServerError::Internal(format!("jwks cache poisoned: {error}")))?;
    cache.insert(
        jwks_url.to_owned(),
        CachedJwks {
            set: jwks.clone(),
            expires_at: Instant::now() + JWKS_CACHE_TTL,
        },
    );
    Ok(jwks)
}

#[cfg(feature = "oauth")]
fn cached_jwks(jwks_url: &str) -> Result<Option<JwkSet>, McpServerError> {
    let Some(cache) = JWKS_CACHE.get() else {
        return Ok(None);
    };
    let cache = cache
        .lock()
        .map_err(|error| McpServerError::Internal(format!("jwks cache poisoned: {error}")))?;
    Ok(cache
        .get(jwks_url)
        .filter(|entry| entry.expires_at > Instant::now())
        .map(|entry| entry.set.clone()))
}

#[cfg(feature = "oauth")]
fn claim_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[async_trait]
pub trait TenantResolver: Send + Sync + 'static {
    async fn resolve_tenant(
        &self,
        context: &McpServerRequestContext,
    ) -> Result<TenantId, McpServerError>;
}

#[derive(Clone)]
pub enum TenantMapping {
    Single(TenantId),
    Header(String),
    Claim(String),
    Custom(Arc<dyn TenantResolver>),
}

impl fmt::Debug for TenantMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single(tenant) => f.debug_tuple("Single").field(tenant).finish(),
            Self::Header(header) => f.debug_tuple("Header").field(header).finish(),
            Self::Claim(claim) => f.debug_tuple("Claim").field(claim).finish(),
            Self::Custom(_) => f.write_str("Custom(<tenant_resolver>)"),
        }
    }
}

impl TenantMapping {
    async fn resolve_tenant(
        &self,
        context: &McpServerRequestContext,
    ) -> Result<TenantId, McpServerError> {
        match self {
            Self::Single(tenant) => Ok(*tenant),
            Self::Header(header) => {
                let value = context.header(header).ok_or_else(|| {
                    McpServerError::TenantMapping(format!("missing tenant header `{header}`"))
                })?;
                value.parse::<TenantId>().map_err(|error| {
                    McpServerError::TenantMapping(format!(
                        "invalid tenant header `{header}`: {error}"
                    ))
                })
            }
            Self::Claim(claim) => {
                let value = context.verified_claim(claim).ok_or_else(|| {
                    McpServerError::TenantMapping(format!(
                        "missing verified tenant claim `{claim}`"
                    ))
                })?;
                value.parse::<TenantId>().map_err(|error| {
                    McpServerError::TenantMapping(format!(
                        "invalid verified tenant claim `{claim}`: {error}"
                    ))
                })
            }
            Self::Custom(resolver) => resolver.resolve_tenant(context).await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct McpServerPolicy {
    pub server_name: String,
    pub server_version: String,
    pub exposed_capabilities: ExposedCapabilities,
    pub auth: McpServerAuth,
    pub tenant_mapping: TenantMapping,
    pub tenant_isolation: TenantIsolationPolicy,
    pub rate_limit: McpServerRateLimit,
}

impl Default for McpServerPolicy {
    fn default() -> Self {
        Self {
            server_name: "jyowo-harness-mcp".to_owned(),
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
            exposed_capabilities: ExposedCapabilities::default(),
            auth: McpServerAuth::None,
            tenant_mapping: TenantMapping::Single(TenantId::SINGLE),
            tenant_isolation: TenantIsolationPolicy::default(),
            rate_limit: McpServerRateLimit::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRateLimit {
    pub global_rps: u32,
    pub per_tenant_rps: u32,
    pub per_capability_rps: BTreeMap<ExposedCapability, u32>,
    pub burst: u32,
    pub audit_throttle: bool,
}

impl McpServerRateLimit {
    pub fn unlimited() -> Self {
        Self {
            global_rps: 0,
            per_tenant_rps: 0,
            per_capability_rps: BTreeMap::new(),
            burst: 0,
            audit_throttle: false,
        }
    }
}

impl Default for McpServerRateLimit {
    fn default() -> Self {
        let mut per_capability_rps = BTreeMap::new();
        per_capability_rps.insert(ExposedCapability::MessagesSend, 6);
        Self {
            global_rps: 0,
            per_tenant_rps: 60,
            per_capability_rps,
            burst: 30,
            audit_throttle: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedCapabilities {
    pub sessions_list: bool,
    pub session_get: bool,
    pub messages_read: bool,
    pub messages_send: bool,
    pub attachments_fetch: bool,
    pub events_poll: bool,
    pub events_wait: bool,
    pub permissions_list_open: bool,
    pub permissions_respond: bool,
    pub channels_list: bool,
}

impl Default for ExposedCapabilities {
    fn default() -> Self {
        Self {
            sessions_list: true,
            session_get: true,
            messages_read: true,
            messages_send: false,
            attachments_fetch: true,
            events_poll: true,
            events_wait: true,
            permissions_list_open: true,
            permissions_respond: false,
            channels_list: true,
        }
    }
}

impl ExposedCapabilities {
    #[must_use]
    pub fn is_enabled(&self, capability: ExposedCapability) -> bool {
        match capability {
            ExposedCapability::SessionsList => self.sessions_list,
            ExposedCapability::SessionGet => self.session_get,
            ExposedCapability::MessagesRead => self.messages_read,
            ExposedCapability::MessagesSend => self.messages_send,
            ExposedCapability::AttachmentsFetch => self.attachments_fetch,
            ExposedCapability::EventsPoll => self.events_poll,
            ExposedCapability::EventsWait => self.events_wait,
            ExposedCapability::PermissionsListOpen => self.permissions_list_open,
            ExposedCapability::PermissionsRespond => self.permissions_respond,
            ExposedCapability::ChannelsList => self.channels_list,
            ExposedCapability::ToolsList
            | ExposedCapability::ToolsCall
            | ExposedCapability::ResourcesList
            | ExposedCapability::PromptsList => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantIsolationPolicy {
    pub mode: IsolationMode,
    pub audit_severity: Severity,
}

impl Default for TenantIsolationPolicy {
    fn default() -> Self {
        Self {
            mode: IsolationMode::StrictTenant,
            audit_severity: Severity::High,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationMode {
    StrictTenant,
    SingleTenant,
    Delegated,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExposedCapability {
    ToolsList,
    ToolsCall,
    ResourcesList,
    PromptsList,
    SessionsList,
    SessionGet,
    MessagesRead,
    MessagesSend,
    AttachmentsFetch,
    EventsPoll,
    EventsWait,
    PermissionsListOpen,
    PermissionsRespond,
    ChannelsList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRequestContext {
    pub tenant_id: TenantId,
    headers: BTreeMap<String, String>,
    verified_claims: BTreeMap<String, String>,
    verified_client_cert_subject: Option<String>,
    verified_client_cert_sha256_fingerprint: Option<String>,
}

impl Default for McpServerRequestContext {
    fn default() -> Self {
        Self {
            tenant_id: TenantId::SINGLE,
            headers: BTreeMap::new(),
            verified_claims: BTreeMap::new(),
            verified_client_cert_subject: None,
            verified_client_cert_sha256_fingerprint: None,
        }
    }
}

impl McpServerRequestContext {
    #[must_use]
    pub fn with_tenant_id(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = tenant_id;
        self
    }

    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(name.into()), value.into());
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&normalize_header_name(name))
            .map(String::as_str)
    }

    #[must_use]
    pub fn with_verified_claim(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.verified_claims.insert(name.into(), value.into());
        self
    }

    pub fn verified_claim(&self, name: &str) -> Option<&str> {
        self.verified_claims.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn with_verified_client_cert_subject(mut self, subject: impl Into<String>) -> Self {
        self.verified_client_cert_subject = Some(subject.into());
        self
    }

    #[must_use]
    pub fn with_verified_client_cert_sha256_fingerprint(
        mut self,
        fingerprint: impl Into<String>,
    ) -> Self {
        self.verified_client_cert_sha256_fingerprint = Some(fingerprint.into());
        self
    }

    pub fn verified_client_cert_subject(&self) -> Option<&str> {
        self.verified_client_cert_subject.as_deref()
    }

    pub fn verified_client_cert_sha256_fingerprint(&self) -> Option<&str> {
        self.verified_client_cert_sha256_fingerprint.as_deref()
    }

    #[cfg(feature = "oauth")]
    fn insert_verified_claim(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.verified_claims.insert(name.into(), value.into());
    }
}

#[async_trait]
pub trait ToolContextFactory: Send + Sync + 'static {
    async fn create_tool_context(
        &self,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<ToolContext, McpServerError>;
}

#[async_trait]
pub trait ToolCallAuthorizer: Send + Sync + 'static {
    async fn authorize_tool_call(
        &self,
        raw_input: Value,
        action_plan: ToolActionPlan,
        context: &ToolContext,
    ) -> Result<AuthorizedToolInput, ToolError>;
}

#[derive(Debug, Default)]
pub struct DenyToolCallAuthorizer;

#[async_trait]
impl ToolCallAuthorizer for DenyToolCallAuthorizer {
    async fn authorize_tool_call(
        &self,
        _raw_input: Value,
        _action_plan: ToolActionPlan,
        _context: &ToolContext,
    ) -> Result<AuthorizedToolInput, ToolError> {
        Err(ToolError::PermissionDenied(
            "tool authorization service is not configured".to_owned(),
        ))
    }
}

#[derive(Clone)]
pub struct AuthorizationContextToolCallAuthorizer {
    authorization_context: McpAuthorizationContext,
}

impl AuthorizationContextToolCallAuthorizer {
    #[must_use]
    pub fn new(authorization_context: McpAuthorizationContext) -> Self {
        Self {
            authorization_context,
        }
    }
}

#[async_trait]
impl ToolCallAuthorizer for AuthorizationContextToolCallAuthorizer {
    async fn authorize_tool_call(
        &self,
        raw_input: Value,
        action_plan: ToolActionPlan,
        context: &ToolContext,
    ) -> Result<AuthorizedToolInput, ToolError> {
        let authorization_context = AuthorizationContext {
            tenant_id: context.tenant_id,
            session_id: context.session_id,
            run_id: context.run_id,
            permission_mode: self.authorization_context.permission_mode,
            interactivity: self.authorization_context.interactivity,
            fallback_policy: self.authorization_context.fallback_policy,
            workspace_root: context.workspace_root.clone(),
        };
        self.authorization_context
            .authorization_service
            .authorize_tool_input(authorization_context, action_plan, raw_input)
            .await
            .map_err(mcp_authorization_error_to_tool_error)
    }
}

fn mcp_authorization_error_to_tool_error(error: ExecutionError) -> ToolError {
    match error {
        ExecutionError::PermissionDenied { decision, .. } => {
            ToolError::PermissionDenied(format!("authorization denied: {decision:?}"))
        }
        ExecutionError::SandboxPreflightFailed { reason, .. } => {
            ToolError::PermissionDenied(format!("sandbox preflight failed: {reason}"))
        }
        other => ToolError::Internal(other.to_string()),
    }
}

#[async_trait]
pub trait ResourceProvider: Send + Sync + 'static {
    async fn list_resources(&self) -> Result<Vec<McpResource>, McpServerError>;

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpServerError>;
}

#[async_trait]
pub trait PromptProvider: Send + Sync + 'static {
    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpServerError>;

    async fn get_prompt(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<McpPromptMessages, McpServerError>;
}

#[derive(Debug, Default)]
pub struct EmptyResourceProvider;

#[async_trait]
impl ResourceProvider for EmptyResourceProvider {
    async fn list_resources(&self) -> Result<Vec<McpResource>, McpServerError> {
        Ok(Vec::new())
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpServerError> {
        Err(McpServerError::InvalidParams(format!(
            "unknown resource: {uri}"
        )))
    }
}

#[derive(Debug, Default)]
pub struct EmptyPromptProvider;

#[async_trait]
impl PromptProvider for EmptyPromptProvider {
    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpServerError> {
        Ok(Vec::new())
    }

    async fn get_prompt(
        &self,
        name: &str,
        _arguments: Value,
    ) -> Result<McpPromptMessages, McpServerError> {
        Err(McpServerError::InvalidParams(format!(
            "unknown prompt: {name}"
        )))
    }
}

#[derive(Clone)]
pub struct StaticToolContextFactory {
    context: ToolContext,
}

impl StaticToolContextFactory {
    pub fn new(context: ToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl ToolContextFactory for StaticToolContextFactory {
    async fn create_tool_context(
        &self,
        _tool_name: &str,
        _arguments: &Value,
    ) -> Result<ToolContext, McpServerError> {
        let mut context = self.context.clone();
        context.tool_use_id = ToolUseId::new();
        Ok(context)
    }
}

#[derive(Clone)]
pub struct McpServerAdapter {
    registry: ToolRegistry,
    policy: McpServerPolicy,
    tool_context_factory: Arc<dyn ToolContextFactory>,
    tool_authorizer: Arc<dyn ToolCallAuthorizer>,
    resource_provider: Arc<dyn ResourceProvider>,
    prompt_provider: Arc<dyn PromptProvider>,
    sampling_handler: SamplingJsonRpcHandler,
    authorization_context: Option<McpAuthorizationContext>,
    rate_limit: Arc<Mutex<RateLimitState>>,
    audit_sink: Arc<dyn McpServerAuditSink>,
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl McpServerAdapter {
    pub fn builder(registry: ToolRegistry) -> McpServerAdapterBuilder {
        McpServerAdapterBuilder {
            registry,
            policy: McpServerPolicy::default(),
            tool_context_factory: None,
            tool_authorizer: Arc::new(DenyToolCallAuthorizer),
            tool_authorizer_configured: false,
            resource_provider: Arc::new(EmptyResourceProvider),
            prompt_provider: Arc::new(EmptyPromptProvider),
            sampling_handler: SamplingJsonRpcHandler::new(
                SamplingPolicy::denied(),
                Arc::new(NoopMcpEventSink),
            ),
            authorization_context: None,
            audit_sink: Arc::new(NoopMcpServerAuditSink),
            metrics_sink: Arc::new(NoopMcpMetricsSink),
        }
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        self.handle_request_with_context(request, McpServerRequestContext::default())
            .await
    }

    pub async fn handle_request_with_context(
        &self,
        request: JsonRpcRequest,
        mut context: McpServerRequestContext,
    ) -> JsonRpcResponse {
        let method = request.method.clone();
        if let Err(error) = self.policy.auth.validate(&mut context).await {
            self.record_server_request(&method, McpMetricOutcome::Denied);
            return JsonRpcResponse::failure(request.id, server_error_to_jsonrpc(error));
        }
        match self.policy.tenant_mapping.resolve_tenant(&context).await {
            Ok(tenant_id) => context.tenant_id = tenant_id,
            Err(error) => {
                self.record_tenant_mapping_rejection(&error);
                self.record_server_request(&method, McpMetricOutcome::Denied);
                return JsonRpcResponse::failure(request.id, server_error_to_jsonrpc(error));
            }
        }
        if let Err(error) =
            self.check_rate_limit(context.tenant_id, method_capability(&request.method))
        {
            self.record_rate_limit(
                context.tenant_id,
                method_capability(&request.method),
                &error,
            );
            self.record_server_request(&method, McpMetricOutcome::Throttled);
            return JsonRpcResponse::failure(request.id, server_error_to_jsonrpc(error));
        }

        if request.method == "sampling/createMessage" {
            let response = self.sampling_handler.handle_request(request).await;
            self.record_server_response(&method, &response);
            return response;
        }

        let result = match request.method.as_str() {
            "initialize" => self.initialize(),
            "ping" | "shutdown" => Ok(json!({})),
            "tools/list" => Ok(self.list_tools()),
            "tools/call" => self.call_tool(request.params.as_ref(), context).await,
            "resources/list" => self.list_resources().await,
            "resources/read" => self.read_resource(request.params.as_ref()).await,
            "prompts/list" => self.list_prompts().await,
            "prompts/get" => self.get_prompt(request.params.as_ref()).await,
            method => Err(jsonrpc_error(
                JSONRPC_METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        };

        let response = match result {
            Ok(value) => JsonRpcResponse::success(request.id, value),
            Err(error) => JsonRpcResponse::failure(request.id, error),
        };
        self.record_server_response(&method, &response);
        response
    }

    fn check_rate_limit(
        &self,
        tenant_id: TenantId,
        capability: ExposedCapability,
    ) -> Result<(), McpServerError> {
        let mut state = self.rate_limit.lock().map_err(|error| {
            McpServerError::Internal(format!("rate limit state poisoned: {error}"))
        })?;
        state.check(&self.policy.rate_limit, tenant_id, capability)
    }

    fn initialize(&self) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": {},
                "resources": {},
                "prompts": {},
            },
            "serverInfo": {
                "name": self.policy.server_name,
                "version": self.policy.server_version,
            }
        }))
    }

    fn list_tools(&self) -> Value {
        let tools = self
            .registry
            .snapshot()
            .as_descriptors()
            .into_iter()
            .map(tool_descriptor_to_mcp)
            .collect::<Vec<_>>();
        json!({ "tools": tools })
    }

    async fn call_tool(
        &self,
        params: Option<&Value>,
        request_context: McpServerRequestContext,
    ) -> Result<Value, JsonRpcError> {
        let params = params
            .ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "tools/call missing params"))?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "tools/call missing name"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err(jsonrpc_error(
                JSONRPC_INVALID_PARAMS,
                "tools/call arguments must be an object",
            ));
        }

        let tool = self.registry.get(name).ok_or_else(|| {
            jsonrpc_error(JSONRPC_INVALID_PARAMS, format!("unknown tool: {name}"))
        })?;
        validate_input_schema(&tool.descriptor().input_schema, &arguments)?;
        let context = self
            .tool_context_factory
            .create_tool_context(name, &arguments)
            .await
            .map_err(server_error_to_jsonrpc)?;
        if let Err(error) = self
            .policy
            .tenant_isolation
            .check(request_context.tenant_id, context.tenant_id)
        {
            self.record_tenant_isolation_rejection(&error);
            self.record_tenant_isolation_rejection_metric();
            return Err(server_error_to_jsonrpc(error));
        }

        if let Err(error) = tool.validate(&arguments, &context).await {
            return Ok(tool_error_result(format!("validation: {error}")));
        }

        let action_plan = match tool.plan(&arguments, &context).await {
            Ok(plan) => plan,
            Err(error) => return Ok(tool_error_result(error.to_string())),
        };
        let authorized = match self
            .tool_authorizer
            .authorize_tool_call(arguments, action_plan, &context)
            .await
        {
            Ok(authorized) => authorized,
            Err(error) => return Ok(tool_error_result(error.to_string())),
        };

        let output_schema = tool.descriptor().output_schema.clone();
        let stream = match tool.execute_authorized(authorized, context).await {
            Ok(stream) => stream,
            Err(error) => return Ok(tool_error_result(error.to_string())),
        };
        let result = collect_tool_stream(stream, output_schema.as_ref()).await;
        serde_json::to_value(result).map_err(|_| internal_jsonrpc_error())
    }

    async fn list_resources(&self) -> Result<Value, JsonRpcError> {
        self.authorize_resource(McpResourceOperation::List).await?;
        let resources = self
            .resource_provider
            .list_resources()
            .await
            .map_err(server_error_to_jsonrpc)?;
        Ok(json!({ "resources": resources }))
    }

    async fn read_resource(&self, params: Option<&Value>) -> Result<Value, JsonRpcError> {
        let params = params.ok_or_else(|| {
            jsonrpc_error(JSONRPC_INVALID_PARAMS, "resources/read missing params")
        })?;
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "resources/read missing uri"))?;

        self.authorize_resource(McpResourceOperation::Read {
            uri: uri.to_owned(),
        })
        .await?;
        let contents = self
            .resource_provider
            .read_resource(uri)
            .await
            .map_err(server_error_to_jsonrpc)?;
        serde_json::to_value(contents).map_err(|_| internal_jsonrpc_error())
    }

    async fn list_prompts(&self) -> Result<Value, JsonRpcError> {
        self.authorize_prompt(McpPromptOperation::List).await?;
        let prompts = self
            .prompt_provider
            .list_prompts()
            .await
            .map_err(server_error_to_jsonrpc)?;
        Ok(json!({ "prompts": prompts }))
    }

    async fn get_prompt(&self, params: Option<&Value>) -> Result<Value, JsonRpcError> {
        let params = params
            .ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "prompts/get missing params"))?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "prompts/get missing name"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err(jsonrpc_error(
                JSONRPC_INVALID_PARAMS,
                "prompts/get arguments must be an object",
            ));
        }

        self.authorize_prompt(McpPromptOperation::Get {
            name: name.to_owned(),
        })
        .await?;
        serde_json::to_value(
            self.prompt_provider
                .get_prompt(name, arguments)
                .await
                .map_err(server_error_to_jsonrpc)?,
        )
        .map_err(|_| internal_jsonrpc_error())
    }

    async fn authorize_resource(
        &self,
        operation: McpResourceOperation,
    ) -> Result<(), JsonRpcError> {
        let Some(context) = &self.authorization_context else {
            return Err(jsonrpc_error(
                JSONRPC_UNAUTHORIZED,
                "mcp resource authorization context is not configured",
            ));
        };
        authorize_mcp_resource(context, &self.authorization_spec(), operation)
            .await
            .map_err(|error| jsonrpc_error(JSONRPC_UNAUTHORIZED, error.to_string()))
    }

    async fn authorize_prompt(&self, operation: McpPromptOperation) -> Result<(), JsonRpcError> {
        let Some(context) = &self.authorization_context else {
            return Err(jsonrpc_error(
                JSONRPC_UNAUTHORIZED,
                "mcp prompt authorization context is not configured",
            ));
        };
        authorize_mcp_prompt(context, &self.authorization_spec(), operation)
            .await
            .map_err(|error| jsonrpc_error(JSONRPC_UNAUTHORIZED, error.to_string()))
    }

    fn authorization_spec(&self) -> McpServerSpec {
        McpServerSpec::new(
            McpServerId(self.policy.server_name.clone()),
            self.policy.server_name.clone(),
            TransportChoice::InProcess,
            McpServerSource::Dynamic {
                registered_by: "server_adapter".to_owned(),
            },
        )
        .with_manifest_origin(ManifestOriginRef::File {
            path: "mcp-server-adapter".to_owned(),
        })
    }

    fn record_tenant_mapping_rejection(&self, error: &McpServerError) {
        if let McpServerError::TenantMapping(reason) = error {
            self.audit_sink
                .record(McpServerAuditEvent::TenantMappingRejected {
                    reason: reason.clone(),
                    severity: self.policy.tenant_isolation.audit_severity,
                });
        }
    }

    fn record_tenant_isolation_rejection(&self, error: &McpServerError) {
        if let McpServerError::TenantIsolation {
            request_tenant,
            tool_tenant,
        } = error
        {
            self.audit_sink
                .record(McpServerAuditEvent::TenantIsolationRejected {
                    request_tenant: *request_tenant,
                    tool_tenant: *tool_tenant,
                    severity: self.policy.tenant_isolation.audit_severity,
                });
        }
    }

    fn record_rate_limit(
        &self,
        tenant_id: TenantId,
        capability: ExposedCapability,
        error: &McpServerError,
    ) {
        if let McpServerError::RateLimited { .. } = error {
            self.metrics_sink.record(McpMetric::ServerThrottled {
                capability: capability_label(capability),
            });
        }
        if !self.policy.rate_limit.audit_throttle {
            return;
        }
        if let McpServerError::RateLimited { retry_after } = error {
            self.audit_sink.record(McpServerAuditEvent::RateLimited {
                tenant_id,
                capability,
                retry_after: *retry_after,
                severity: self.policy.tenant_isolation.audit_severity,
            });
        }
    }

    fn record_tenant_isolation_rejection_metric(&self) {
        self.metrics_sink
            .record(McpMetric::ServerTenantIsolationRejected);
    }

    fn record_server_request(&self, method: &str, outcome: McpMetricOutcome) {
        self.metrics_sink.record(McpMetric::ServerRequest {
            method: method_metric_label(method),
            outcome,
        });
    }

    fn record_server_response(&self, method: &str, response: &JsonRpcResponse) {
        let outcome = if response.error.is_some() {
            McpMetricOutcome::Error
        } else {
            McpMetricOutcome::Success
        };
        self.record_server_request(method, outcome);
    }
}

pub struct HarnessMcpServer<H> {
    harness: Arc<H>,
    policy: McpServerPolicy,
    rate_limit: Arc<Mutex<RateLimitState>>,
    audit_sink: Arc<dyn McpServerAuditSink>,
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl<H> Clone for HarnessMcpServer<H> {
    fn clone(&self) -> Self {
        Self {
            harness: Arc::clone(&self.harness),
            policy: self.policy.clone(),
            rate_limit: Arc::clone(&self.rate_limit),
            audit_sink: Arc::clone(&self.audit_sink),
            metrics_sink: Arc::clone(&self.metrics_sink),
        }
    }
}

#[async_trait]
pub trait HarnessMcpBackend: Send + Sync + 'static {
    async fn call_harness_tool(
        &self,
        context: &McpServerRequestContext,
        capability: ExposedCapability,
        arguments: Value,
    ) -> Result<Value, McpServerError>;
}

impl<H> HarnessMcpServer<H>
where
    H: HarnessMcpBackend,
{
    #[allow(clippy::new_ret_no_self)]
    pub fn new(harness: Arc<H>) -> HarnessMcpServerBuilder<H> {
        HarnessMcpServerBuilder {
            harness,
            policy: McpServerPolicy::default(),
            audit_sink: Arc::new(NoopMcpServerAuditSink),
            metrics_sink: Arc::new(NoopMcpMetricsSink),
        }
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        self.handle_request_with_context(request, McpServerRequestContext::default())
            .await
    }

    pub async fn handle_request_with_context(
        &self,
        request: JsonRpcRequest,
        mut context: McpServerRequestContext,
    ) -> JsonRpcResponse {
        let method = request.method.clone();
        if let Err(error) = self.policy.auth.validate(&mut context).await {
            self.record_server_request(&method, McpMetricOutcome::Denied);
            return JsonRpcResponse::failure(request.id, server_error_to_jsonrpc(error));
        }
        match self.policy.tenant_mapping.resolve_tenant(&context).await {
            Ok(tenant_id) => context.tenant_id = tenant_id,
            Err(error) => {
                self.record_tenant_mapping_rejection(&error);
                self.record_server_request(&method, McpMetricOutcome::Denied);
                return JsonRpcResponse::failure(request.id, server_error_to_jsonrpc(error));
            }
        }

        let result = match request.method.as_str() {
            "initialize" => self.initialize(),
            "ping" | "shutdown" => Ok(json!({})),
            "tools/list" => self.list_tools(context.tenant_id),
            "tools/call" => self.call_tool(request.params.as_ref(), context).await,
            method => Err(jsonrpc_error(
                JSONRPC_METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        };

        let response = match result {
            Ok(value) => JsonRpcResponse::success(request.id, value),
            Err(error) => JsonRpcResponse::failure(request.id, error),
        };
        self.record_server_response(&method, &response);
        response
    }

    pub async fn serve_stdio(self) -> Result<(), McpServerError> {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut stdout = tokio::io::stdout();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?
        {
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
                Ok(request) => self.handle_request(request).await,
                Err(error) => JsonRpcResponse::failure(
                    Value::Null,
                    jsonrpc_error(JSONRPC_INVALID_PARAMS, format!("invalid json-rpc: {error}")),
                ),
            };
            let payload = serde_json::to_string(&response)
                .map_err(|error| McpServerError::Internal(error.to_string()))?;
            stdout
                .write_all(payload.as_bytes())
                .await
                .map_err(|error| McpServerError::Internal(error.to_string()))?;
            stdout
                .write_all(b"\n")
                .await
                .map_err(|error| McpServerError::Internal(error.to_string()))?;
            stdout
                .flush()
                .await
                .map_err(|error| McpServerError::Internal(error.to_string()))?;
        }
        Ok(())
    }

    pub async fn serve_http(self, addr: SocketAddr) -> Result<(), McpServerError> {
        self.ensure_public_serving_allowed()?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        self.serve_http_listener(listener).await
    }

    pub async fn serve_http_listener(self, listener: TcpListener) -> Result<(), McpServerError> {
        self.ensure_public_serving_allowed()?;
        let app = Router::new()
            .route("/", post(harness_http_handler::<H>))
            .with_state(self);
        axum::serve(listener, app)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))
    }

    #[cfg(feature = "websocket")]
    pub async fn serve_websocket(self, addr: SocketAddr) -> Result<(), McpServerError> {
        self.ensure_public_serving_allowed()?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        self.serve_websocket_listener(listener).await
    }

    #[cfg(not(feature = "websocket"))]
    pub async fn serve_websocket(self, _addr: SocketAddr) -> Result<(), McpServerError> {
        self.ensure_public_serving_allowed()?;
        Err(McpServerError::UnsupportedServing(
            "websocket serving requires the websocket feature".to_owned(),
        ))
    }

    #[cfg(feature = "websocket")]
    pub async fn serve_websocket_listener(
        self,
        listener: TcpListener,
    ) -> Result<(), McpServerError> {
        self.ensure_public_serving_allowed()?;
        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|error| McpServerError::Internal(error.to_string()))?;
            let server = self.clone();
            tokio::spawn(async move {
                let _ = serve_websocket_connection(server, stream).await;
            });
        }
    }

    fn ensure_public_serving_allowed(&self) -> Result<(), McpServerError> {
        if !self.policy.auth.allows_public_serving() {
            return Err(McpServerError::UnsafeServing(
                "public MCP serving requires explicit auth".to_owned(),
            ));
        }
        Ok(())
    }

    fn initialize(&self) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": {},
            },
            "serverInfo": {
                "name": self.policy.server_name,
                "version": self.policy.server_version,
            }
        }))
    }

    fn list_tools(&self, tenant_id: TenantId) -> Result<Value, JsonRpcError> {
        self.check_rate_limit(tenant_id, ExposedCapability::ToolsList)
            .map_err(|error| {
                self.record_rate_limit(tenant_id, ExposedCapability::ToolsList, &error);
                server_error_to_jsonrpc(error)
            })?;
        let tools = harness_tool_descriptors()
            .into_iter()
            .filter(|tool| self.policy.exposed_capabilities.is_enabled(tool.capability))
            .map(|tool| tool.descriptor)
            .collect::<Vec<_>>();
        Ok(json!({ "tools": tools }))
    }

    async fn call_tool(
        &self,
        params: Option<&Value>,
        context: McpServerRequestContext,
    ) -> Result<Value, JsonRpcError> {
        let (name, arguments) = tool_call_parts(params)?;
        let tool = harness_tool_descriptors()
            .into_iter()
            .find(|tool| tool.descriptor.name == name)
            .ok_or_else(|| {
                jsonrpc_error(JSONRPC_INVALID_PARAMS, format!("unknown tool: {name}"))
            })?;
        if !self.policy.exposed_capabilities.is_enabled(tool.capability) {
            return Err(jsonrpc_error(
                JSONRPC_INVALID_PARAMS,
                format!("unknown tool: {name}"),
            ));
        }
        validate_input_schema(&tool.descriptor.input_schema, &arguments)?;
        self.check_rate_limit(context.tenant_id, tool.capability)
            .map_err(|error| {
                self.record_rate_limit(context.tenant_id, tool.capability, &error);
                server_error_to_jsonrpc(error)
            })?;
        let result = self
            .harness
            .call_harness_tool(&context, tool.capability, arguments)
            .await
            .map_err(server_error_to_jsonrpc)?;
        serde_json::to_value(McpToolResult {
            content: vec![McpContent::text(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            )],
            structured_content: Some(structured_object(result)),
            is_error: false,
            meta: BTreeMap::new(),
        })
        .map_err(|_| internal_jsonrpc_error())
    }

    fn check_rate_limit(
        &self,
        tenant_id: TenantId,
        capability: ExposedCapability,
    ) -> Result<(), McpServerError> {
        let mut state = self.rate_limit.lock().map_err(|error| {
            McpServerError::Internal(format!("rate limit state poisoned: {error}"))
        })?;
        state.check(&self.policy.rate_limit, tenant_id, capability)
    }

    fn record_tenant_mapping_rejection(&self, error: &McpServerError) {
        if let McpServerError::TenantMapping(reason) = error {
            self.audit_sink
                .record(McpServerAuditEvent::TenantMappingRejected {
                    reason: reason.clone(),
                    severity: self.policy.tenant_isolation.audit_severity,
                });
        }
    }

    fn record_rate_limit(
        &self,
        tenant_id: TenantId,
        capability: ExposedCapability,
        error: &McpServerError,
    ) {
        if let McpServerError::RateLimited { .. } = error {
            self.metrics_sink.record(McpMetric::ServerThrottled {
                capability: capability_label(capability),
            });
        }
        if !self.policy.rate_limit.audit_throttle {
            return;
        }
        if let McpServerError::RateLimited { retry_after } = error {
            self.audit_sink.record(McpServerAuditEvent::RateLimited {
                tenant_id,
                capability,
                retry_after: *retry_after,
                severity: self.policy.tenant_isolation.audit_severity,
            });
        }
    }

    fn record_server_request(&self, method: &str, outcome: McpMetricOutcome) {
        self.metrics_sink.record(McpMetric::ServerRequest {
            method: method_metric_label(method),
            outcome,
        });
    }

    fn record_server_response(&self, method: &str, response: &JsonRpcResponse) {
        let outcome = if response.error.is_some() {
            McpMetricOutcome::Error
        } else {
            McpMetricOutcome::Success
        };
        self.record_server_request(method, outcome);
    }
}

async fn harness_http_handler<H>(
    State(server): State<HarnessMcpServer<H>>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse>
where
    H: HarnessMcpBackend,
{
    Json(
        server
            .handle_request_with_context(request, request_context_from_headers(&headers))
            .await,
    )
}

#[cfg(feature = "websocket")]
async fn serve_websocket_connection<H>(
    server: HarnessMcpServer<H>,
    stream: tokio::net::TcpStream,
) -> Result<(), McpServerError>
where
    H: HarnessMcpBackend,
{
    let context_slot = Arc::new(Mutex::new(None));
    let context_capture = Arc::clone(&context_slot);
    let mut socket = accept_hdr_async(stream, move |request: &WsRequest, response: WsResponse| {
        if let Ok(mut slot) = context_capture.lock() {
            *slot = Some(request_context_from_headers(request.headers()));
        }
        Ok(response)
    })
    .await
    .map_err(|error| McpServerError::Internal(error.to_string()))?;
    let context = context_slot
        .lock()
        .map_err(|error| McpServerError::Internal(error.to_string()))?
        .clone()
        .unwrap_or_default();

    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| McpServerError::Internal(error.to_string()))?;
        let WsMessage::Text(text) = message else {
            continue;
        };
        let response = match serde_json::from_str::<JsonRpcRequest>(&text) {
            Ok(request) => {
                server
                    .handle_request_with_context(request, context.clone())
                    .await
            }
            Err(error) => JsonRpcResponse::failure(
                Value::Null,
                jsonrpc_error(JSONRPC_INVALID_PARAMS, format!("invalid json-rpc: {error}")),
            ),
        };
        let payload = serde_json::to_string(&response)
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        socket
            .send(WsMessage::Text(payload.into()))
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
    }
    Ok(())
}

fn request_context_from_headers(headers: &HeaderMap) -> McpServerRequestContext {
    let mut context = McpServerRequestContext::default();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            context = context.with_header(name.as_str(), value);
        }
    }
    context
}

pub struct HarnessMcpServerBuilder<H> {
    harness: Arc<H>,
    policy: McpServerPolicy,
    audit_sink: Arc<dyn McpServerAuditSink>,
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl<H> HarnessMcpServerBuilder<H>
where
    H: HarnessMcpBackend,
{
    #[must_use]
    pub fn with_policy(mut self, policy: McpServerPolicy) -> Self {
        self.policy = policy;
        self
    }

    #[must_use]
    pub fn with_auth(mut self, auth: McpServerAuth) -> Self {
        self.policy.auth = auth;
        self
    }

    #[must_use]
    pub fn with_tenant_mapping(mut self, tenant_mapping: TenantMapping) -> Self {
        self.policy.tenant_mapping = tenant_mapping;
        self
    }

    #[must_use]
    pub fn with_audit_sink<T>(mut self, audit_sink: Arc<T>) -> Self
    where
        T: McpServerAuditSink,
    {
        self.audit_sink = audit_sink;
        self
    }

    #[must_use]
    pub fn with_metrics_sink<T>(mut self, metrics_sink: Arc<T>) -> Self
    where
        T: McpMetricsSink,
    {
        self.metrics_sink = metrics_sink;
        self
    }

    pub fn build(self) -> Result<HarnessMcpServer<H>, McpServerError> {
        Ok(HarnessMcpServer {
            harness: self.harness,
            policy: self.policy,
            rate_limit: Arc::new(Mutex::new(RateLimitState::default())),
            audit_sink: self.audit_sink,
            metrics_sink: self.metrics_sink,
        })
    }
}

impl TenantIsolationPolicy {
    fn check(&self, request_tenant: TenantId, tool_tenant: TenantId) -> Result<(), McpServerError> {
        match self.mode {
            IsolationMode::StrictTenant if request_tenant != tool_tenant => {
                Err(McpServerError::TenantIsolation {
                    request_tenant,
                    tool_tenant,
                })
            }
            IsolationMode::StrictTenant
            | IsolationMode::SingleTenant
            | IsolationMode::Delegated => Ok(()),
        }
    }
}

pub struct McpServerAdapterBuilder {
    registry: ToolRegistry,
    policy: McpServerPolicy,
    tool_context_factory: Option<Arc<dyn ToolContextFactory>>,
    tool_authorizer: Arc<dyn ToolCallAuthorizer>,
    tool_authorizer_configured: bool,
    resource_provider: Arc<dyn ResourceProvider>,
    prompt_provider: Arc<dyn PromptProvider>,
    sampling_handler: SamplingJsonRpcHandler,
    authorization_context: Option<McpAuthorizationContext>,
    audit_sink: Arc<dyn McpServerAuditSink>,
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl McpServerAdapterBuilder {
    #[must_use]
    pub fn with_policy(mut self, policy: McpServerPolicy) -> Self {
        self.policy = policy;
        self
    }

    #[must_use]
    pub fn with_tool_context_factory<T>(mut self, factory: T) -> Self
    where
        T: ToolContextFactory,
    {
        self.tool_context_factory = Some(Arc::new(factory));
        self
    }

    #[must_use]
    pub fn with_tool_authorizer<T>(mut self, authorizer: T) -> Self
    where
        T: ToolCallAuthorizer,
    {
        self.tool_authorizer = Arc::new(authorizer);
        self.tool_authorizer_configured = true;
        self
    }

    #[must_use]
    pub fn with_resource_provider<T>(mut self, provider: T) -> Self
    where
        T: ResourceProvider,
    {
        self.resource_provider = Arc::new(provider);
        self
    }

    #[must_use]
    pub fn with_prompt_provider<T>(mut self, provider: T) -> Self
    where
        T: PromptProvider,
    {
        self.prompt_provider = Arc::new(provider);
        self
    }

    #[must_use]
    pub fn with_sampling_handler(mut self, handler: SamplingJsonRpcHandler) -> Self {
        self.sampling_handler = handler;
        self
    }

    #[must_use]
    pub fn with_authorization_context(mut self, context: McpAuthorizationContext) -> Self {
        self.authorization_context = Some(context);
        self
    }

    #[must_use]
    pub fn with_audit_sink<T>(mut self, audit_sink: Arc<T>) -> Self
    where
        T: McpServerAuditSink,
    {
        self.audit_sink = audit_sink;
        self
    }

    #[must_use]
    pub fn with_metrics_sink<T>(mut self, metrics_sink: Arc<T>) -> Self
    where
        T: McpMetricsSink,
    {
        self.metrics_sink = metrics_sink;
        self
    }

    pub fn build(self) -> Result<McpServerAdapter, McpServerError> {
        let mut sampling_handler = self
            .sampling_handler
            .with_metrics_sink(Arc::clone(&self.metrics_sink));
        if let Some(context) = &self.authorization_context {
            sampling_handler = sampling_handler.with_authorization_context(context.clone());
        }
        let tool_authorizer = if self.tool_authorizer_configured {
            self.tool_authorizer
        } else if let Some(context) = &self.authorization_context {
            Arc::new(AuthorizationContextToolCallAuthorizer::new(context.clone()))
        } else {
            self.tool_authorizer
        };
        Ok(McpServerAdapter {
            registry: self.registry,
            policy: self.policy,
            tool_context_factory: self
                .tool_context_factory
                .ok_or(McpServerError::MissingToolContextFactory)?,
            tool_authorizer,
            resource_provider: self.resource_provider,
            prompt_provider: self.prompt_provider,
            sampling_handler,
            authorization_context: self.authorization_context,
            rate_limit: Arc::new(Mutex::new(RateLimitState::default())),
            audit_sink: self.audit_sink,
            metrics_sink: self.metrics_sink,
        })
    }
}

#[async_trait]
impl HarnessMcpBackend for () {
    async fn call_harness_tool(
        &self,
        _context: &McpServerRequestContext,
        _capability: ExposedCapability,
        _arguments: Value,
    ) -> Result<Value, McpServerError> {
        Err(McpServerError::UnsupportedServing(
            "harness backend is not configured".to_owned(),
        ))
    }
}

#[derive(Debug, Default)]
struct RateLimitState {
    global: TokenBucket,
    tenants: HashMap<TenantId, TokenBucket>,
    capabilities: BTreeMap<ExposedCapability, TokenBucket>,
}

impl RateLimitState {
    fn check(
        &mut self,
        policy: &McpServerRateLimit,
        tenant_id: TenantId,
        capability: ExposedCapability,
    ) -> Result<(), McpServerError> {
        let now = Instant::now();
        let burst = policy.burst.max(1);
        let mut retry_after = Duration::ZERO;

        if let Err(wait) = self.global.check(now, policy.global_rps, burst) {
            retry_after = retry_after.max(wait);
        }
        if let Err(wait) =
            self.tenants
                .entry(tenant_id)
                .or_default()
                .check(now, policy.per_tenant_rps, burst)
        {
            retry_after = retry_after.max(wait);
        }
        if let Some(rate) = policy.per_capability_rps.get(&capability) {
            if let Err(wait) = self
                .capabilities
                .entry(capability)
                .or_default()
                .check(now, *rate, burst)
            {
                retry_after = retry_after.max(wait);
            }
        }

        if retry_after.is_zero() {
            Ok(())
        } else {
            Err(McpServerError::RateLimited { retry_after })
        }
    }
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    initialized: bool,
}

impl Default for TokenBucket {
    fn default() -> Self {
        Self {
            tokens: 0.0,
            last_refill: Instant::now(),
            initialized: false,
        }
    }
}

impl TokenBucket {
    fn check(&mut self, now: Instant, rate_per_second: u32, burst: u32) -> Result<(), Duration> {
        if rate_per_second == 0 {
            return Ok(());
        }
        let capacity = f64::from(burst.max(1));
        if self.initialized {
            let elapsed = now.duration_since(self.last_refill).as_secs_f64();
            self.tokens = (self.tokens + elapsed * f64::from(rate_per_second)).min(capacity);
            self.last_refill = now;
        } else {
            self.tokens = capacity;
            self.last_refill = now;
            self.initialized = true;
        }
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let seconds = (1.0 - self.tokens) / f64::from(rate_per_second);
            Err(Duration::from_secs_f64(seconds.max(0.001)))
        }
    }
}

#[derive(Debug, Clone)]
struct HarnessToolSpec {
    capability: ExposedCapability,
    descriptor: McpToolDescriptor,
}

fn harness_tool_descriptors() -> Vec<HarnessToolSpec> {
    vec![
        harness_tool_spec(
            ExposedCapability::SessionsList,
            "sessions_list",
            "List Harness sessions",
            object_schema(
                &[],
                json!({
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "default": 50 },
                    "since": { "type": "string", "format": "date-time" },
                    "include_ended": { "type": "boolean", "default": true }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::SessionGet,
            "session_get",
            "Get Harness session metadata",
            object_schema(
                &["session_id"],
                json!({ "session_id": { "type": "string" } }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::MessagesRead,
            "messages_read",
            "Read Harness session messages",
            object_schema(
                &["session_id"],
                json!({
                    "session_id": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0, "default": 0 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "default": 50 }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::MessagesSend,
            "messages_send",
            "Send a user message into an existing Harness session",
            object_schema(
                &["session_id", "message"],
                json!({
                    "session_id": { "type": "string" },
                    "message": { "type": "string", "minLength": 1 }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::AttachmentsFetch,
            "attachments_fetch",
            "Fetch a Harness blob attachment",
            object_schema(&["blob_ref"], json!({ "blob_ref": { "type": "object" } })),
        ),
        harness_tool_spec(
            ExposedCapability::EventsPoll,
            "events_poll",
            "Poll Harness events",
            object_schema(
                &[],
                json!({
                    "after_event_id": { "type": "string" },
                    "session_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 20 }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::EventsWait,
            "events_wait",
            "Long-poll Harness events",
            object_schema(
                &[],
                json!({
                    "after_event_id": { "type": "string" },
                    "session_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 20 },
                    "timeout_ms": { "type": "integer", "minimum": 0, "maximum": 300_000, "default": 30_000 }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::PermissionsListOpen,
            "permissions_list_open",
            "List open Harness permission requests",
            object_schema(
                &[],
                json!({
                    "session_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "default": 50 }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::PermissionsRespond,
            "permissions_respond",
            "Resolve a Harness permission request",
            object_schema(
                &["session_id", "request_id", "option_id", "decision"],
                json!({
                    "session_id": { "type": "string" },
                    "request_id": { "type": "string" },
                    "option_id": { "type": "string" },
                    "decision": {
                        "type": "string",
                        "enum": [
                            "allow_once",
                            "deny_once"
                        ]
                    },
                    "confirmation_text": { "type": "string" }
                }),
            ),
        ),
        harness_tool_spec(
            ExposedCapability::ChannelsList,
            "channels_list",
            "List Harness outbound message channels",
            object_schema(&[], json!({ "platform": { "type": "string" } })),
        ),
    ]
}

fn harness_tool_spec(
    capability: ExposedCapability,
    name: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
) -> HarnessToolSpec {
    HarnessToolSpec {
        capability,
        descriptor: McpToolDescriptor {
            name: name.into(),
            title: None,
            description: Some(description.into()),
            icons: None,
            input_schema,
            execution: None,
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        },
    }
}

fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties,
    })
}

fn tool_call_parts(params: Option<&Value>) -> Result<(&str, Value), JsonRpcError> {
    let params =
        params.ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "tools/call missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| jsonrpc_error(JSONRPC_INVALID_PARAMS, "tools/call missing name"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        return Err(jsonrpc_error(
            JSONRPC_INVALID_PARAMS,
            "tools/call arguments must be an object",
        ));
    }
    Ok((name, arguments))
}

fn validate_input_schema(schema: &Value, arguments: &Value) -> Result<(), JsonRpcError> {
    let validator = if schema.get("$schema").is_some() {
        jsonschema::validator_for(schema)
    } else {
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(schema)
    }
    .map_err(|error| {
        jsonrpc_error(
            JSONRPC_INTERNAL_ERROR,
            format!("failed to compile tool input schema: {error}"),
        )
    })?;
    if validator.is_valid(arguments) {
        return Ok(());
    }
    let details = validator.iter_errors(arguments).next().map_or_else(
        || "tool input does not match input schema".to_owned(),
        |error| error.to_string(),
    );
    Err(jsonrpc_error(
        JSONRPC_INVALID_PARAMS,
        format!("tool input schema validation failed: {details}"),
    ))
}

fn method_capability(method: &str) -> ExposedCapability {
    match method {
        "tools/list" => ExposedCapability::ToolsList,
        "resources/list" | "resources/read" => ExposedCapability::ResourcesList,
        "prompts/list" | "prompts/get" => ExposedCapability::PromptsList,
        _ => ExposedCapability::ToolsCall,
    }
}

fn method_metric_label(method: &str) -> &'static str {
    match method {
        "initialize" => "initialize",
        "ping" => "ping",
        "shutdown" => "shutdown",
        "tools/list" => "tools_list",
        "tools/call" => "tools_call",
        "resources/list" => "resources_list",
        "resources/read" => "resources_read",
        "prompts/list" => "prompts_list",
        "prompts/get" => "prompts_get",
        "sampling/createMessage" => "sampling_create_message",
        _ => "unknown",
    }
}

fn capability_label(capability: ExposedCapability) -> &'static str {
    match capability {
        ExposedCapability::ToolsList => "tools_list",
        ExposedCapability::ToolsCall => "tools_call",
        ExposedCapability::ResourcesList => "resources_list",
        ExposedCapability::PromptsList => "prompts_list",
        ExposedCapability::SessionsList => "sessions_list",
        ExposedCapability::SessionGet => "session_get",
        ExposedCapability::MessagesRead => "messages_read",
        ExposedCapability::MessagesSend => "messages_send",
        ExposedCapability::AttachmentsFetch => "attachments_fetch",
        ExposedCapability::EventsPoll => "events_poll",
        ExposedCapability::EventsWait => "events_wait",
        ExposedCapability::PermissionsListOpen => "permissions_list_open",
        ExposedCapability::PermissionsRespond => "permissions_respond",
        ExposedCapability::ChannelsList => "channels_list",
    }
}

fn tool_descriptor_to_mcp(descriptor: &ToolDescriptor) -> McpToolDescriptor {
    McpToolDescriptor {
        name: descriptor.name.clone(),
        title: Some(descriptor.display_name.clone()),
        description: Some(descriptor.description.clone()),
        icons: None,
        input_schema: descriptor.input_schema.clone(),
        execution: None,
        output_schema: descriptor.output_schema.clone(),
        annotations: None,
        meta: BTreeMap::new(),
    }
}

async fn collect_tool_stream(
    mut stream: harness_tool::ToolStream,
    output_schema: Option<&Value>,
) -> McpToolResult {
    let mut content = Vec::new();
    let mut structured = Vec::new();
    while let Some(event) = stream.next().await {
        match event {
            ToolEvent::Progress(_) => {}
            ToolEvent::Partial(part) => match part {
                MessagePart::ToolResult {
                    content: result, ..
                } => append_tool_result(result, &mut content, &mut structured),
                other => {
                    append_mcp_result(message_part_to_mcp(other), &mut content, &mut structured)
                }
            },
            ToolEvent::Journal(_) => {}
            ToolEvent::Final(result) => {
                append_tool_result(result, &mut content, &mut structured);
                return finalize_tool_result(content, structured, output_schema);
            }
            ToolEvent::Error(error) => return mcp_error_result(error.to_string()),
        }
    }

    mcp_error_result("tool stream ended without final result")
}

fn append_tool_result(
    result: ToolResult,
    content: &mut Vec<McpContent>,
    structured: &mut Vec<serde_json::Map<String, Value>>,
) {
    match result {
        ToolResult::Mixed(parts) => {
            for part in parts {
                append_mcp_result(tool_result_part_to_mcp(part), content, structured);
            }
        }
        other => append_mcp_result(tool_result_to_mcp(other), content, structured),
    }
}

fn append_mcp_result(
    mapped: McpToolResult,
    content: &mut Vec<McpContent>,
    structured: &mut Vec<serde_json::Map<String, Value>>,
) {
    content.extend(mapped.content);
    if let Some(value) = mapped.structured_content {
        structured.push(value);
    }
}

fn tool_result_to_mcp(result: ToolResult) -> McpToolResult {
    match result {
        ToolResult::Text(text) => McpToolResult::text(text),
        ToolResult::Structured(value) => structured_result(value),
        ToolResult::Blob {
            content_type,
            blob_ref,
        } => McpToolResult::text(format!(
            "Blob content ({content_type}, {} bytes)",
            blob_ref.size
        )),
        ToolResult::Mixed(parts) => {
            let mut content = Vec::new();
            let mut structured = Vec::new();
            for part in parts {
                let mapped = tool_result_part_to_mcp(part);
                content.extend(mapped.content);
                if let Some(value) = mapped.structured_content {
                    structured.push(value);
                }
            }
            McpToolResult {
                content,
                structured_content: combine_structured(structured),
                is_error: false,
                meta: BTreeMap::new(),
            }
        }
        _ => McpToolResult::text("Unsupported tool result"),
    }
}

fn tool_result_part_to_mcp(part: ToolResultPart) -> McpToolResult {
    match part {
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => {
            McpToolResult::text(text)
        }
        ToolResultPart::Structured { value, .. } => structured_result(value),
        ToolResultPart::Blob {
            content_type,
            blob_ref,
            summary,
        } => {
            McpToolResult::text(summary.unwrap_or_else(|| {
                format!("Blob content ({content_type}, {} bytes)", blob_ref.size)
            }))
        }
        ToolResultPart::Reference {
            reference_kind: ReferenceKind::Url { url },
            title,
            summary,
        } => McpToolResult {
            content: vec![McpContent::ResourceLink {
                resource: Box::new(McpResource {
                    uri: url.clone(),
                    name: title.clone().unwrap_or(url),
                    title,
                    description: summary,
                    mime_type: None,
                    icons: None,
                    annotations: None,
                    size: None,
                    meta: BTreeMap::new(),
                }),
            }],
            structured_content: None,
            is_error: false,
            meta: BTreeMap::new(),
        },
        ToolResultPart::Reference {
            reference_kind,
            title,
            summary,
        } => {
            let identity = match reference_kind {
                ReferenceKind::File { path, line_range } => {
                    let mut identity = format!("file:{}", encode_path_identity(&path));
                    if let Some((start, end)) = line_range {
                        identity.push_str(&format!("#L{start}-L{end}"));
                    }
                    identity
                }
                ReferenceKind::Transcript(transcript) => format!(
                    "transcript:{}#offset={}-{}",
                    transcript.blob.id, transcript.from_offset.0, transcript.to_offset.0
                ),
                ReferenceKind::ToolUse { tool_use_id } => format!("tool-use:{tool_use_id}"),
                ReferenceKind::Memory { memory_id } => format!("memory:{memory_id}"),
                other => format!(
                    "reference:{}",
                    serde_json::to_string(&other).unwrap_or_else(|_| format!("{other:?}"))
                ),
            };
            McpToolResult::text(reference_text(identity, title, summary))
        }
        ToolResultPart::Table {
            headers,
            rows,
            caption,
        } => {
            let mut table = serde_json::Map::from_iter([
                ("headers".to_owned(), json!(headers)),
                ("rows".to_owned(), json!(rows)),
            ]);
            if let Some(caption) = caption {
                table.insert("caption".to_owned(), Value::String(caption));
            }
            structured_result(Value::Object(table))
        }
        ToolResultPart::Progress {
            stage,
            ratio,
            detail,
        } => {
            let mut text = detail.unwrap_or(stage);
            if let Some(ratio) = ratio {
                text.push_str(&format!(" ({:.0}%)", ratio * 100.0));
            }
            McpToolResult::text(text)
        }
        ToolResultPart::Error {
            code,
            message,
            retriable,
        } => McpToolResult::text(format!("{message} (code: {code}, retriable: {retriable})")),
        ToolResultPart::Artifact { title, preview, .. } => {
            McpToolResult::text(preview.unwrap_or(title))
        }
        _ => McpToolResult::text("Unsupported tool result part"),
    }
}

fn reference_text(identity: String, title: Option<String>, summary: Option<String>) -> String {
    let mut text = identity;
    if let Some(title) = title {
        text.push_str(" | title: ");
        text.push_str(&title);
    }
    if let Some(summary) = summary {
        text.push_str(" | summary: ");
        text.push_str(&summary);
    }
    text
}

fn encode_path_identity(path: &Path) -> String {
    let mut encoded = String::with_capacity(path.as_os_str().as_encoded_bytes().len());
    for &byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':' | b'\\')
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[usize::from(byte >> 4)]));
            encoded.push(char::from(b"0123456789ABCDEF"[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn message_part_to_mcp(part: MessagePart) -> McpToolResult {
    match part {
        MessagePart::Text(text) => McpToolResult::text(text),
        MessagePart::ToolResult { content, .. } => tool_result_to_mcp(content),
        other => structured_result(serde_json::to_value(other).unwrap_or_else(|_| json!({}))),
    }
}

fn structured_result(value: Value) -> McpToolResult {
    match value {
        Value::Object(structured) => McpToolResult {
            content: vec![McpContent::text(
                serde_json::to_string_pretty(&structured)
                    .unwrap_or_else(|_| Value::Object(structured.clone()).to_string()),
            )],
            structured_content: Some(structured),
            is_error: false,
            meta: BTreeMap::new(),
        },
        value => McpToolResult::text(
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        ),
    }
}

fn structured_object(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(object) => object,
        value => serde_json::Map::from_iter([("value".to_owned(), value)]),
    }
}

fn combine_structured(
    mut values: Vec<serde_json::Map<String, Value>>,
) -> Option<serde_json::Map<String, Value>> {
    match values.len() {
        0 => None,
        1 => values.pop(),
        _ => None,
    }
}

fn finalize_tool_result(
    mut content: Vec<McpContent>,
    structured: Vec<serde_json::Map<String, Value>>,
    output_schema: Option<&Value>,
) -> McpToolResult {
    let structured_count = structured.len();
    let structured_content = combine_structured(structured);
    let output_error = output_schema.and_then(|schema| match &structured_content {
        Some(value) => validate_output_schema(schema, value).err(),
        None => Some(format!(
            "tool output schema validation failed: expected exactly one structured object, got {structured_count}"
        )),
    });

    if let Some(error) = output_error {
        content.push(McpContent::text(error));
        return McpToolResult {
            content,
            structured_content: None,
            is_error: true,
            meta: BTreeMap::new(),
        };
    }

    McpToolResult {
        content,
        structured_content,
        is_error: false,
        meta: BTreeMap::new(),
    }
}

fn validate_output_schema(
    schema: &Value,
    structured: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let validator = if schema.get("$schema").is_some() {
        jsonschema::validator_for(schema)
    } else {
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(schema)
    }
    .map_err(|error| format!("failed to compile tool output schema: {error}"))?;
    let value = Value::Object(structured.clone());
    if validator.is_valid(&value) {
        return Ok(());
    }
    let details = validator.iter_errors(&value).next().map_or_else(
        || "tool output does not match output schema".to_owned(),
        |error| error.to_string(),
    );
    Err(format!("tool output schema validation failed: {details}"))
}

fn tool_error_result(message: impl Into<String>) -> Value {
    serde_json::to_value(mcp_error_result(message)).expect("mcp error result serializes")
}

fn mcp_error_result(message: impl Into<String>) -> McpToolResult {
    McpToolResult {
        content: vec![McpContent::text(message)],
        structured_content: None,
        is_error: true,
        meta: BTreeMap::new(),
    }
}

fn server_error_to_jsonrpc(error: McpServerError) -> JsonRpcError {
    match error {
        McpServerError::InvalidParams(message) => jsonrpc_error(JSONRPC_INVALID_PARAMS, message),
        McpServerError::RateLimited { retry_after } => JsonRpcError {
            code: JSONRPC_RATE_LIMITED,
            message: "rate limit exceeded".to_owned(),
            data: Some(json!({
                "retry_after_ms": retry_after.as_millis().max(1),
            })),
            extra: Default::default(),
        },
        McpServerError::Unauthorized(message) => jsonrpc_error(JSONRPC_UNAUTHORIZED, message),
        McpServerError::TenantMapping(message) => jsonrpc_error(JSONRPC_TENANT_MAPPING, message),
        McpServerError::TenantIsolation { .. }
        | McpServerError::MissingToolContextFactory
        | McpServerError::UnsafeServing(_)
        | McpServerError::UnsupportedServing(_)
        | McpServerError::Internal(_) => internal_jsonrpc_error(),
    }
}

fn internal_jsonrpc_error() -> JsonRpcError {
    jsonrpc_error(JSONRPC_INTERNAL_ERROR, "internal error")
}

fn jsonrpc_error(code: i32, message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code,
        message: message.into(),
        data: None,
        extra: Default::default(),
    }
}

fn normalize_header_name(name: impl AsRef<str>) -> String {
    name.as_ref().to_ascii_lowercase()
}
