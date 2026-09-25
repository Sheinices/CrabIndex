//! Request security: client network context, key validation, access policies,
//! security headers and the authorization middleware.

pub mod evaluator;
pub mod keys;
pub mod network;
pub mod registry;

use axum::extract::{ConnectInfo, Request};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use crab_core::log::{self, Level};
use std::net::SocketAddr;
use std::time::Instant;

use evaluator::{evaluate_path, should_set_private_network_header, KeyConfig, RequestView};
use network::{ClientNetworkContext, RequestNetwork};

/// First middleware after CORS/compression: remember the TCP peer and apply trusted forwarded headers.
pub async fn capture_network(mut req: Request, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let net = RequestNetwork::capture(peer, req.headers());
    req.extensions_mut().insert(net);
    next.run(req).await
}

pub fn request_network(req: &Request) -> RequestNetwork {
    req.extensions().get::<RequestNetwork>().cloned().unwrap_or_else(|| RequestNetwork::direct(None))
}

const CSP: &str = "default-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:; connect-src 'self' https: http:; manifest-src 'self'; worker-src 'self'";

/// Documentation (mdBook) needs inline scripts for its theme/sidebar bootstrap.
const DOCS_CSP: &str = "default-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'; object-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data:; connect-src 'self'";

/// Security headers for a response to `path` (CSP omitted for Swagger UI / openapi.yaml,
/// relaxed for the /docs book).
pub fn security_headers(path: &str) -> Vec<(&'static str, &'static str)> {
    let mut v = vec![
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "strict-origin-when-cross-origin"),
        ("x-frame-options", "SAMEORIGIN"),
        ("permissions-policy", "camera=(), microphone=(), geolocation=()"),
    ];
    let lower = path.to_ascii_lowercase();
    if lower == "/docs" || lower.starts_with("/docs/") {
        v.push(("content-security-policy", DOCS_CSP));
    } else if !(lower.starts_with("/swagger") || lower == "/openapi.yaml") {
        v.push(("content-security-policy", CSP));
    }
    v
}

fn apply_headers_if_absent(headers: &mut HeaderMap, list: &[(&'static str, &'static str)]) {
    for (k, v) in list {
        let name = HeaderName::from_static(k);
        if !headers.contains_key(&name) {
            headers.insert(name, HeaderValue::from_static(v));
        }
    }
}

/// Adds security headers unless the handler already set them.
pub async fn security_headers_mw(req: Request, next: Next) -> Response {
    let list = security_headers(req.uri().path());
    let mut resp = next.run(req).await;
    apply_headers_if_absent(resp.headers_mut(), &list);
    resp
}

fn set_private_network_header(headers: &mut HeaderMap) {
    headers.insert(HeaderName::from_static("access-control-allow-private-network"), HeaderValue::from_static("true"));
}

/// Request extension set by the admin panel middleware on requests it rewrote after checking
/// the gate cookie and the admin session. Such requests skip the key/network policies.
#[derive(Clone, Copy, Debug)]
pub struct AdminAuthorized;

/// Enforces access policies and logs `/cron/` requests with their duration.
pub async fn authorization(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let admin = req.extensions().get::<AdminAuthorized>().is_some();
    let net = request_network(&req);
    let network = ClientNetworkContext::from_request(&net, req.headers());

    if !admin {
        let conf = crate::conf();
        let keys = KeyConfig { apikey: conf.apikey.as_deref(), devkey: conf.devkey.as_deref() };
        let view = RequestView { method: req.method(), headers: req.headers(), raw_query: req.uri().query(), network: &net };
        let result = evaluate_path(&path, &view, keys);
        if !result.is_allowed {
            let mut resp = StatusCode::from_u16(result.deny_status_code).unwrap_or(StatusCode::FORBIDDEN).into_response();
            if result.set_private_network_header_on_deny {
                set_private_network_header(resp.headers_mut());
            }
            return resp;
        }
    }

    let set_pna = !admin && should_set_private_network_header(&network, &path);
    let is_cron = path.len() >= 6 && path[..6].eq_ignore_ascii_case("/cron/");
    let started = Instant::now();

    let mut resp = next.run(req).await;
    if set_pna {
        set_private_network_header(resp.headers_mut());
    }
    if is_cron {
        log_cron_request(&path, started.elapsed().as_millis() as i64, resp.status().as_u16());
    }
    resp
}

/// `cron: [HH:mm:ss] rutor/parse 1.2s 200` - fast 200s go to Debug (logging.cronSkipFastMs).
pub fn cron_log_line(path: &str, elapsed_ms: i64, status: u16, skip_fast_ms: i32) -> (Level, String) {
    let label = path.get(6..).unwrap_or("");
    let elapsed = if elapsed_ms >= 1000 { format!("{:.1}s", elapsed_ms as f64 / 1000.0) } else { format!("{elapsed_ms}ms") };
    let fail = if status >= 400 { " FAIL" } else { "" };
    let level = if status == 200 && skip_fast_ms > 0 && elapsed_ms < skip_fast_ms as i64 { Level::Debug } else { Level::Information };
    let ts = chrono::Local::now().format("%H:%M:%S");
    (level, format!("[{ts}] {label} {elapsed} {status}{fail}"))
}

fn log_cron_request(path: &str, elapsed_ms: i64, status: u16) {
    let (level, line) = cron_log_line(path, elapsed_ms, status, log::settings().cron_skip_fast_ms);
    log::write(log::cat::CRON_HTTP, level, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_line_format() {
        let (lvl, line) = cron_log_line("/cron/rutor/parse", 1500, 200, 100);
        assert_eq!(lvl, Level::Information);
        assert!(line.ends_with("] rutor/parse 1.5s 200"), "{line}");
        let (lvl, line) = cron_log_line("/cron/rutor/parse", 5, 200, 100);
        assert_eq!(lvl, Level::Debug);
        assert!(line.ends_with(" 5ms 200"));
        let (lvl, line) = cron_log_line("/cron/x", 5, 500, 100);
        assert_eq!(lvl, Level::Information);
        assert!(line.ends_with("x 5ms 500 FAIL"));
        let (lvl, _) = cron_log_line("/cron/x", 5, 200, 0);
        assert_eq!(lvl, Level::Information);
    }

    #[test]
    fn csp_skipped_for_swagger() {
        assert!(security_headers("/swagger/index.html").iter().all(|(k, _)| *k != "content-security-policy"));
        assert!(security_headers("/OpenApi.yaml").iter().all(|(k, _)| *k != "content-security-policy"));
        assert!(security_headers("/").iter().any(|(k, _)| *k == "content-security-policy"));
        assert!(security_headers("/docs/index.html").contains(&("content-security-policy", DOCS_CSP)));
    }
}
