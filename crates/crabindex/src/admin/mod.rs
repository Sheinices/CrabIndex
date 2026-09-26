//! Admin panel: entry gate, session login and the admin API, all under `admin.path`
//! (read from the live config on every request, so a saved path applies immediately).
//!
//! WAF endpoints (`{path}/api/waf/…`) are served by [`crate::waf::api`].
//!
//! 1. `GET {path}?{token}` (or `?token={token}`) sets the `crab_gate` cookie (HMAC of the
//!    token with the install secret) and redirects to `{path}/`.
//! 2. Any `{path}` / `{path}/*` request without a valid gate cookie falls through to the
//!    regular pipeline and gets exactly the response of an unknown route.
//! 3. With the gate: `{path}/` serves `wwwroot/admin/index.html` (with the
//!    `crab-admin-base` meta tag), `{path}/*` serves the admin build, `{path}/api/*` is the
//!    admin API (`session`, `login`, `logout` without a session; everything else requires the
//!    `crab_session` cookie and, for mutating methods, `X-Crab-Admin: 1`).
//!
//! Existing handlers (config, cron, dev, jsondb, stats, background jobs) are reused by
//! rewriting the request path and tagging it with [`AdminAuthorized`].
//!
//! Sessions are kept in memory and end on restart; gate cookies survive restarts as long as
//! `Data/temp/admin.secret` and the token stay the same.

pub mod api;
pub mod bootstrap;
pub mod crypto;
pub mod session;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use crab_core::config::AppOptions;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::security::keys::secure_equals;
use crate::security::network::ClientNetworkContext;
use crate::security::{request_network, AdminAuthorized};
use crate::static_files::{resolve_file, ADMIN_DIR, WWWROOT};
use crate::version;

pub const GATE_COOKIE: &str = "crab_gate";
pub const SESSION_COOKIE: &str = "crab_session";
pub const CSRF_HEADER: &str = "x-crab-admin";
/// Gate cookie lifetime (the session cookie uses `admin.sessionHours`).
pub const GATE_MAX_AGE_SECS: u64 = 30 * 24 * 3600;
const MAX_LOGIN_BODY: usize = 16 * 1024;

/// Where a request path falls relative to the admin prefix.
#[derive(Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// `{path}` itself (entry URL).
    Entry,
    /// `{path}/{rest}` (`rest` may be empty).
    Inner(&'a str),
}

pub fn classify<'a>(req_path: &'a str, base: &str) -> Option<Target<'a>> {
    if req_path == base {
        return Some(Target::Entry);
    }
    req_path.strip_prefix(base).and_then(|r| r.strip_prefix('/')).map(Target::Inner)
}

/// Token from `?{token}` or `?token={token}`.
pub fn query_token(raw_query: Option<&str>) -> Option<String> {
    let q = raw_query?;
    if q.contains('=') {
        return url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "token").map(|(_, v)| v.into_owned());
    }
    Some(urlencoding::decode(q).map(|s| s.into_owned()).unwrap_or_else(|_| q.to_string()))
}

/// Values of cookie `name` from every `Cookie` header.
pub fn cookie_values(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(k, _)| k.trim() == name)
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
        .collect()
}

fn gate_ok(headers: &HeaderMap, base: &str, token: &str) -> bool {
    let expected = crypto::gate_value(base, token);
    cookie_values(headers, GATE_COOKIE).iter().any(|v| crypto::ct_eq(v.as_bytes(), expected.as_bytes()))
}

fn has_session(headers: &HeaderMap, c: &AppOptions) -> bool {
    let devkey = c.devkey.as_deref().unwrap_or("");
    cookie_values(headers, SESSION_COOKIE).iter().any(|v| session::is_valid(v, devkey))
}

fn cookie(name: &str, value: &str, base: &str, max_age: u64, secure: bool) -> HeaderValue {
    let s = format!(
        "{name}={value}; Path={base}; Max-Age={max_age}; HttpOnly; SameSite=Strict{}",
        if secure { "; Secure" } else { "" }
    );
    HeaderValue::from_str(&s).unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn no_store(h: &mut HeaderMap) {
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::HeaderName::from_static("x-robots-tag"), HeaderValue::from_static("noindex, nofollow"));
}

pub fn json_response(status: StatusCode, v: Value) -> Response {
    let mut r = (status, serde_json::to_string(&v).unwrap_or_else(|_| "{}".into())).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    no_store(r.headers_mut());
    r
}

fn redirect(to: &str) -> Response {
    let mut r = StatusCode::FOUND.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    no_store(r.headers_mut());
    r
}

fn is_https(req: &Request) -> bool {
    request_network(req).scheme.eq_ignore_ascii_case("https")
}

/// Middleware in front of the static files / API pipeline.
pub async fn admin_mw(req: Request, next: Next) -> Response {
    let c = crate::conf();
    if !c.admin.enable || c.admin.token.is_empty() {
        return next.run(req).await;
    }
    let base = c.admin_path();
    let path = req.uri().path().to_string();
    let Some(target) = classify(&path, &base) else {
        return next.run(req).await;
    };
    let is_get = req.method() == Method::GET || req.method() == Method::HEAD;
    let entry_token_ok = is_get
        && matches!(target, Target::Entry | Target::Inner(""))
        && query_token(req.uri().query()).map(|t| secure_equals(Some(&t), Some(&c.admin.token))).unwrap_or(false);

    // Entry URL: `{path}?{token}` (also tolerated as `{path}/?{token}`).
    if entry_token_ok {
        let mut r = redirect(&format!("{base}/"));
        let v = cookie(GATE_COOKIE, &crypto::gate_value(&base, &c.admin.token), &base, GATE_MAX_AGE_SECS, is_https(&req));
        r.headers_mut().append(header::SET_COOKIE, v);
        return r;
    }
    if !gate_ok(req.headers(), &base, &c.admin.token) {
        // Same response as any unknown route.
        return next.run(req).await;
    }
    match target {
        Target::Entry if is_get => redirect(&format!("{base}/")),
        Target::Entry => next.run(req).await,
        Target::Inner(rest) => {
            if rest == "api" || rest.starts_with("api/") {
                let sub = rest.strip_prefix("api").unwrap_or("").trim_start_matches('/').to_string();
                api_request(req, next, &c, &base, &sub).await
            } else if is_get {
                serve_ui(req, &base, rest).await
            } else {
                next.run(req).await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

/// Insert `<base href="{base}/">` and `<meta name="crab-admin-base" content="{base}">` right after
/// `<head…>`. The `<base>` tag makes the build's relative asset URLs (`./assets/…`) resolve from the
/// admin root even on nested routes such as `{base}/waf/ips`.
pub fn inject_base_meta(html: &str, base: &str) -> String {
    let esc = crate::controllers::home::xml_escape(base);
    let tag = format!("<base href=\"{esc}/\"><meta name=\"crab-admin-base\" content=\"{esc}\">");
    let lower = html.to_ascii_lowercase();
    let head_end = lower
        .match_indices("<head")
        .find(|(i, _)| matches!(lower.as_bytes().get(i + 5), Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')))
        .and_then(|(i, _)| lower[i..].find('>').map(|j| i + j + 1));
    match head_end {
        Some(at) => format!("{}\n    {tag}{}", &html[..at], &html[at..]),
        None => format!("{tag}\n{html}"),
    }
}

fn admin_root() -> PathBuf {
    Path::new(WWWROOT).join(ADMIN_DIR)
}

async fn index_response(root: &Path, base: &str) -> Response {
    let file = root.join("index.html");
    let mut r = match tokio::fs::read_to_string(&file).await {
        Ok(html) => {
            let mut r = inject_base_meta(&html, base).into_response();
            r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
            r
        }
        Err(e) => {
            crab_core::log::warn("admin", format!("{}: {e}", file.display()));
            let mut r = (StatusCode::SERVICE_UNAVAILABLE, "admin UI is not built (wwwroot/admin/index.html is missing)").into_response();
            r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
            r
        }
    };
    no_store(r.headers_mut());
    r
}

async fn serve_ui(req: Request, base: &str, rest: &str) -> Response {
    let root = admin_root();
    if rest.is_empty() || rest == "index.html" {
        return index_response(&root, base).await;
    }
    if let Some(file) = resolve_file(&root, &format!("/{rest}")) {
        if file.file_name().map(|n| n == "index.html").unwrap_or(false) {
            return index_response(&root, base).await;
        }
        let mut resp = match ServeFile::new(&file).oneshot(req).await {
            Ok(r) => r.map(Body::new),
            Err(never) => match never {},
        };
        resp.headers_mut().insert(header::HeaderName::from_static("x-robots-tag"), HeaderValue::from_static("noindex, nofollow"));
        return resp;
    }
    let last = rest.rsplit('/').next().unwrap_or("");
    if !last.contains('.') {
        // client-side route of the admin SPA
        return index_response(&root, base).await;
    }
    let mut r = StatusCode::NOT_FOUND.into_response();
    no_store(r.headers_mut());
    r
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// Internal path of an admin API sub-path handled by an existing router entry.
pub fn internal_path(sub: &str) -> Option<String> {
    let (head, tail) = match sub.split_once('/') {
        Some((h, t)) => (h, Some(t)),
        None => (sub, None),
    };
    let tail_nonempty = tail.filter(|t| !t.is_empty());
    match (head.to_ascii_lowercase().as_str(), tail_nonempty) {
        ("config", None) => Some("/api/v1.0/config".into()),
        ("config", Some(t)) => Some(format!("/api/v1.0/config/{t}")),
        ("cron", Some(t)) => Some(format!("/cron/{t}")),
        ("dev", Some(t)) => Some(format!("/dev/{t}")),
        ("jsondb", None) => Some("/jsondb".into()),
        ("jsondb", Some(t)) => Some(format!("/jsondb/{t}")),
        ("stats", Some(t)) => Some(format!("/stats/{t}")),
        ("health", Some(t)) if t.eq_ignore_ascii_case("background-jobs") => Some("/health/background-jobs".into()),
        _ => None,
    }
}

fn is_mutating(m: &Method) -> bool {
    !(m == Method::GET || m == Method::HEAD || m == Method::OPTIONS)
}

fn client_key(req: &Request) -> String {
    let net = request_network(req);
    ClientNetworkContext::from_request(&net, req.headers()).client_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "unknown".into())
}

async fn api_request(mut req: Request, next: Next, c: &AppOptions, base: &str, sub: &str) -> Response {
    // Browsers mark cross-site / sibling-subdomain requests; the admin API is same-origin only.
    if let Some(site) = req.headers().get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site.eq_ignore_ascii_case("cross-site") || site.eq_ignore_ascii_case("same-site") {
            return json_response(StatusCode::FORBIDDEN, json!({ "error": "forbidden" }));
        }
    }
    if is_mutating(req.method()) && req.headers().get(CSRF_HEADER).map(|v| v.as_bytes() != b"1").unwrap_or(true) {
        return json_response(StatusCode::FORBIDDEN, json!({ "error": "missing X-Crab-Admin header" }));
    }
    let method = req.method().clone();
    let devkey = c.devkey.as_deref().unwrap_or("");
    match (method.as_str(), sub) {
        ("GET", "session") => {
            return json_response(
                StatusCode::OK,
                json!({ "authenticated": has_session(req.headers(), c), "version": version::VERSION, "loginEnabled": !devkey.is_empty() }),
            );
        }
        ("POST", "login") => return login(req, c, base).await,
        ("POST", "logout") => {
            for v in cookie_values(req.headers(), SESSION_COOKIE) {
                session::destroy(&v);
            }
            let mut r = json_response(StatusCode::OK, json!({ "ok": true }));
            r.headers_mut().append(header::SET_COOKIE, cookie(SESSION_COOKIE, "", base, 0, is_https(&req)));
            return r;
        }
        _ => {}
    }
    if !has_session(req.headers(), c) {
        return json_response(StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" }));
    }
    if sub == "waf" || sub.starts_with("waf/") {
        return crate::waf::api::handle(req, sub.strip_prefix("waf").unwrap_or("").trim_start_matches('/')).await;
    }
    match (method.as_str(), sub) {
        ("GET", "overview") => {
            let c2 = crate::conf();
            return match tokio::task::spawn_blocking(move || api::overview(&c2)).await {
                Ok(v) => json_response(StatusCode::OK, v),
                Err(_) => crate::app::internal_error_response(),
            };
        }
        ("GET", "update") => {
            let force = req.uri().query().map(|q| q.split('&').any(|kv| kv == "force=1" || kv == "force=true")).unwrap_or(false);
            return json_response(StatusCode::OK, crate::update::check(force).await);
        }
        ("POST", "update/apply") => {
            return match crate::update::apply().await {
                Ok(v) => json_response(StatusCode::OK, v),
                Err(e) => json_response(StatusCode::OK, json!({ "ok": false, "error": e })),
            };
        }
        ("GET", "logs") => {
            let v = tokio::task::spawn_blocking(|| api::list_logs(Path::new(api::LOG_DIR))).await.unwrap_or_default();
            return json_response(StatusCode::OK, Value::Array(v));
        }
        ("GET", s) if s.starts_with("logs/") => {
            let name = s["logs/".len()..].to_string();
            let n = api::tail_count(req.uri().query());
            let Some(file) = api::log_file(Path::new(api::LOG_DIR), &name) else {
                return json_response(StatusCode::NOT_FOUND, json!({ "error": "not found" }));
            };
            return match tokio::task::spawn_blocking(move || api::tail_lines(&file, n)).await {
                Ok(Ok(lines)) => json_response(StatusCode::OK, json!({ "name": name, "lines": lines })),
                _ => json_response(StatusCode::NOT_FOUND, json!({ "error": "not found" })),
            };
        }
        _ => {}
    }
    let Some(target) = internal_path(sub) else {
        return json_response(StatusCode::NOT_FOUND, json!({ "error": "not found" }));
    };
    let pq = match req.uri().query() {
        Some(q) => format!("{target}?{q}"),
        None => target,
    };
    let mut parts = req.uri().clone().into_parts();
    parts.path_and_query = match pq.parse() {
        Ok(p) => Some(p),
        Err(_) => return json_response(StatusCode::BAD_REQUEST, json!({ "error": "bad path" })),
    };
    match Uri::from_parts(parts) {
        Ok(u) => *req.uri_mut() = u,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, json!({ "error": "bad path" })),
    }
    req.extensions_mut().insert(AdminAuthorized);
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn login(req: Request, c: &AppOptions, base: &str) -> Response {
    let client = client_key(&req);
    let secure = is_https(&req);
    if session::is_rate_limited(&client) {
        crab_core::log::warn("admin", format!("login rate limited for {client}"));
        let mut r = json_response(StatusCode::TOO_MANY_REQUESTS, json!({ "ok": false, "error": "too many attempts" }));
        r.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(session::FAILURE_WINDOW.as_secs()));
        return r;
    }
    let body = axum::body::to_bytes(req.into_body(), MAX_LOGIN_BODY).await.unwrap_or_default();
    let supplied = serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["devkey"].as_str().map(str::to_string));
    let devkey = c.devkey.as_deref().unwrap_or("");
    let ok = !devkey.is_empty() && supplied.as_deref().map(|s| secure_equals(Some(s), Some(devkey))).unwrap_or(false);
    if !ok {
        session::record_failure(&client);
        crab_core::log::warn("admin", format!("login failed from {client}"));
        return json_response(StatusCode::UNAUTHORIZED, json!({ "ok": false, "error": "invalid key" }));
    }
    session::clear_failures(&client);
    let hours = c.admin.sessionHours.clamp(1, 8760);
    let token = session::create(devkey, hours);
    crab_core::log::info("admin", format!("login from {client}"));
    let mut r = json_response(StatusCode::OK, json!({ "ok": true }));
    r.headers_mut().append(header::SET_COOKIE, cookie(SESSION_COOKIE, &token, base, hours as u64 * 3600, secure));
    r
}

#[cfg(test)]
mod tests;
