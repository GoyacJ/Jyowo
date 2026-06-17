use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, TransportFailureKind, TrustLevel,
};
use reqwest::{redirect, Client, Url};

use crate::{
    HookContext, HookEvent, HookHandler, HookHttpSecurityPosture, HookOutcome, HookRegistrationKind,
};

use super::protocol::{decode_response, encode_request};
use super::{HookOutput, HookPayload, HookProtocolVersion, HookTransport};

#[derive(Debug, Clone)]
pub struct HookHttpSpec {
    pub handler_id: String,
    pub interested_events: Vec<HookEventKind>,
    pub failure_mode: HookFailureMode,
    pub url: Url,
    pub auth: HookHttpAuth,
    pub timeout: Duration,
    pub security: HookHttpSecurityPolicy,
    pub protocol_version: HookProtocolVersion,
    pub trust: TrustLevel,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookHttpAuth {
    None,
    BearerToken(String),
    StaticHeader { name: String, value: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookHttpSecurityPolicy {
    pub allowlist: HostAllowlist,
    pub ssrf_guard: SsrfGuardPolicy,
    pub max_redirects: usize,
    pub max_body_bytes: u64,
    pub mtls: Option<MtlsConfig>,
}

impl Default for HookHttpSecurityPolicy {
    fn default() -> Self {
        Self {
            allowlist: HostAllowlist::default(),
            ssrf_guard: SsrfGuardPolicy::default(),
            max_redirects: 0,
            max_body_bytes: 1024 * 1024,
            mtls: None,
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct HostAllowlist {
    hosts: Vec<String>,
}

impl HostAllowlist {
    pub fn from_hosts(hosts: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut hosts: Vec<_> = hosts
            .into_iter()
            .map(|host| normalize_host(host.as_ref()))
            .filter(|host| !host.is_empty())
            .collect();
        hosts.sort();
        hosts.dedup();
        Self { hosts }
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    pub fn contains_host(&self, host: &str) -> bool {
        let host = normalize_host(host);
        self.hosts.binary_search(&host).is_ok()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SsrfGuardPolicy {
    pub deny_loopback: bool,
    pub deny_private: bool,
    pub deny_link_local: bool,
    pub deny_metadata: bool,
}

impl Default for SsrfGuardPolicy {
    fn default() -> Self {
        Self {
            deny_loopback: true,
            deny_private: true,
            deny_link_local: true,
            deny_metadata: true,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MtlsConfig {
    pub identity_pem: Vec<u8>,
}

#[derive(Clone)]
pub struct HttpHookTransport {
    spec: HookHttpSpec,
    resolver: Arc<dyn HookHttpDnsResolver>,
}

impl HttpHookTransport {
    pub fn new(spec: HookHttpSpec) -> Result<Self, HookError> {
        Self::with_resolver(spec, Arc::new(TokioHookHttpDnsResolver))
    }

    pub fn with_resolver(
        spec: HookHttpSpec,
        resolver: Arc<dyn HookHttpDnsResolver>,
    ) -> Result<Self, HookError> {
        validate_spec(&spec)?;
        Ok(Self { spec, resolver })
    }

    pub fn spec(&self) -> &HookHttpSpec {
        &self.spec
    }

    pub fn handler_id(&self) -> &str {
        &self.spec.handler_id
    }

    pub fn interested_events(&self) -> &[HookEventKind] {
        &self.spec.interested_events
    }
}

#[async_trait]
impl HookTransport for HttpHookTransport {
    async fn invoke(&self, payload: HookPayload) -> HookOutput {
        let resolved = enforce_url_security(&self.spec, self.resolver.as_ref()).await?;
        let client = build_client(&self.spec, Some((&resolved.host, &resolved.addrs)))?;

        let request = encode_request(&payload, self.spec.protocol_version);
        let mut builder = client
            .post(self.spec.url.clone())
            .timeout(self.spec.timeout)
            .json(&request);

        builder = match &self.spec.auth {
            HookHttpAuth::None => builder,
            HookHttpAuth::BearerToken(token) => builder.bearer_auth(token),
            HookHttpAuth::StaticHeader { name, value } => builder.header(name, value),
        };

        let response = builder.send().await.map_err(network_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(HookError::Transport {
                kind: TransportFailureKind::NetworkError,
                detail: format!("hook http endpoint returned status {status}"),
            });
        }

        let body = response.bytes().await.map_err(network_error)?;
        if body.len() as u64 > self.spec.security.max_body_bytes {
            return Err(HookError::Transport {
                kind: TransportFailureKind::BodyTooLarge,
                detail: "hook http response exceeded max_body_bytes".to_owned(),
            });
        }

        decode_response(&body, self.spec.protocol_version)
    }
}

#[async_trait]
impl HookHandler for HttpHookTransport {
    fn handler_id(&self) -> &str {
        self.handler_id()
    }

    fn interested_events(&self) -> &[HookEventKind] {
        self.interested_events()
    }

    fn failure_mode(&self) -> HookFailureMode {
        self.spec.failure_mode
    }

    fn registration_kind(&self) -> HookRegistrationKind {
        HookRegistrationKind::Http
    }

    fn http_security_posture(&self) -> Option<HookHttpSecurityPosture> {
        Some(HookHttpSecurityPosture {
            allowlist_non_empty: !self.spec.security.allowlist.is_empty(),
            ssrf_guard_strict: self.spec.security.ssrf_guard.is_strict(),
        })
    }

    fn declared_trust(&self) -> Option<TrustLevel> {
        Some(self.spec.trust)
    }

    async fn handle(&self, event: HookEvent, ctx: HookContext) -> Result<HookOutcome, HookError> {
        self.invoke(HookPayload { event, ctx }).await
    }
}

fn validate_spec(spec: &HookHttpSpec) -> Result<(), HookError> {
    if spec.handler_id.trim().is_empty() {
        return Err(HookError::Message(
            "handler_id must not be empty".to_owned(),
        ));
    }
    if spec.interested_events.is_empty() {
        return Err(HookError::Message(
            "interested_events must not be empty".to_owned(),
        ));
    }
    if spec.trust == TrustLevel::UserControlled
        && (spec.security.allowlist.is_empty() || !spec.security.ssrf_guard.is_strict())
    {
        return Err(HookError::Unauthorized(
            "user-controlled http hooks require allowlist and strict ssrf guard".to_owned(),
        ));
    }
    if spec.security.ssrf_guard.is_strict() && spec.security.max_redirects > 0 {
        return Err(HookError::Unauthorized(
            "http hook redirects require per-hop SSRF validation and are disabled in M3".to_owned(),
        ));
    }
    if let Some(mtls) = &spec.security.mtls {
        reqwest::Identity::from_pem(&mtls.identity_pem).map_err(|error| HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            detail: format!("invalid mTLS identity: {error}"),
        })?;
    }
    Ok(())
}

fn build_client(
    spec: &HookHttpSpec,
    resolved: Option<(&str, &[SocketAddr])>,
) -> Result<Client, HookError> {
    let redirect_policy = if spec.security.max_redirects == 0 {
        redirect::Policy::none()
    } else {
        redirect::Policy::limited(spec.security.max_redirects)
    };

    let mut builder = Client::builder()
        .timeout(spec.timeout)
        .redirect(redirect_policy)
        .no_proxy();

    if let Some(mtls) = &spec.security.mtls {
        let identity = reqwest::Identity::from_pem(&mtls.identity_pem).map_err(|error| {
            HookError::Transport {
                kind: TransportFailureKind::NetworkError,
                detail: format!("invalid mTLS identity: {error}"),
            }
        })?;
        builder = builder.identity(identity);
    }

    if let Some((host, addrs)) = resolved {
        builder = builder.resolve_to_addrs(host, addrs);
    }

    builder.build().map_err(network_error)
}

async fn enforce_url_security(
    spec: &HookHttpSpec,
    resolver: &dyn HookHttpDnsResolver,
) -> Result<ResolvedHookTarget, HookError> {
    let host = spec.url.host_str().ok_or_else(|| HookError::Transport {
        kind: TransportFailureKind::AllowlistMiss,
        detail: "hook http url has no host".to_owned(),
    })?;

    if !spec.security.allowlist.is_empty() && !spec.security.allowlist.contains_host(host) {
        return Err(HookError::Transport {
            kind: TransportFailureKind::AllowlistMiss,
            detail: format!("host {host} is not allowlisted"),
        });
    }

    if spec.security.ssrf_guard.blocks_host(host) {
        return Err(HookError::Transport {
            kind: TransportFailureKind::SsrfBlocked,
            detail: format!("host {host} is blocked by ssrf guard"),
        });
    }

    let port = spec
        .url
        .port_or_known_default()
        .ok_or_else(|| HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            detail: "hook http url has no known port".to_owned(),
        })?;
    let addrs = resolver.resolve(host, port).await?;
    if addrs.is_empty() {
        return Err(HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            detail: format!("host {host} resolved to no addresses"),
        });
    }
    for addr in &addrs {
        if spec.security.ssrf_guard.blocks_ip(addr.ip()) {
            return Err(HookError::Transport {
                kind: TransportFailureKind::SsrfBlocked,
                detail: format!("host {host} resolved to blocked address {}", addr.ip()),
            });
        }
    }

    Ok(ResolvedHookTarget {
        host: host.to_owned(),
        addrs,
    })
}

struct ResolvedHookTarget {
    host: String,
    addrs: Vec<SocketAddr>,
}

#[async_trait]
pub trait HookHttpDnsResolver: Send + Sync + 'static {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, HookError>;
}

#[derive(Debug, Default)]
pub struct TokioHookHttpDnsResolver;

#[async_trait]
impl HookHttpDnsResolver for TokioHookHttpDnsResolver {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, HookError> {
        tokio::net::lookup_host((host, port))
            .await
            .map(|addrs| addrs.collect())
            .map_err(|error| HookError::Transport {
                kind: TransportFailureKind::NetworkError,
                detail: format!("dns resolution failed for {host}: {error}"),
            })
    }
}

impl SsrfGuardPolicy {
    fn is_strict(&self) -> bool {
        self.deny_loopback && self.deny_private && self.deny_link_local && self.deny_metadata
    }

    fn blocks_host(&self, host: &str) -> bool {
        if self.deny_loopback && host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        let Ok(ip) = host.parse::<IpAddr>() else {
            return false;
        };

        self.blocks_ip(ip)
    }

    fn blocks_ip(&self, ip: IpAddr) -> bool {
        (self.deny_loopback && ip.is_loopback())
            || (self.deny_private && is_private(ip))
            || (self.deny_link_local && is_link_local(ip))
            || (self.deny_metadata && is_metadata(ip))
    }
}

fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

fn network_error(error: reqwest::Error) -> HookError {
    HookError::Transport {
        kind: TransportFailureKind::NetworkError,
        detail: error.to_string(),
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private(),
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            (segments[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_link_local(),
        IpAddr::V6(ip) => (ip.segments()[0] & 0xffc0) == 0xfe80,
    }
}

fn is_metadata(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(ip) if ip == Ipv4Addr::new(169, 254, 169, 254))
}
