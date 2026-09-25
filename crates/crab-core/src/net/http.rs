//! HTTP client.
//!
//! * Proxies: `globalproxy` (regex pattern on URL, all entries tried in random order),
//!   else `proxy.list` when `useproxy` is set.
//! * TLS certificate errors are ignored.
//! * GET falls back to the Cloudflare solver on `cf-mitigated` / interstitial bodies.
//! * Failures return `None`; cancellation via `Req::cancel`.

use dashmap::DashMap;
use encoding_rs::Encoding;
use once_cell::sync::Lazy;
use rand::seq::SliceRandom;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::cf;
use crate::config::ProxySettings;
use crate::conf;
use crate::log::{self, cat};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/111.0.0.0 Safari/537.36";

/// windows-1251.
pub fn cp1251() -> &'static Encoding {
    encoding_rs::WINDOWS_1251
}

/// Request options for GET/POST/download.
#[derive(Clone)]
pub struct Req {
    /// Decode body with this encoding instead of charset/UTF-8.
    pub encoding: Option<&'static Encoding>,
    pub cookie: Option<String>,
    pub referer: Option<String>,
    /// Seconds (default 15; download default should be 30).
    pub timeout: u64,
    pub headers: Vec<(String, String)>,
    /// Max response bytes (0 = 10MB).
    pub max_size: usize,
    pub useproxy: bool,
    /// Explicit proxy URL overriding config (`proxy:` parameter).
    pub proxy: Option<String>,
    /// Follow redirects (default true).
    pub redirect: bool,
    pub http2: bool,
    pub cancel: Option<CancellationToken>,
}

impl Default for Req {
    fn default() -> Self {
        Req {
            encoding: None,
            cookie: None,
            referer: None,
            timeout: 15,
            headers: Vec::new(),
            max_size: 0,
            useproxy: false,
            proxy: None,
            redirect: true,
            http2: false,
            cancel: None,
        }
    }
}

impl Req {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn encoding(mut self, e: &'static Encoding) -> Self {
        self.encoding = Some(e);
        self
    }
    pub fn cp1251(self) -> Self {
        self.encoding(cp1251())
    }
    pub fn cookie(mut self, c: impl Into<String>) -> Self {
        let c = c.into();
        self.cookie = if c.is_empty() { None } else { Some(c) };
        self
    }
    pub fn cookie_opt(mut self, c: Option<impl Into<String>>) -> Self {
        self.cookie = c.map(Into::into).filter(|s: &String| !s.is_empty());
        self
    }
    pub fn referer(mut self, r: impl Into<String>) -> Self {
        self.referer = Some(r.into());
        self
    }
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout = secs;
        self
    }
    pub fn header(mut self, name: impl Into<String>, val: impl Into<String>) -> Self {
        self.headers.push((name.into(), val.into()));
        self
    }
    pub fn headers(mut self, h: Vec<(String, String)>) -> Self {
        self.headers.extend(h);
        self
    }
    pub fn max_size(mut self, n: usize) -> Self {
        self.max_size = n;
        self
    }
    pub fn useproxy(mut self, v: bool) -> Self {
        self.useproxy = v;
        self
    }
    pub fn proxy(mut self, p: impl Into<String>) -> Self {
        self.proxy = Some(p.into());
        self
    }
    pub fn no_redirect(mut self) -> Self {
        self.redirect = false;
        self
    }
    pub fn http2(mut self) -> Self {
        self.http2 = true;
        self
    }
    pub fn cancel(mut self, ct: &CancellationToken) -> Self {
        self.cancel = Some(ct.clone());
        self
    }
}

/// Status / headers / final URL of a response (`HttpResponseMessage` subset).
#[derive(Clone, Debug)]
pub struct BaseResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub url: String,
}

impl BaseResponse {
    fn failed(url: &str) -> Self {
        BaseResponse { status: 500, headers: HeaderMap::new(), url: url.to_string() }
    }
    fn ok_synthetic(url: &str) -> Self {
        BaseResponse { status: 200, headers: HeaderMap::new(), url: url.to_string() }
    }
    pub fn is_ok(&self) -> bool {
        self.status == 200
    }
    /// All `Set-Cookie` header values.
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(|s| s.to_string())).collect()
    }
    /// `name=value` pairs from Set-Cookie joined with "; ".
    pub fn cookie_header(&self) -> String {
        self.set_cookies()
            .iter()
            .filter_map(|c| c.split(';').next().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("; ")
    }
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProxySpec {
    url: String,
    auth: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ClientKey {
    proxy: Option<ProxySpec>,
    redirect: bool,
    http2: bool,
}

static CLIENTS: Lazy<DashMap<ClientKey, Client>> = Lazy::new(DashMap::new);

/// socks5h:// keeps DNS on the proxy side (needed for .onion); bare host:port → http://.
pub fn normalize_proxy_address(proxy_url: &str) -> String {
    let p = proxy_url.trim();
    let lower = p.to_ascii_lowercase();
    if lower.starts_with("socks5h://") {
        p.to_string()
    } else if let Some(rest) = strip_prefix_ci(p, "socks5://") {
        format!("socks5h://{rest}")
    } else if let Some(rest) = strip_prefix_ci(p, "socks://") {
        format!("socks5h://{rest}")
    } else if !p.contains("://") {
        format!("http://{p}")
    } else {
        p.to_string()
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn spec_from(url: &str, settings: Option<&ProxySettings>) -> ProxySpec {
    let auth = settings.filter(|s| s.useAuth).map(|s| (s.username.clone().unwrap_or_default(), s.password.clone().unwrap_or_default()));
    ProxySpec { url: normalize_proxy_address(url), auth }
}

fn resolve_proxies(url: &str, useproxy: bool, explicit: Option<&str>) -> Vec<Option<ProxySpec>> {
    if let Some(p) = explicit {
        return vec![Some(spec_from(p, None))];
    }
    let c = conf();
    if let Some(gp) = &c.globalproxy {
        for p in gp {
            let Some(list) = p.list.as_ref().filter(|l| !l.is_empty()) else { continue };
            let pattern = p.pattern.as_deref().unwrap_or("");
            if !crate::rx::is_match_i(url, pattern) {
                continue;
            }
            let mut uniq: Vec<String> = Vec::new();
            for u in list.iter().filter(|u| !u.trim().is_empty()) {
                if !uniq.iter().any(|x| x.eq_ignore_ascii_case(u)) {
                    uniq.push(u.clone());
                }
            }
            uniq.shuffle(&mut rand::thread_rng());
            return uniq.into_iter().map(|u| Some(spec_from(&u, Some(p)))).collect();
        }
    }
    if useproxy {
        if let Some(list) = c.proxy.list.as_ref().filter(|l| !l.is_empty()) {
            let pick = list.choose(&mut rand::thread_rng()).cloned().unwrap_or_default();
            return vec![Some(spec_from(&pick, Some(&c.proxy)))];
        }
    }
    vec![None]
}

fn client_for(proxy: Option<ProxySpec>, redirect: bool, http2: bool) -> Option<Client> {
    let key = ClientKey { proxy: proxy.clone(), redirect, http2 };
    if let Some(c) = CLIENTS.get(&key) {
        return Some(c.clone());
    }
    let mut b = Client::builder()
        .danger_accept_invalid_certs(true)
        .gzip(true)
        .brotli(true)
        .deflate(true)
        .connect_timeout(Duration::from_secs(20))
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(if redirect { reqwest::redirect::Policy::limited(10) } else { reqwest::redirect::Policy::none() });
    if !http2 {
        b = b.http1_only();
    }
    if let Some(p) = &proxy {
        let mut px = reqwest::Proxy::all(&p.url).ok()?;
        if let Some((u, pw)) = &p.auth {
            px = px.basic_auth(u, pw);
        }
        b = b.proxy(px);
    } else {
        b = b.no_proxy();
    }
    let client = b.build().ok()?;
    CLIENTS.insert(key, client.clone());
    Some(client)
}

/// A shared reqwest client honouring proxy settings - for custom flows (login forms,
/// multipart, cookie jars). Build your own `Client` if you need a cookie store.
pub fn raw_client(url: &str, useproxy: bool, redirect: bool) -> Option<Client> {
    let px = resolve_proxies(url, useproxy, None).into_iter().next().flatten();
    client_for(px, redirect, false)
}

fn build_headers(o: &Req) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(reqwest::header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    if let Some(c) = &o.cookie {
        if let Ok(v) = HeaderValue::from_str(c) {
            h.insert(reqwest::header::COOKIE, v);
        }
    }
    if let Some(r) = &o.referer {
        if let Ok(v) = HeaderValue::from_str(r) {
            h.insert(reqwest::header::REFERER, v);
        }
    }
    for (k, v) in &o.headers {
        if let (Ok(n), Ok(v)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            h.append(n, v);
        }
    }
    h
}

async fn read_body(resp: reqwest::Response, max: usize) -> Option<Vec<u8>> {
    let max = if max == 0 { 10_000_000 } else { max };
    if let Some(len) = resp.content_length() {
        if len as usize > max {
            return None;
        }
    }
    let mut resp = resp;
    let mut buf = Vec::new();
    while let Ok(Some(chunk)) = resp.chunk().await {
        buf.extend_from_slice(&chunk);
        if buf.len() > max {
            return None;
        }
    }
    Some(buf)
}

fn charset_of(headers: &HeaderMap) -> Option<&'static Encoding> {
    let ct = headers.get(reqwest::header::CONTENT_TYPE)?.to_str().ok()?.to_ascii_lowercase();
    let cs = ct.split(';').find_map(|p| p.trim().strip_prefix("charset=").map(|s| s.trim_matches('"').to_string()))?;
    Encoding::for_label(cs.as_bytes())
}

fn decode(bytes: &[u8], explicit: Option<&'static Encoding>, headers: &HeaderMap) -> String {
    let enc = explicit.or_else(|| charset_of(headers)).unwrap_or(encoding_rs::UTF_8);
    let (s, _, _) = enc.decode(bytes);
    s.into_owned()
}

async fn with_cancel<T>(ct: &Option<CancellationToken>, fut: impl std::future::Future<Output = T>) -> Option<T> {
    match ct {
        Some(ct) => tokio::select! {
            _ = ct.cancelled() => None,
            r = fut => Some(r),
        },
        None => Some(fut.await),
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default()
}

static NON_CF_BLOCK_LOGGED: Lazy<DashMap<String, bool>> = Lazy::new(DashMap::new);

/// GET returning body + response meta.
pub async fn base_get(url: &str, o: &Req) -> (Option<String>, BaseResponse) {
    let host = host_of(url);

    if cf::is_guarded(&host) {
        return match cf::fetch(url, o.cookie.as_deref(), o.referer.as_deref(), &o.headers).await {
            Some(b) => (Some(b), BaseResponse::ok_synthetic(url)),
            None => (None, BaseResponse::failed(url)),
        };
    }

    let mut last = BaseResponse::failed(url);
    for px in resolve_proxies(url, o.useproxy, o.proxy.as_deref()) {
        if o.cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false) {
            return (None, last);
        }
        let Some(client) = client_for(px, o.redirect, o.http2) else { continue };
        let req = client.request(Method::GET, url).headers(build_headers(o)).timeout(Duration::from_secs(o.timeout.max(1)));
        let Some(Ok(resp)) = with_cancel(&o.cancel, req.send()).await else { continue };

        let status = resp.status();
        let meta = BaseResponse { status: status.as_u16(), headers: resp.headers().clone(), url: resp.url().to_string() };

        if status == StatusCode::OK {
            let Some(Some(bytes)) = with_cancel(&o.cancel, read_body(resp, o.max_size)).await else { continue };
            let body = decode(&bytes, o.encoding, &meta.headers);
            if body.trim().is_empty() {
                continue;
            }
            if cf::is_challenge_body(&body) {
                cf::mark_guarded(&host);
                if let Some(b) = cf::fetch(url, o.cookie.as_deref(), o.referer.as_deref(), &o.headers).await {
                    return (Some(b), BaseResponse::ok_synthetic(url));
                }
                continue;
            }
            cf::unguard(&host);
            return (Some(body), meta);
        }

        let mut challenge = cf::is_challenge(meta.status, &meta.headers);
        if !challenge && (meta.status == 403 || meta.status == 503) {
            if let Some(Some(bytes)) = with_cancel(&o.cancel, read_body(resp, o.max_size)).await {
                challenge = cf::is_challenge_body(&decode(&bytes, o.encoding, &meta.headers));
            }
            if !challenge && NON_CF_BLOCK_LOGGED.insert(host.clone(), true).is_none() {
                log::warn(
                    cat::HOST,
                    format!("{host}: {} без признаков Cloudflare - браузерный fallback не сработает; проверьте Referer/зеркало/лимит", meta.status),
                );
            }
        }
        if challenge {
            cf::mark_guarded(&host);
            if let Some(b) = cf::fetch(url, o.cookie.as_deref(), o.referer.as_deref(), &o.headers).await {
                return (Some(b), BaseResponse::ok_synthetic(url));
            }
        }
        last = meta;
    }
    (None, last)
}

/// GET body.
pub async fn get(url: &str, o: &Req) -> Option<String> {
    base_get(url, o).await.0
}

/// GET + JSON decode.
pub async fn get_json<T: DeserializeOwned>(url: &str, o: &Req) -> Option<T> {
    let body = get(url, o).await?;
    serde_json::from_str(&body).ok()
}

/// Request body for POST.
#[derive(Clone, Debug)]
pub enum PostBody {
    /// `application/x-www-form-urlencoded` (already encoded).
    Form(String),
    /// Form from pairs (encoded here).
    FormPairs(Vec<(String, String)>),
    Json(String),
    Raw { content_type: String, data: Vec<u8> },
}

impl PostBody {
    fn apply(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            PostBody::Form(s) => rb.header("content-type", "application/x-www-form-urlencoded; charset=utf-8").body(s.clone()),
            PostBody::FormPairs(p) => rb.form(p),
            PostBody::Json(s) => rb.header("content-type", "application/json; charset=utf-8").body(s.clone()),
            PostBody::Raw { content_type, data } => rb.header("content-type", content_type.as_str()).body(data.clone()),
        }
    }
}

/// POST returning body + meta (useful for login flows that read Set-Cookie).
pub async fn base_post(url: &str, body: &PostBody, o: &Req) -> (Option<String>, BaseResponse) {
    let mut last = BaseResponse::failed(url);
    for px in resolve_proxies(url, o.useproxy, o.proxy.as_deref()) {
        if o.cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false) {
            break;
        }
        let Some(client) = client_for(px, o.redirect, o.http2) else { continue };
        let rb = body.apply(client.post(url).headers(build_headers(o)).timeout(Duration::from_secs(o.timeout.max(1))));
        let Some(Ok(resp)) = with_cancel(&o.cancel, rb.send()).await else { continue };
        let meta = BaseResponse { status: resp.status().as_u16(), headers: resp.headers().clone(), url: resp.url().to_string() };
        if meta.status != 200 {
            last = meta;
            continue;
        }
        let Some(Some(bytes)) = with_cancel(&o.cancel, read_body(resp, o.max_size)).await else { continue };
        let text = decode(&bytes, o.encoding, &meta.headers);
        if text.trim().is_empty() {
            last = meta;
            continue;
        }
        return (Some(text), meta);
    }
    (None, last)
}

/// POST body. Only 200 with a non-empty body counts as success.
pub async fn post(url: &str, body: &PostBody, o: &Req) -> Option<String> {
    base_post(url, body, o).await.0
}

pub async fn post_json<T: DeserializeOwned>(url: &str, body: &PostBody, o: &Req) -> Option<T> {
    let s = post(url, body, o).await?;
    serde_json::from_str(&s).ok()
}

/// Download bytes. Downloads usually want `Req::timeout(30)`.
pub async fn download(url: &str, o: &Req) -> Option<Vec<u8>> {
    for px in resolve_proxies(url, o.useproxy, o.proxy.as_deref()) {
        if o.cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false) {
            return None;
        }
        let Some(client) = client_for(px, o.redirect, o.http2) else { continue };
        let req = client.get(url).headers(build_headers(o)).timeout(Duration::from_secs(o.timeout.max(1)));
        let Some(Ok(resp)) = with_cancel(&o.cancel, req.send()).await else { continue };
        if resp.status() != StatusCode::OK {
            continue;
        }
        let Some(Some(bytes)) = with_cancel(&o.cancel, read_body(resp, o.max_size)).await else { continue };
        if bytes.is_empty() {
            continue;
        }
        return Some(bytes);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_normalization() {
        assert_eq!(normalize_proxy_address("1.2.3.4:8080"), "http://1.2.3.4:8080");
        assert_eq!(normalize_proxy_address("socks5://127.0.0.1:9050"), "socks5h://127.0.0.1:9050");
        assert_eq!(normalize_proxy_address("socks://h:1"), "socks5h://h:1");
    }
}
