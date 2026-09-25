//! Browser (FlareSolverr) fetches for hosts behind Cloudflare.
//!
//! Hosts behind Cloudflare: FlareSolverr solves the challenge and yields `cf_clearance`.
//! Pages are then fetched by [`crate::cffetch`] (localhost cffetch, Chrome TLS). Each host
//! gets its own Chromium session, otherwise kinozal and anibelka would share a tab.
//!
//! Challenge detection and guarded-host bookkeeping live in `crab_core::net::cf`.

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crab_core::config::FlareSolverrSettings;
use crab_core::conf;
use crab_core::log::{self, cat};
use crab_core::net::cf;

use crate::cffetch::{self, Clearance};
use crate::json_util::{error_type_name, val_i32, val_str};

const SESSION_PREFIX: &str = "crabindex";

/// Config snapshot with the lane-specific solver URL.
#[derive(Clone, Debug)]
struct View {
    url: String,
    max_timeout_ms: i32,
    browser_timeout_retries: i32,
    recycle_after_timeouts: i32,
    session_idle_minutes: i32,
}

impl View {
    fn with_url(c: &FlareSolverrSettings, url: &str) -> Self {
        View {
            url: url.to_string(),
            max_timeout_ms: c.maxTimeoutMs,
            browser_timeout_retries: c.browserTimeoutRetries.max(0),
            recycle_after_timeouts: c.recycleAfterTimeouts.max(1),
            session_idle_minutes: c.sessionIdleMinutes,
        }
    }
}

/// FlareSolverr settings view: `None` when FlareSolverr is disabled. In the crawl lane the
/// non-empty `crawlUrl` is used instead of `url`.
fn view() -> Option<View> {
    let c = conf();
    let fs = &c.flaresolverr;
    if !fs.enable || fs.url.trim().is_empty() {
        return None;
    }
    let url = if cf::is_crawl_lane() && !fs.crawlUrl.trim().is_empty() { &fs.crawlUrl } else { &fs.url };
    Some(View::with_url(fs, url))
}

struct SessionState {
    alive: bool,
    last_use: DateTime<Utc>,
    consecutive_browser_timeouts: i32,
}

struct BrowserSession {
    name: String,
    solver_url: String,
    /// One request per session at a time.
    gate: tokio::sync::Mutex<()>,
    st: Mutex<SessionState>,
}

impl BrowserSession {
    fn alive(&self) -> bool {
        self.st.lock().alive
    }
    fn set_alive(&self, v: bool) {
        self.st.lock().alive = v;
    }
}

static SESSIONS: Lazy<DashMap<String, Arc<BrowserSession>>> = Lazy::new(DashMap::new);
static RENEW_GATES: Lazy<DashMap<String, Arc<Semaphore>>> = Lazy::new(DashMap::new);
static RENEWING: Lazy<DashMap<String, OwnedSemaphorePermit>> = Lazy::new(DashMap::new);
static IDLE_TIMER: AtomicBool = AtomicBool::new(false);

static CLIENT: Lazy<reqwest::Client> =
    Lazy::new(|| reqwest::Client::builder().no_proxy().build().unwrap_or_else(|_| reqwest::Client::new()));

const RENEW_WAIT_SECS: i64 = 100;

/// FlareSolverr session id for a host, charset `[A-Za-z0-9_-]`.
/// `kinozal.guru` → `crabindex-kinozal_guru`.
pub fn session_name_for(host: Option<&str>) -> String {
    let Some(host) = host.filter(|h| !h.trim().is_empty()) else { return SESSION_PREFIX.to_string() };
    let mut s = String::with_capacity(SESSION_PREFIX.len() + 1 + host.len());
    s.push_str(SESSION_PREFIX);
    s.push('-');
    for c in host.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    s
}

fn session_for_host(v: &View, host: &str) -> Arc<BrowserSession> {
    let name = session_name_for(Some(host));
    let k = format!("{}\n{}", v.url, name);
    SESSIONS
        .entry(k)
        .or_insert_with(|| {
            Arc::new(BrowserSession {
                name,
                solver_url: v.url.clone(),
                gate: tokio::sync::Mutex::new(()),
                st: Mutex::new(SessionState { alive: false, last_use: DateTime::<Utc>::MIN_UTC, consecutive_browser_timeouts: 0 }),
            })
        })
        .value()
        .clone()
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string()))
}

// ---------------------------------------------------------------- fetching a page

enum FastOutcome {
    NotAvailable,
    Ok(String),
    PageFailed,
    ClearanceLost,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum FetchOutcome {
    Ok,
    PageFailed,
    BrowserFailed,
}

/// Releases the renew gate when the browser section ends.
struct RenewRelease(String);

impl Drop for RenewRelease {
    fn drop(&mut self) {
        RENEWING.remove(&self.0.to_lowercase());
    }
}

/// Page through cffetch fast path, then the browser.
/// Browser timeout: retry the same session first; destroy only after `recycleAfterTimeouts`
/// in a row (or immediately on an explicit session error). `referer` / `extra_headers`
/// go to cffetch only (FlareSolverr v2 drops `headers`).
pub async fn fetch_async(url: &str, cookie: Option<&str>, referer: Option<&str>, extra_headers: &[(String, String)]) -> Option<String> {
    let v = view()?;
    if url.trim().is_empty() {
        return None;
    }
    let host = host_of(url)?;

    for _round in 0..3 {
        match try_fast(&host, url, cookie, referer, extra_headers).await {
            FastOutcome::Ok(html) => return Some(html),
            FastOutcome::PageFailed => return None,
            FastOutcome::NotAvailable => break,
            FastOutcome::ClearanceLost => {}
        }
        if clearance_renewed(&host).await {
            break;
        }
    }

    let session = session_for_host(&v, &host);
    let _gate = session.gate.lock().await;
    let _renew = RenewRelease(host.clone());

    if !session.alive() && !create_session(&v, &session).await {
        return None;
    }

    let (outcome, html, fail) = request_with_timeout_retries(&v, &session, url, cookie).await;

    if outcome == FetchOutcome::Ok {
        session.st.lock().consecutive_browser_timeouts = 0;
        touch_session(&v, &session);
        return html;
    }
    if outcome == FetchOutcome::PageFailed {
        touch_session(&v, &session);
        return None;
    }

    let fail_message = fail.unwrap_or_default();
    let browser_timeout = is_browser_timeout_message(&fail_message);
    let session_broken = is_session_broken_message(&fail_message);

    if browser_timeout && !session_broken {
        let n = {
            let mut st = session.st.lock();
            st.consecutive_browser_timeouts += 1;
            st.consecutive_browser_timeouts
        };
        if n < v.recycle_after_timeouts {
            log::warn(
                cat::HOST,
                format!("{host}: FlareSolverr browser timeout ({n}/{}) - сессию оставляем, caller ретраит", v.recycle_after_timeouts),
            );
            touch_session(&v, &session);
            return None;
        }
        log::warn(cat::HOST, format!("{host}: session recycled after {n} browser timeouts"));
    } else {
        log::warn(cat::HOST, format!("{host}: FlareSolverr session recycle - {fail_message}"));
    }

    destroy_session(&v, &session).await;
    session.st.lock().consecutive_browser_timeouts = 0;

    if !create_session(&v, &session).await {
        return None;
    }

    let (outcome, html, fail) = request_with_timeout_retries(&v, &session, url, cookie).await;
    if outcome == FetchOutcome::Ok {
        session.st.lock().consecutive_browser_timeouts = 0;
        log::warn(cat::HOST, format!("{host}: session recycled, OK"));
        touch_session(&v, &session);
        return html;
    }
    if is_browser_timeout_message(fail.as_deref().unwrap_or_default()) {
        session.st.lock().consecutive_browser_timeouts = 1;
    }
    touch_session(&v, &session);
    None
}

/// `true` = this caller renews through the browser;
/// `false` = someone else renews, we waited (up to 100 s) for a fresh clearance.
async fn clearance_renewed(host: &str) -> bool {
    let k = host.to_lowercase();
    let gate = RENEW_GATES.entry(k.clone()).or_insert_with(|| Arc::new(Semaphore::new(1))).value().clone();
    if let Ok(permit) = gate.try_acquire_owned() {
        RENEWING.insert(k, permit);
        return true;
    }
    let deadline = Utc::now() + Duration::seconds(RENEW_WAIT_SECS);
    while Utc::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if cffetch::for_host(host).is_some() {
            return false;
        }
    }
    false
}

async fn try_fast(host: &str, url: &str, cookie: Option<&str>, referer: Option<&str>, extra_headers: &[(String, String)]) -> FastOutcome {
    // cffetch without cookie first (TLS impersonation): some hosts reject the plain client
    // without serving a challenge - no need to pay the full browser timeout for them.
    let Some(clearance) = cffetch::for_host(host).or_else(|| cffetch::for_uncleared(host)) else {
        return FastOutcome::NotAvailable;
    };

    let merged = merge_cookies(clearance.cookies.as_deref(), cookie);
    let request_headers = extra_browser_headers(referer, extra_headers);
    let c = Clearance { cookies: merged, user_agent: clearance.user_agent.clone(), at: clearance.at };

    let (status, body, cf_mitigated) =
        cffetch::get_async(url, &c, if request_headers.is_empty() { None } else { Some(&request_headers) }).await;

    if status == 0 {
        return FastOutcome::NotAvailable;
    }
    if cffetch::clearance_lost(status, body.as_deref(), cf_mitigated) {
        if cffetch::should_drop_clearance(host) {
            cffetch::forget(host);
            return FastOutcome::ClearanceLost;
        }
        return FastOutcome::NotAvailable;
    }
    if status == 200 {
        if let Some(b) = body.as_ref().filter(|b| !b.trim().is_empty()) {
            return FastOutcome::Ok(b.clone());
        }
    }
    // Old cffetch ignores JSON `headers`; Ultradox nginx 503s without Referer.
    // Not PageFailed - fall through to FlareSolverr.
    if should_skip_fast_path_for_origin_503(status, body.as_deref(), referer) {
        return FastOutcome::NotAvailable;
    }
    FastOutcome::PageFailed
}

/// Origin nginx 503 with a Referer set: let the browser try instead of failing the page.
pub fn should_skip_fast_path_for_origin_503(status: i32, body: Option<&str>, referer: Option<&str>) -> bool {
    if status != 503 || referer.map(|r| r.trim().is_empty()).unwrap_or(true) {
        return false;
    }
    match body {
        None | Some("") => true,
        Some(b) => b.to_lowercase().contains(&"503 Service Temporarily Unavailable".to_lowercase()),
    }
}

/// Merge browser cookies with the caller's (caller wins per name).
fn merge_cookies(from_browser: Option<&str>, from_caller: Option<&str>) -> Option<String> {
    if from_caller.map(|c| c.trim().is_empty()).unwrap_or(true) {
        return from_browser.map(str::to_string);
    }
    let mut jar: IndexMap<String, (String, String)> = IndexMap::new();
    for source in [from_browser, from_caller] {
        let Some(source) = source.filter(|s| !s.trim().is_empty()) else { continue };
        for part in source.split(';') {
            let Some(eq) = part.find('=').filter(|&e| e > 0) else { continue };
            let name = part[..eq].trim();
            if name.is_empty() {
                continue;
            }
            let val = part[eq + 1..].trim().to_string();
            match jar.get_mut(&name.to_lowercase()) {
                Some(slot) => slot.1 = val,
                None => {
                    jar.insert(name.to_lowercase(), (name.to_string(), val));
                }
            }
        }
    }
    Some(jar.values().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; "))
}

fn ci_set(map: &mut IndexMap<String, String>, key: &str, val: String) {
    if let Some(k) = map.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned() {
        map.insert(k, val);
    } else {
        map.insert(key.to_string(), val);
    }
}

/// Header lookup ignoring case.
pub fn header_ci<'a>(map: &'a IndexMap<String, String>, key: &str) -> Option<&'a String> {
    map.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

/// Referer + extra GET headers for cffetch (Referer / Accept / Accept-Language only).
/// Cookie and User-Agent stay on their own fields so the TLS path is not overwritten.
pub fn extra_browser_headers(referer: Option<&str>, extra_headers: &[(String, String)]) -> IndexMap<String, String> {
    let mut headers = IndexMap::new();
    if let Some(r) = referer.filter(|r| !r.trim().is_empty()) {
        ci_set(&mut headers, "Referer", r.trim().to_string());
    }
    for (name, val) in extra_headers {
        if name.trim().is_empty() {
            continue;
        }
        let key = name.trim();
        let forwarded = ["Referer", "Accept", "Accept-Language"].iter().any(|h| key.eq_ignore_ascii_case(h));
        if !forwarded {
            continue;
        }
        ci_set(&mut headers, key, val.clone());
    }
    headers
}

async fn remember_clearance(url: &str, solution: &Value) {
    if !cffetch::enabled() {
        return;
    }
    let Some(host) = host_of(url) else { return };
    if cffetch::for_host(&host).is_some() {
        return;
    }
    let Some(jar) = solution.get("cookies").and_then(|j| j.as_array()).filter(|j| !j.is_empty()) else { return };

    let mut parts = Vec::new();
    for c in jar {
        let Some(name) = val_str(c.get("name")).filter(|n| !n.trim().is_empty()) else { continue };
        parts.push(format!("{name}={}", val_str(c.get("value")).unwrap_or_default()));
    }
    let cookies = parts.join("; ");
    if cookies.trim().is_empty() {
        return;
    }
    let candidate = Clearance { cookies: Some(cookies), user_agent: val_str(solution.get("userAgent")), at: Utc::now() };

    if !cffetch::validate_async(url, &candidate).await {
        cffetch::block_fast_path(&host);
        return;
    }
    cffetch::remember(&host, candidate.cookies.as_deref(), candidate.user_agent.as_deref());
}

/// Destroy and recreate the host's Chromium session
/// (stale-shell storms). No-op when FlareSolverr is disabled.
pub async fn recycle_session(host: &str) {
    let Some(v) = view() else { return };
    if host.trim().is_empty() {
        return;
    }
    let session = session_for_host(&v, host);
    let _gate = session.gate.lock().await;
    log::warn(cat::HOST, format!("{host}: FlareSolverr session recycle requested ({})", session.name));
    destroy_session(&v, &session).await;
    session.st.lock().consecutive_browser_timeouts = 0;
    create_session(&v, &session).await;
}

fn touch_session(v: &View, session: &BrowserSession) {
    session.st.lock().last_use = Utc::now();
    arm_idle_timer(v);
}

/// Same-session retries on browser timeout before escalating.
async fn request_with_timeout_retries(v: &View, session: &BrowserSession, url: &str, cookie: Option<&str>) -> (FetchOutcome, Option<String>, Option<String>) {
    let attempts = 1 + v.browser_timeout_retries;
    let mut last = (FetchOutcome::BrowserFailed, None, None);
    for i in 0..attempts {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        last = request(v, session, url, cookie).await;
        if last.0 != FetchOutcome::BrowserFailed {
            return last;
        }
        let msg = last.2.as_deref().unwrap_or_default();
        if !is_browser_timeout_message(msg) || is_session_broken_message(msg) {
            return last;
        }
        if i + 1 < attempts {
            log::warn(cat::HOST, format!("FlareSolverr browser timeout - same-session retry {}/{}", i + 1, v.browser_timeout_retries));
        }
    }
    last
}

async fn request(v: &View, session: &BrowserSession, url: &str, cookie: Option<&str>) -> (FetchOutcome, Option<String>, Option<String>) {
    let mut payload = Map::new();
    payload.insert("cmd".into(), json!("request.get"));
    payload.insert("session".into(), json!(session.name));
    payload.insert("url".into(), json!(url));
    payload.insert("maxTimeout".into(), json!(v.max_timeout_ms));
    let jar = parse_cookies(cookie);
    if !jar.is_empty() {
        payload.insert("cookies".into(), Value::Array(jar));
    }
    // FlareSolverr v2 removed `headers`; proxy only via PROXY_* of the FlareSolverr container.

    let Some(root) = call(&v.url, Value::Object(payload), v.max_timeout_ms as i64 + 30_000).await else {
        session.set_alive(false);
        return (FetchOutcome::BrowserFailed, None, Some("empty response / unreachable".into()));
    };

    if !val_str(root.get("status")).map(|s| s.eq_ignore_ascii_case("ok")).unwrap_or(false) {
        let message = val_str(root.get("message")).unwrap_or_default();
        log::error(cat::HOST, format!("FlareSolverr отказал: {message}"));
        return (FetchOutcome::BrowserFailed, None, Some(message));
    }

    let solution = root.get("solution").filter(|s| s.is_object());
    let status = solution.and_then(|s| val_i32(s.get("status"))).unwrap_or(0);
    let html = solution.and_then(|s| val_str(s.get("response")));

    let Some(html) = html.filter(|h| status == 200 && !h.trim().is_empty()) else {
        return (FetchOutcome::PageFailed, None, Some(format!("http {status}")));
    };

    // FS sometimes reports ok with the interstitial page - not a success.
    if cf::is_challenge_body(&html) {
        return (FetchOutcome::PageFailed, None, Some("challenge html in solution".into()));
    }
    // Origin nginx 503 behind CF: FS still reports solution.status=200.
    if html.encode_utf16().count() < 2000 && html.to_lowercase().contains("503 service temporarily unavailable") {
        return (FetchOutcome::PageFailed, None, Some("origin 503".into()));
    }

    if let Some(sol) = solution {
        remember_clearance(url, sol).await;
    }
    (FetchOutcome::Ok, Some(html), None)
}

pub(crate) fn is_browser_timeout_message(message: &str) -> bool {
    if message.is_empty() {
        return false;
    }
    let l = message.to_lowercase();
    l.contains("read timed out") || l.contains("httpconnectionpool") || l.contains("timeout after")
}

pub(crate) fn is_session_broken_message(message: &str) -> bool {
    if message.is_empty() {
        return false;
    }
    // "Session not found" / "Session timeout" - not to be confused with request timeout.
    message.to_lowercase().contains("session") && !is_browser_timeout_message(message)
}

fn parse_cookies(cookie: Option<&str>) -> Vec<Value> {
    let mut list = Vec::new();
    let Some(cookie) = cookie.filter(|c| !c.trim().is_empty()) else { return list };
    for part in cookie.split(';') {
        let Some(eq) = part.find('=').filter(|&e| e > 0) else { continue };
        let name = part[..eq].trim();
        let value = part[eq + 1..].trim();
        if !name.is_empty() {
            list.push(json!({ "name": name, "value": value }));
        }
    }
    list
}

// ---------------------------------------------------------------- sessions

async fn create_session(v: &View, session: &BrowserSession) -> bool {
    let root = call(&v.url, json!({ "cmd": "sessions.create", "session": session.name }), v.max_timeout_ms as i64 + 30_000).await;
    let ok = root
        .as_ref()
        .map(|r| {
            val_str(r.get("status")).map(|s| s.eq_ignore_ascii_case("ok")).unwrap_or(false)
                || val_str(r.get("message")).unwrap_or_default().to_lowercase().contains("already exists")
        })
        .unwrap_or(false);
    session.set_alive(ok);
    if ok {
        log::warn(cat::HOST, format!("FlareSolverr: сессия {} создана", session.name));
    } else {
        let msg = root.as_ref().and_then(|r| val_str(r.get("message"))).unwrap_or_default();
        log::error(cat::HOST, format!("FlareSolverr: сессию {} создать не удалось: {msg}", session.name));
    }
    ok
}

async fn destroy_session(v: &View, session: &BrowserSession) {
    call(&v.url, json!({ "cmd": "sessions.destroy", "session": session.name }), 60_000).await;
    session.set_alive(false);
}

fn arm_idle_timer(v: &View) {
    if v.session_idle_minutes <= 0 {
        return;
    }
    if IDLE_TIMER.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        IDLE_TIMER.store(false, Ordering::SeqCst);
        return;
    };
    handle.spawn(async {
        let period = std::time::Duration::from_secs(60);
        loop {
            tokio::time::sleep(period).await;
            close_if_idle().await;
        }
    });
}

/// Destroy browser sessions idle for `sessionIdleMinutes` (frees Chromium memory).
pub async fn close_if_idle() {
    let c = conf();
    let fs = &c.flaresolverr;
    if !fs.enable || fs.url.trim().is_empty() || fs.sessionIdleMinutes <= 0 {
        return;
    }
    let sessions: Vec<Arc<BrowserSession>> = SESSIONS.iter().map(|e| e.value().clone()).collect();
    for session in sessions {
        {
            let st = session.st.lock();
            if !st.alive {
                continue;
            }
            if st.last_use != DateTime::<Utc>::MIN_UTC && Utc::now() < st.last_use + Duration::minutes(fs.sessionIdleMinutes as i64) {
                continue;
            }
        }
        let Ok(_gate) = session.gate.try_lock() else { continue };
        if session.solver_url.trim().is_empty() {
            continue;
        }
        let v = View::with_url(fs, &session.solver_url);
        call(&v.url, json!({ "cmd": "sessions.destroy", "session": session.name }), 60_000).await;
        session.set_alive(false);
        log::warn(cat::HOST, format!("FlareSolverr: сессия {} закрыта по простою, память освобождена", session.name));
    }
}

/// POST a command to FlareSolverr; `None` when unreachable / not a JSON object.
async fn call(url: &str, payload: Value, timeout_ms: i64) -> Option<Value> {
    let res: Result<Value, (String, String)> = async {
        let resp = CLIENT
            .post(url)
            .header("content-type", "application/json; charset=utf-8")
            .body(payload.to_string())
            .timeout(std::time::Duration::from_millis(timeout_ms.max(1) as u64))
            .send()
            .await
            .map_err(|e| (error_type_name(&e).to_string(), e.to_string()))?;
        let text = resp.text().await.map_err(|e| (error_type_name(&e).to_string(), e.to_string()))?;
        match serde_json::from_str::<Value>(&text) {
            Ok(v @ Value::Object(_)) => Ok(v),
            Ok(_) => Err(("InvalidJson".into(), "response is not a JSON object".into())),
            Err(e) => Err(("InvalidJson".into(), e.to_string())),
        }
    }
    .await;
    match res {
        Ok(v) => Some(v),
        Err((t, m)) => {
            log::error(cat::HOST, format!("FlareSolverr недоступен: {t}: {m}"));
            None
        }
    }
}

/// Form POST from the host's browser session (FlareSolverr `request.post`), for logins behind
/// Cloudflare where a plain client gets 403. Returns the page after redirects and the
/// browser's cookie jar (it includes the cookies the login just set).
pub async fn post_form_async(url: &str, form: &str) -> Option<cf::BrowserPost> {
    let v = view()?;
    let host = host_of(url)?;
    let session = session_for_host(&v, &host);
    let _gate = session.gate.lock().await;

    if !session.alive() && !create_session(&v, &session).await {
        return None;
    }
    let payload = json!({
        "cmd": "request.post",
        "session": session.name,
        "url": url,
        "postData": form,
        "maxTimeout": v.max_timeout_ms,
    });
    let root = call(&v.url, payload, v.max_timeout_ms as i64 + 30_000).await;
    touch_session(&v, &session);
    let Some(root) = root else {
        session.set_alive(false);
        return None;
    };
    if !val_str(root.get("status")).map(|s| s.eq_ignore_ascii_case("ok")).unwrap_or(false) {
        let message = val_str(root.get("message")).unwrap_or_default();
        log::error(cat::HOST, format!("{host}: FlareSolverr POST отказал: {message}"));
        if is_session_broken_message(&message) {
            session.set_alive(false);
        }
        return None;
    }

    let solution = root.get("solution").filter(|s| s.is_object())?;
    let status = val_i32(solution.get("status")).unwrap_or(0).clamp(0, u16::MAX as i32) as u16;
    let body = val_str(solution.get("response")).unwrap_or_default();
    let cookies = solution
        .get("cookies")
        .and_then(|j| j.as_array())
        .map(|jar| {
            jar.iter()
                .filter_map(|c| {
                    let name = val_str(c.get("name")).filter(|n| !n.trim().is_empty())?;
                    Some((name, val_str(c.get("value")).unwrap_or_default()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(cf::BrowserPost { status, body, cookies })
}

/// [`cf::ChallengeSolver`] backed by [`fetch_async`] and [`post_form_async`].
pub struct FlareSolverrSolver;

#[async_trait::async_trait]
impl cf::ChallengeSolver for FlareSolverrSolver {
    async fn fetch(&self, url: &str, cookie: Option<&str>, referer: Option<&str>, headers: &[(String, String)]) -> Option<String> {
        fetch_async(url, cookie, referer, headers).await
    }

    async fn post_form(&self, url: &str, form: &str) -> Option<cf::BrowserPost> {
        post_form_async(url, form).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_cookies_caller_overrides_browser() {
        assert_eq!(merge_cookies(Some("cf_clearance=a; x=1"), Some("X=2; y=3")).as_deref(), Some("cf_clearance=a; x=2; y=3"));
        assert_eq!(merge_cookies(None, None), None);
        assert_eq!(merge_cookies(Some("a=1"), Some(" ")).as_deref(), Some("a=1"));
    }

    #[test]
    fn timeout_and_session_messages() {
        assert!(is_browser_timeout_message("Error solving the challenge. Timeout after 60.0 seconds."));
        assert!(is_browser_timeout_message("HTTPConnectionPool(host='localhost'): Read timed out."));
        assert!(!is_session_broken_message("Session timeout after 60 s"));
        assert!(is_session_broken_message("Session not found"));
        assert!(!is_browser_timeout_message(""));
    }

    #[test]
    fn parse_cookies_skips_bad_parts() {
        let v = parse_cookies(Some("a=1; =x; b ; c= 3 "));
        assert_eq!(v, vec![json!({"name":"a","value":"1"}), json!({"name":"c","value":"3"})]);
    }
}
