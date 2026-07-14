use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use url::{Host, Url};

use crate::McpError;

pub(super) struct ParsedNetworkEndpoint {
    pub(super) url: Url,
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) kind: NetworkHostKind,
}

#[derive(Clone, Copy)]
pub(super) enum NetworkHostKind {
    Localhost,
    IpLiteral(IpAddr),
    DnsName,
}

pub(super) fn normalize_endpoint_host<S: AsRef<str>>(
    host: Host<S>,
) -> Result<(String, NetworkHostKind), McpError> {
    match host {
        Host::Domain(domain) => {
            let host = domain.as_ref().trim_end_matches('.').to_ascii_lowercase();
            if host.is_empty() {
                return Err(McpError::Protocol("MCP endpoint has no host".to_owned()));
            }
            let kind = if host == "localhost" {
                NetworkHostKind::Localhost
            } else {
                NetworkHostKind::DnsName
            };
            Ok((host, kind))
        }
        Host::Ipv4(ip) => Ok((ip.to_string(), NetworkHostKind::IpLiteral(IpAddr::V4(ip)))),
        Host::Ipv6(ip) => Ok((ip.to_string(), NetworkHostKind::IpLiteral(IpAddr::V6(ip)))),
    }
}

pub(super) fn normalize_endpoint_host_key(raw: &str) -> Option<String> {
    normalize_endpoint_host(Host::parse(raw).ok()?)
        .ok()
        .map(|(host, _)| host)
}

pub(super) async fn resolve_network_endpoint(
    endpoint: &ParsedNetworkEndpoint,
    explicit: Option<&[SocketAddr]>,
) -> Result<Vec<SocketAddr>, McpError> {
    let explicitly_pinned = explicit.is_some();
    let mut addrs = if let Some(addrs) = explicit {
        addrs
            .iter()
            .map(|addr| SocketAddr::new(addr.ip(), endpoint.port))
            .collect::<Vec<_>>()
    } else if let NetworkHostKind::IpLiteral(ip) = endpoint.kind {
        vec![SocketAddr::new(ip, endpoint.port)]
    } else {
        tokio::net::lookup_host((endpoint.host.as_str(), endpoint.port))
            .await
            .map_err(|_| McpError::Transport("MCP DNS resolution failed".to_owned()))?
            .collect::<Vec<_>>()
    };
    addrs.sort_unstable();
    addrs.dedup();
    if addrs.is_empty() {
        return Err(McpError::Transport(
            "MCP endpoint resolved to no addresses".to_owned(),
        ));
    }
    for addr in &addrs {
        validate_network_address(&endpoint.kind, addr.ip(), explicitly_pinned)?;
    }
    Ok(addrs)
}

pub(super) fn validate_network_address(
    kind: &NetworkHostKind,
    ip: IpAddr,
    _explicitly_pinned: bool,
) -> Result<(), McpError> {
    if is_always_blocked_ip(ip) {
        return Err(McpError::PermissionDenied(
            "MCP endpoint resolved to a disallowed address".to_owned(),
        ));
    }
    let ip = normalize_mapped_ip(ip);
    let valid = match kind {
        NetworkHostKind::Localhost => ip.is_loopback(),
        NetworkHostKind::IpLiteral(expected) => ip == normalize_mapped_ip(*expected),
        NetworkHostKind::DnsName => is_publicly_routable_dns_ip(ip),
    };
    if valid {
        Ok(())
    } else {
        Err(McpError::PermissionDenied(
            "MCP endpoint resolved to a disallowed address".to_owned(),
        ))
    }
}

fn normalize_mapped_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        ip => ip,
    }
}

fn is_always_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_link_local()
                || ip == Ipv4Addr::new(100, 100, 100, 200)
                || ip == Ipv4Addr::BROADCAST
        }
        IpAddr::V6(ip) => {
            ip.is_unspecified()
                || ip.is_multicast()
                || ip.to_ipv4_mapped().is_some()
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip
                    == "fd00:ec2::254"
                        .parse::<Ipv6Addr>()
                        .expect("valid metadata IP")
        }
    }
}

fn is_publicly_routable_dns_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 88 && c == 99)
                || (a == 192 && b == 168)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && segments[1] == 0x0002)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && !(segments[0] == 0x2001 && (segments[1] & 0xfff0) == 0x0010)
                && !(segments[0] == 0x2001 && (segments[1] & 0xfff0) == 0x0020)
                && !((segments[0] & 0xfff0) == 0x3ff0)
        }
    }
}
