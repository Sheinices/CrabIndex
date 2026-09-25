//! Access decision for a request path.

use axum::http::{HeaderMap, Method};

use super::keys::{ApiKeyValidator, DevKeyValidator};
use super::network::{has_proxy_client_identity_headers, ClientNetworkContext, RequestNetwork};
use super::registry::{is_restricted_admin_path, resolve_policy, AccessPolicy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessResult {
    pub is_allowed: bool,
    pub deny_status_code: u16,
    pub set_private_network_header_on_deny: bool,
}

impl AccessResult {
    pub const ALLOW: AccessResult = AccessResult { is_allowed: true, deny_status_code: 0, set_private_network_header_on_deny: false };

    pub fn deny(status: u16, set_private_network_header: bool) -> Self {
        AccessResult { is_allowed: false, deny_status_code: status, set_private_network_header_on_deny: set_private_network_header }
    }
}

/// Request facts needed for a decision.
pub struct RequestView<'a> {
    pub method: &'a Method,
    pub headers: &'a HeaderMap,
    pub raw_query: Option<&'a str>,
    pub network: &'a RequestNetwork,
}

/// Keys currently configured (`apikey`, `devkey`).
#[derive(Clone, Copy, Default)]
pub struct KeyConfig<'a> {
    pub apikey: Option<&'a str>,
    pub devkey: Option<&'a str>,
}

pub fn deny_status(key_configured: bool, method: &Method) -> u16 {
    if method == Method::OPTIONS {
        204
    } else if key_configured {
        401
    } else {
        403
    }
}

pub fn should_set_private_network_header(network: &ClientNetworkContext, path: &str) -> bool {
    network.is_trusted_context || !is_restricted_admin_path(path)
}

pub fn evaluate_path(path: &str, req: &RequestView<'_>, keys: KeyConfig<'_>) -> AccessResult {
    let policy = resolve_policy(path);
    let network = ClientNetworkContext::from_request(req.network, req.headers);

    match policy {
        // Only reachable through the admin panel, which bypasses this evaluation.
        AccessPolicy::ConfigApi => AccessResult::deny(404, false),
        AccessPolicy::DevAdmin => evaluate_dev_admin(&network, req, keys),
        AccessPolicy::ApiKeyWhenConfigured => {
            let v = ApiKeyValidator { configured: keys.apikey };
            if !v.is_configured() || v.validate(req.headers, req.raw_query) {
                return AccessResult::ALLOW;
            }
            AccessResult::deny(deny_status(true, req.method), should_set_private_network_header(&network, path))
        }
        AccessPolicy::Public => AccessResult::ALLOW,
    }
}

fn evaluate_dev_admin(network: &ClientNetworkContext, req: &RequestView<'_>, keys: KeyConfig<'_>) -> AccessResult {
    if is_dev_endpoint_access_allowed(network, req, keys) {
        return AccessResult::ALLOW;
    }
    let dev = DevKeyValidator { configured: keys.devkey };
    AccessResult::deny(deny_status(dev.is_configured(), req.method), true)
}

fn is_dev_endpoint_access_allowed(network: &ClientNetworkContext, req: &RequestView<'_>, keys: KeyConfig<'_>) -> bool {
    if is_trusted_lan_client(network, req.headers) {
        return true;
    }
    // Empty/missing devkey = LAN-only; never world-open.
    let dev = DevKeyValidator { configured: keys.devkey };
    dev.is_configured() && dev.validate(req.headers, req.raw_query)
}

/// LAN / direct localhost. A reverse proxy (loopback or Docker/LAN peer) alone is not enough:
/// with proxy identity headers present the real client is behind the proxy and needs the DEV key.
fn is_trusted_lan_client(network: &ClientNetworkContext, headers: &HeaderMap) -> bool {
    if !network.is_direct_local_client {
        return false;
    }
    if network.is_via_local_peer && has_proxy_client_identity_headers(headers) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::network::tests::headers;

    const CONFIG_PATH: &str = "/jsondb/save";
    const DEV_PATH: &str = "/dev/";

    fn eval(path: &str, peer: &str, pairs: &[(&str, &str)], devkey: Option<&str>) -> AccessResult {
        let h = headers(pairs);
        let net = RequestNetwork::direct(Some(peer.parse().unwrap()));
        let req = RequestView { method: &Method::GET, headers: &h, raw_query: None, network: &net };
        evaluate_path(path, &req, KeyConfig { apikey: None, devkey })
    }

    #[test]
    fn spoofed_cf_connecting_ip_from_public_peer_is_denied_without_devkey() {
        let r = eval(CONFIG_PATH, "203.0.113.10", &[("CF-Connecting-IP", "127.0.0.1")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn loopback_peer_with_public_cf_ip_requires_devkey() {
        let r = eval(CONFIG_PATH, "127.0.0.1", &[("CF-Connecting-IP", "8.8.8.8")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn direct_loopback_without_headers_is_allowed() {
        assert!(eval(CONFIG_PATH, "127.0.0.1", &[], None).is_allowed);
        assert!(eval(DEV_PATH, "127.0.0.1", &[], None).is_allowed);
    }

    #[test]
    fn direct_lan_without_headers_is_allowed() {
        assert!(eval(CONFIG_PATH, "10.0.0.5", &[], None).is_allowed);
    }

    #[test]
    fn empty_devkey_denies_public_peer() {
        let r = eval(CONFIG_PATH, "203.0.113.10", &[], Some(""));
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn valid_x_dev_key_allows_public_peer() {
        let pairs = [("X-Dev-Key", "secret-dev-key")];
        assert!(eval(CONFIG_PATH, "203.0.113.10", &pairs, Some("secret-dev-key")).is_allowed);
        assert!(eval(DEV_PATH, "203.0.113.10", &pairs, Some("secret-dev-key")).is_allowed);
    }

    #[test]
    fn invalid_x_dev_key_denies_public_peer_with_401() {
        let r = eval(CONFIG_PATH, "203.0.113.10", &[("X-Dev-Key", "wrong")], Some("secret-dev-key"));
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 401);
    }

    #[test]
    fn loopback_proxy_with_valid_devkey_allows_public_client() {
        let pairs = [("CF-Connecting-IP", "8.8.8.8"), ("X-Dev-Key", "secret-dev-key")];
        assert!(eval(CONFIG_PATH, "127.0.0.1", &pairs, Some("secret-dev-key")).is_allowed);
    }

    #[test]
    fn docker_proxy_peer_with_xff_requires_devkey() {
        let r = eval(CONFIG_PATH, "172.18.0.2", &[("X-Forwarded-For", "203.0.113.50")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn docker_proxy_peer_with_x_real_ip_requires_devkey() {
        let r = eval(CONFIG_PATH, "172.18.0.2", &[("X-Real-IP", "203.0.113.50")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn docker_proxy_peer_with_xff_and_valid_devkey_allows() {
        let pairs = [("X-Forwarded-For", "203.0.113.50"), ("X-Dev-Key", "secret-dev-key")];
        assert!(eval(CONFIG_PATH, "172.18.0.2", &pairs, Some("secret-dev-key")).is_allowed);
        assert!(eval(DEV_PATH, "172.18.0.2", &pairs, Some("secret-dev-key")).is_allowed);
    }

    #[test]
    fn docker_bridge_peer_without_proxy_headers_is_allowed() {
        assert!(eval(CONFIG_PATH, "172.18.0.2", &[], None).is_allowed);
        assert!(eval(DEV_PATH, "172.18.0.2", &[], None).is_allowed);
    }

    #[test]
    fn docker_proxy_peer_with_x_forwarded_host_requires_devkey() {
        let r = eval(CONFIG_PATH, "172.18.0.2", &[("X-Forwarded-Host", "crabindex.example.com")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn docker_proxy_peer_with_forwarded_header_requires_devkey() {
        let r = eval(CONFIG_PATH, "172.18.0.2", &[("Forwarded", "for=203.0.113.50;proto=https")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn docker_proxy_peer_with_x_forwarded_proto_requires_devkey() {
        let r = eval(CONFIG_PATH, "172.18.0.2", &[("X-Forwarded-Proto", "http")], None);
        assert!(!r.is_allowed);
        assert_eq!(r.deny_status_code, 403);
    }

    #[test]
    fn config_api_is_not_public() {
        for peer in ["127.0.0.1", "10.0.0.5", "203.0.113.10"] {
            let r = eval("/api/v1.0/config", peer, &[("X-Dev-Key", "k")], Some("k"));
            assert_eq!((r.is_allowed, r.deny_status_code, r.set_private_network_header_on_deny), (false, 404, false));
        }
    }

    #[test]
    fn apikey_policy() {
        let h = headers(&[]);
        let net = RequestNetwork::direct(Some("203.0.113.10".parse().unwrap()));
        let keys = KeyConfig { apikey: Some("k"), devkey: None };
        let ok = RequestView { method: &Method::GET, headers: &h, raw_query: Some("apikey=k"), network: &net };
        assert!(evaluate_path("/api/v1.0/torrents", &ok, keys).is_allowed);
        let bad = RequestView { method: &Method::GET, headers: &h, raw_query: None, network: &net };
        let r = evaluate_path("/api/v1.0/torrents", &bad, keys);
        assert_eq!((r.is_allowed, r.deny_status_code, r.set_private_network_header_on_deny), (false, 401, true));
        let opt = RequestView { method: &Method::OPTIONS, headers: &h, raw_query: None, network: &net };
        assert_eq!(evaluate_path("/api/v1.0/torrents", &opt, keys).deny_status_code, 204);
        assert!(evaluate_path("/health", &bad, keys).is_allowed);
    }
}
