//! Client network context: who is the TCP peer, who is the (possibly proxied) client.
//!
//! Proxy headers (`X-Forwarded-For`, `X-Forwarded-Proto`, `CF-Connecting-IP`, `X-Real-IP`)
//! are trusted only when the TCP peer is loopback (same-host reverse proxy such as
//! cloudflared or nginx). LAN peers must not be able to rewrite the client IP.

use axum::http::HeaderMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

/// Per-request network facts, stored in request extensions by the capture middleware.
#[derive(Clone, Debug)]
pub struct RequestNetwork {
    /// Original TCP peer address.
    pub peer_ip: Option<IpAddr>,
    /// Peer after applying `X-Forwarded-For` (only when the peer is loopback).
    pub remote_ip: Option<IpAddr>,
    /// Request scheme after applying `X-Forwarded-Proto` (only when the peer is loopback).
    pub scheme: String,
}

impl RequestNetwork {
    /// Build from the TCP peer and request headers (applies trusted forwarded headers).
    pub fn capture(peer: Option<IpAddr>, headers: &HeaderMap) -> Self {
        let mut remote_ip = peer;
        let mut scheme = "http".to_string();
        if is_loopback(peer) {
            // Forward limit 1: only the right-most X-Forwarded-For entry is consumed.
            if let Some(ip) = header_values(headers, "x-forwarded-for").last().and_then(|v| parse_ip_lenient(v)) {
                remote_ip = Some(ip);
            }
            if let Some(p) = header_values(headers, "x-forwarded-proto").last() {
                if !p.is_empty() {
                    scheme = p.to_ascii_lowercase();
                }
            }
        }
        RequestNetwork { peer_ip: peer, remote_ip, scheme }
    }

    /// Context without any proxy processing (peer = remote).
    pub fn direct(peer: Option<IpAddr>) -> Self {
        RequestNetwork { peer_ip: peer, remote_ip: peer, scheme: "http".into() }
    }
}

/// Every comma separated value of a (possibly repeated) header, trimmed.
fn header_values(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Accepts `1.2.3.4`, `1.2.3.4:5678`, `::1`, `[::1]:80`.
fn parse_ip_lenient(s: &str) -> Option<IpAddr> {
    let s = s.trim();
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(ip);
    }
    if let Ok(sa) = s.parse::<SocketAddr>() {
        return Some(sa.ip());
    }
    s.strip_prefix('[').and_then(|x| x.strip_suffix(']')).and_then(|x| x.parse::<IpAddr>().ok())
}

fn unmap(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

pub fn is_loopback(ip: Option<IpAddr>) -> bool {
    match ip.map(unmap) {
        None => false,
        Some(IpAddr::V4(v4)) => v4.octets()[0] == 127,
        Some(IpAddr::V6(v6)) => v6 == Ipv6Addr::LOCALHOST,
    }
}

/// Loopback, RFC1918, IPv6 link-local (fe80::/10) and unique-local (fc00::/7).
pub fn is_local_or_private(ip: Option<IpAddr>) -> bool {
    match ip.map(unmap) {
        None => false,
        Some(IpAddr::V4(v4)) => {
            let b = v4.octets();
            b[0] == 127 || b[0] == 10 || (b[0] == 172 && (16..=31).contains(&b[1])) || (b[0] == 192 && b[1] == 168)
        }
        Some(IpAddr::V6(v6)) => {
            let b = v6.octets();
            v6 == Ipv6Addr::LOCALHOST || (b[0] == 0xfe && (b[1] & 0xc0) == 0x80) || (b[0] & 0xfe) == 0xfc
        }
    }
}

fn try_parse_header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    let v = headers.get(name)?.to_str().ok()?;
    if v.trim().is_empty() {
        return None;
    }
    v.split(',').next()?.trim().parse::<IpAddr>().ok()
}

/// Resolved view used by the access evaluator.
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct ClientNetworkContext {
    pub client_ip: Option<IpAddr>,
    pub peer_ip: Option<IpAddr>,
    pub is_direct_local_client: bool,
    pub is_via_local_peer: bool,
    /// cloudflared/nginx on 127.0.0.1 - same-host reverse proxy, not a direct LAN client.
    pub is_same_host_reverse_proxy: bool,
    pub is_trusted_context: bool,
}

impl ClientNetworkContext {
    pub fn from_request(net: &RequestNetwork, headers: &HeaderMap) -> Self {
        let client_ip = resolve_client_ip(net, headers);
        let peer_ip = net.peer_ip;
        let is_direct_local_client = is_local_or_private(client_ip);
        let is_via_local_peer = is_local_or_private(peer_ip);
        ClientNetworkContext {
            client_ip,
            peer_ip,
            is_direct_local_client,
            is_via_local_peer,
            is_same_host_reverse_proxy: is_loopback(peer_ip),
            is_trusted_context: is_direct_local_client || is_via_local_peer,
        }
    }
}

/// Proxy identity headers are honoured only when the TCP peer is loopback
/// (Cloudflare Tunnel sends `CF-Connecting-IP`, not always `X-Forwarded-For`).
fn resolve_client_ip(net: &RequestNetwork, headers: &HeaderMap) -> Option<IpAddr> {
    if is_loopback(net.peer_ip) {
        if let Some(ip) = try_parse_header_ip(headers, "cf-connecting-ip") {
            return Some(ip);
        }
        if let Some(ip) = try_parse_header_ip(headers, "x-real-ip") {
            return Some(ip);
        }
    }
    net.remote_ip
}

/// Headers that indicate the request was forwarded by a reverse proxy / tunnel.
pub fn has_proxy_client_identity_headers(headers: &HeaderMap) -> bool {
    [
        "cf-connecting-ip",
        "cf-ray",
        "x-forwarded-for",
        "x-real-ip",
        // Traefik/nginx/Caddy often set these even when client IP headers vary.
        "x-forwarded-host",
        "x-forwarded-proto",
        "forwarded",
    ]
    .iter()
    .any(|h| headers.contains_key(*h))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    pub(crate) fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(HeaderName::from_bytes(k.as_bytes()).unwrap(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    fn ctx(peer: &str, pairs: &[(&str, &str)]) -> ClientNetworkContext {
        let h = headers(pairs);
        ClientNetworkContext::from_request(&RequestNetwork::direct(Some(peer.parse().unwrap())), &h)
    }

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn public_peer_ignores_spoofed_cf_connecting_ip() {
        let n = ctx("203.0.113.10", &[("CF-Connecting-IP", "127.0.0.1")]);
        assert_eq!(n.client_ip, ip("203.0.113.10"));
        assert_eq!(n.peer_ip, ip("203.0.113.10"));
        assert!(!n.is_direct_local_client);
        assert!(!n.is_same_host_reverse_proxy);
    }

    #[test]
    fn public_peer_ignores_spoofed_x_real_ip() {
        let n = ctx("203.0.113.10", &[("X-Real-IP", "192.168.1.1")]);
        assert_eq!(n.client_ip, ip("203.0.113.10"));
        assert!(!n.is_direct_local_client);
    }

    #[test]
    fn loopback_peer_trusts_cf_connecting_ip() {
        let n = ctx("127.0.0.1", &[("CF-Connecting-IP", "8.8.8.8")]);
        assert_eq!(n.client_ip, ip("8.8.8.8"));
        assert_eq!(n.peer_ip, ip("127.0.0.1"));
        assert!(!n.is_direct_local_client);
        assert!(n.is_same_host_reverse_proxy);
    }

    #[test]
    fn direct_loopback_without_headers_is_local_client() {
        let n = ctx("127.0.0.1", &[]);
        assert_eq!(n.client_ip, ip("127.0.0.1"));
        assert!(n.is_direct_local_client);
        assert!(n.is_same_host_reverse_proxy);
    }

    #[test]
    fn direct_lan_without_headers_is_local_client() {
        let n = ctx("192.168.1.50", &[]);
        assert_eq!(n.client_ip, ip("192.168.1.50"));
        assert!(n.is_direct_local_client);
        assert!(!n.is_same_host_reverse_proxy);
    }

    #[test]
    fn xff_applied_only_from_loopback_peer() {
        let h = headers(&[("X-Forwarded-For", "10.0.0.1, 8.8.4.4"), ("X-Forwarded-Proto", "https")]);
        let n = RequestNetwork::capture(ip("127.0.0.1"), &h);
        assert_eq!(n.remote_ip, ip("8.8.4.4"));
        assert_eq!(n.scheme, "https");
        let ctx = ClientNetworkContext::from_request(&n, &h);
        assert_eq!(ctx.client_ip, ip("8.8.4.4"));
        assert!(!ctx.is_direct_local_client);

        let n = RequestNetwork::capture(ip("192.168.1.2"), &h);
        assert_eq!(n.remote_ip, ip("192.168.1.2"));
        assert_eq!(n.scheme, "http");
    }

    #[test]
    fn ipv4_mapped_and_v6_ranges() {
        assert!(is_loopback(ip("::ffff:127.0.0.1")));
        assert!(is_local_or_private(ip("::ffff:10.1.2.3")));
        assert!(is_local_or_private(ip("fe80::1")));
        assert!(is_local_or_private(ip("fd00::1")));
        assert!(!is_local_or_private(ip("2001:db8::1")));
        assert!(!is_local_or_private(ip("172.32.0.1")));
        assert!(is_local_or_private(ip("172.31.0.1")));
    }
}
