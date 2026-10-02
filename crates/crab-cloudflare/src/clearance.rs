// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

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
use crate::stats;
use crate::json_util::{error_type_name, val_i32, val_str};

const SESSION_PREFIX: &str = "crabindex";

/// Config snapshot with the lane-specific solver URL.
#[derive(Clone, Debug)]
struct View {
    url: String,
    max_timeout_ms: i32,
    browser_timeout_retries: i32,
    recycle_after_timeouts: i32,
    max_sessions: usize,
}

impl View {
    fn with_url(c: &FlareSolverrSettings, url: &str) -> Self {
        View {
            url: url.to_string(),
            max_timeout_ms: c.maxTimeoutMs,
            browser_timeout_retries: c.browserTimeoutRetries.max(0),
            recycle_after_timeouts: c.recycleAfterTimeouts.max(1),
            max_sessions: c.maxSessions.max(1) as usize,
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
    may_exist: bool,
    last_use: DateTime<Utc>,
    consecutive_browser_timeouts: i32,
}

struct BrowserSession {
    name: String,
    host: String,
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
static SOLVER_GATES: Lazy<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = Lazy::new(DashMap::new);
static PENDING_DESTROYS: Lazy<DashMap<String, (String, String)>> = Lazy::new(DashMap::new);
static RENEW_GATES: Lazy<DashMap<String, Arc<Semaphore>>> = Lazy::new(DashMap::new);
static RENEWING: Lazy<DashMap<String, OwnedSemaphorePermit>> = Lazy::new(DashMap::new);
static IDLE_TIMER: AtomicBool = AtomicBool::new(false);

static CLIENT: Lazy<reqwest::Client> =
    Lazy::new(|| reqwest::Client::builder().no_proxy().build().unwrap_or_else(|_| reqwest::Client::new()));

const RENEW_WAIT_SECS: i64 = 100;

fn solver_gate(url: &str) -> Arc<tokio::sync::Mutex<()>> {
    SOLVER_GATES.entry(url.to_string()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone()
}

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
                host: host.to_lowercase(),
                solver_url: v.url.clone(),
                gate: tokio::sync::Mutex::new(()),
                st: Mutex::new(SessionState { alive: false, may_exist: false, last_use: DateTime::<Utc>::MIN_UTC, consecutive_browser_timeouts: 0 }),
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

    if let Some(until) = crate::backoff::skip(&host) {
        // most recent browser requests to this site failed: do not feed FlareSolverr more work
        log::debug(cat::HOST, format!("{host}: browser backoff until {}", until.format("%H:%M:%S")));
        return None;
    }

    let url = url.to_string();
    let cookie = cookie.map(str::to_string);
    tokio::spawn(async move { fetch_browser(&v, &host, &url, cookie.as_deref()).await }).await.ok().flatten()
}

async fn fetch_browser(v: &View, host: &str, url: &str, cookie: Option<&str>) -> Option<String> {
    let solver = solver_gate(&v.url);
    let _solver_guard = solver.lock().await;
    if cf::is_paused() || crate::backoff::active().iter().any(|entry| entry.host == host && entry.until > Utc::now()) {
        return None;
    }
    let session = session_for_host(v, host);
    let _gate = session.gate.lock().await;
    let _renew = RenewRelease(host.to_string());

    if !session.alive() && !create_session(v, &session).await {
        return None;
    }

    let (outcome, html, fail) = request_with_timeout_retries(v, &session, url, cookie).await;

    if outcome == FetchOutcome::Ok {
        session.st.lock().consecutive_browser_timeouts = 0;
        touch_session(v, &session);
        return html;
    }
    if outcome == FetchOutcome::PageFailed {
        if fail.as_deref() == Some(CHALLENGE_HTML_MSG) {
            destroy_session(v, &session).await;
        }
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
            return None;
        }
        log::warn(cat::HOST, format!("{host}: session recycled after {n} browser timeouts"));
    } else {
        log::warn(cat::HOST, format!("{host}: FlareSolverr session recycle - {fail_message}"));
    }

    if !destroy_session(v, &session).await {
        return None;
    }
    session.st.lock().consecutive_browser_timeouts = 0;

    if !create_session(v, &session).await {
        return None;
    }

    let (outcome, html, fail) = request_with_timeout_retries(v, &session, url, cookie).await;
    if outcome == FetchOutcome::Ok {
        session.st.lock().consecutive_browser_timeouts = 0;
        log::warn(cat::HOST, format!("{host}: session recycled, OK"));
        touch_session(v, &session);
        return html;
    }
    if is_browser_timeout_message(fail.as_deref().unwrap_or_default()) {
        session.st.lock().consecutive_browser_timeouts = 1;
    }
    destroy_session(v, &session).await;
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
        stats::fast(host, false);
        if cffetch::should_drop_clearance(host) {
            cffetch::forget(host);
            stats::clearance_renewal(host);
            return FastOutcome::ClearanceLost;
        }
        return FastOutcome::NotAvailable;
    }
    if status == 200 {
        if let Some(b) = body.as_ref().filter(|b| !b.trim().is_empty()) {
            stats::fast(host, true);
            return FastOutcome::Ok(b.clone());
        }
    }
    stats::fast(host, false);
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
    let host = host.to_string();
    let _ = tokio::spawn(async move { recycle_inner(v, &host).await }).await;
}

async fn recycle_inner(v: View, host: &str) {
    let solver = solver_gate(&v.url);
    let _solver_guard = solver.lock().await;
    let session = session_for_host(&v, host);
    let _gate = session.gate.lock().await;
    log::warn(cat::HOST, format!("{host}: FlareSolverr session recycle requested ({})", session.name));
    if !destroy_session(&v, &session).await {
        return;
    }
    session.st.lock().consecutive_browser_timeouts = 0;
    create_session(&v, &session).await;
}

fn touch_session(_v: &View, session: &BrowserSession) {
    session.st.lock().last_use = Utc::now();
    arm_idle_timer();
}

/// Same-session retries before escalating: on a browser timeout (`browserTimeoutRetries`) and
/// when FlareSolverr reports "ok" with the interstitial still in the page ("challenge html in
/// solution"). The latter is FlareSolverr not recognising the current Cloudflare markup and
/// answering before the challenge auto-resolves; the same session usually returns the real page
/// a moment later, so a couple of quick re-requests beat failing the fetch and re-guarding the host.
const CHALLENGE_HTML_RETRIES: u32 = 2;
const CHALLENGE_HTML_MSG: &str = "challenge html in solution";

async fn request_with_timeout_retries(v: &View, session: &BrowserSession, url: &str, cookie: Option<&str>) -> (FetchOutcome, Option<String>, Option<String>) {
    let mut timeout_left = v.browser_timeout_retries;
    let mut challenge_left = CHALLENGE_HTML_RETRIES;
    let mut first = true;
    loop {
        if !first {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        first = false;
        let last = request(v, session, url, cookie).await;
        let msg = last.2.as_deref().unwrap_or_default();
        match last.0 {
            FetchOutcome::PageFailed if msg == CHALLENGE_HTML_MSG && challenge_left > 0 => {
                challenge_left -= 1;
                log::warn(
                    cat::HOST,
                    format!(
                        "FlareSolverr returned the challenge page as solved - same-session retry {}/{CHALLENGE_HTML_RETRIES}",
                        CHALLENGE_HTML_RETRIES - challenge_left
                    ),
                );
            }
            FetchOutcome::BrowserFailed if is_browser_timeout_message(msg) && !is_session_broken_message(msg) && timeout_left > 0 => {
                timeout_left -= 1;
                log::warn(
                    cat::HOST,
                    format!("FlareSolverr browser timeout - same-session retry {}/{}", v.browser_timeout_retries - timeout_left, v.browser_timeout_retries),
                );
            }
            _ => return last,
        }
    }
}

async fn request(v: &View, session: &BrowserSession, url: &str, cookie: Option<&str>) -> (FetchOutcome, Option<String>, Option<String>) {
    let started = std::time::Instant::now();
    let r = request_inner(v, session, url, cookie).await;
    let ms = started.elapsed().as_millis() as u64;
    let ok = r.0 == FetchOutcome::Ok;
    stats::browser(&session.host, ms, if ok { None } else { Some(r.2.as_deref().unwrap_or("failed")) });
    note_backoff(&session.host, ok);
    r
}

/// Feed the backoff ladder and log when a site is paused.
fn note_backoff(host: &str, ok: bool) {
    if crate::backoff::note(host, ok) {
        if let Some(b) = crate::backoff::active().into_iter().find(|b| b.host == host.to_lowercase()) {
            log::warn(
                cat::HOST,
                format!(
                    "{host}: браузерные запросы приостановлены до {} (успешных за 30 мин: {:.0}% из {}, пауза №{})",
                    b.until.format("%H:%M"),
                    b.ok_ratio * 100.0,
                    b.window_requests,
                    b.entered
                ),
            );
        }
    }
}

async fn request_inner(v: &View, session: &BrowserSession, url: &str, cookie: Option<&str>) -> (FetchOutcome, Option<String>, Option<String>) {
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

/// The browser behind the session is gone: crashed tab, deleted or unknown session. The only
/// recovery is `sessions.destroy` + `sessions.create` - `sessions.create` alone answers
/// "already exists" and hands back the same dead tab.
pub(crate) fn is_session_broken_message(message: &str) -> bool {
    if message.is_empty() {
        return false;
    }
    let l = message.to_lowercase();
    if l.contains("tab crashed") || l.contains("page crash") || l.contains("invalid session id") || l.contains("chrome not reachable") {
        return true;
    }
    // "Session not found" / "Session timeout" - not to be confused with request timeout.
    l.contains("session") && !is_browser_timeout_message(message)
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
    if !make_room(v, session).await {
        return false;
    }
    {
        let mut state = session.st.lock();
        state.may_exist = true;
        state.last_use = Utc::now();
    }
    arm_idle_timer();
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
        stats::session_created(&session.host);
        log::warn(cat::HOST, format!("FlareSolverr: сессия {} создана", session.name));
    } else {
        let msg = root.as_ref().and_then(|r| val_str(r.get("message"))).unwrap_or_default();
        stats::browser(&session.host, 0, Some(&format!("session create failed: {}", if msg.is_empty() { "unreachable" } else { msg.as_str() })));
        note_backoff(&session.host, false);
        log::error(cat::HOST, format!("FlareSolverr: сессию {} создать не удалось: {msg}", session.name));
    }
    ok
}

async fn destroy_remote(url: &str, name: &str) -> bool {
    let key = format!("{url}\n{name}");
    PENDING_DESTROYS.insert(key.clone(), (url.to_string(), name.to_string()));
    arm_idle_timer();
    let root = call(url, json!({ "cmd": "sessions.destroy", "session": name }), 60_000).await;
    let ok = root.as_ref().is_some_and(|response| {
        let message = val_str(response.get("message")).unwrap_or_default().to_lowercase();
        val_str(response.get("status")).is_some_and(|status| status.eq_ignore_ascii_case("ok"))
            || message.contains("not found") || message.contains("does not exist")
    });
    if !ok {
        log::warn(cat::HOST, format!("FlareSolverr: закрытие {name} не подтверждено, повторим уборку"));
    } else {
        PENDING_DESTROYS.remove(&key);
        if let Some(session) = SESSIONS.get(&key) {
            let mut state = session.st.lock();
            state.alive = false;
            state.may_exist = false;
        }
    }
    ok
}

async fn destroy_session(v: &View, session: &BrowserSession) -> bool {
    session.set_alive(false);
    if !destroy_remote(&v.url, &session.name).await {
        return false;
    }
    session.st.lock().may_exist = false;
    stats::session_recycled(&session.host);
    true
}

async fn make_room(v: &View, session: &BrowserSession) -> bool {
    let Some(remote) = list_solver_sessions(&v.url).await else {
        log::warn(cat::HOST, "FlareSolverr: список сессий недоступен, новый браузер не создаём");
        return false;
    };
    if remote.contains(&session.name) || session.st.lock().may_exist {
        session.st.lock().may_exist = true;
        if !destroy_session(v, session).await {
            return false;
        }
    }
    let known: Vec<Arc<BrowserSession>> = SESSIONS.iter().filter(|entry| entry.solver_url == v.url).map(|entry| entry.value().clone()).collect();
    let mut candidates: Vec<(String, DateTime<Utc>)> = remote.into_iter()
        .filter(|name| name.starts_with(SESSION_PREFIX) && name != &session.name)
        .map(|name| (name, DateTime::<Utc>::MIN_UTC)).collect();
    for other in &known {
        let state = other.st.lock();
        if !state.may_exist || other.name == session.name {
            continue;
        }
        if let Some(candidate) = candidates.iter_mut().find(|candidate| candidate.0 == other.name) {
            candidate.1 = state.last_use;
        } else {
            candidates.push((other.name.clone(), state.last_use));
        }
    }
    candidates.sort_by_key(|candidate| candidate.1);
    let remove_count = candidates.len().saturating_add(1).saturating_sub(v.max_sessions);
    for (name, _) in candidates.into_iter().take(remove_count) {
        if let Some(other) = known.iter().find(|other| other.name == name) {
            other.st.lock().may_exist = true;
            if !destroy_session(v, other).await {
                return false;
            }
        } else if !destroy_remote(&v.url, &name).await {
            return false;
        }
        log::info(cat::HOST, format!("FlareSolverr: сессия {name} закрыта для соблюдения maxSessions={}", v.max_sessions));
    }
    true
}

fn arm_idle_timer() {
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

/// Browser sessions known to this process, for the admin panel.
pub fn sessions_snapshot() -> Vec<Value> {
    let mut v: Vec<Value> = SESSIONS
        .iter()
        .map(|e| {
            let s = e.value();
            let st = s.st.lock();
            json!({
                "name": s.name,
                "host": s.host,
                "alive": st.alive,
                "cleanupPending": st.may_exist && !st.alive,
                "busy": s.gate.try_lock().is_err(),
                "lastUse": (st.last_use != DateTime::<Utc>::MIN_UTC).then_some(st.last_use),
                "consecutiveTimeouts": st.consecutive_browser_timeouts,
            })
        })
        .collect();
    v.sort_by(|a, b| a["host"].as_str().cmp(&b["host"].as_str()));
    v
}

/// Close browser sessions (all, or one host's) to free Chromium memory. Sessions with a
/// request in flight are skipped. Without `host`, orphan `crabindex-*` sessions that
/// FlareSolverr still holds are closed too. Returns (closed, busy).
pub async fn close_sessions(host: Option<&str>) -> (usize, usize) {
    let host = host.map(str::to_string);
    tokio::spawn(async move { close_sessions_inner(host.as_deref()).await }).await.unwrap_or((0, 0))
}

async fn close_sessions_inner(host: Option<&str>) -> (usize, usize) {
    let c = conf();
    let fs = &c.flaresolverr;
    if fs.url.trim().is_empty() {
        return (0, 0);
    }
    let host = host.map(|h| h.trim().to_lowercase()).filter(|h| !h.is_empty());
    let (mut closed, mut busy) = (0, 0);
    let sessions: Vec<Arc<BrowserSession>> = SESSIONS.iter().map(|e| e.value().clone()).collect();
    let mut urls = vec![fs.url.clone()];
    if !fs.crawlUrl.trim().is_empty() {
        urls.push(fs.crawlUrl.clone());
    }
    urls.extend(sessions.iter().map(|session| session.solver_url.clone()));
    urls.sort();
    urls.dedup();
    for url in urls {
        let known: Vec<_> = sessions.iter().filter(|session| session.solver_url == url && host.as_deref().is_none_or(|host| host == session.host)).collect();
        let solver = solver_gate(&url);
        let Ok(_solver_guard) = solver.try_lock() else {
            busy += known.iter().filter(|session| session.st.lock().may_exist).count().max(1);
            continue;
        };
        let mut attempted = Vec::new();
        let v = View::with_url(fs, &url);
        for session in known {
            if !session.st.lock().may_exist {
                continue;
            }
            let Ok(_gate) = session.gate.try_lock() else {
                busy += 1;
                continue;
            };
            attempted.push(session.name.clone());
            if destroy_session(&v, session).await {
                closed += 1;
            }
        }
        for name in solver_session_names(&url).await {
            if !name.starts_with(SESSION_PREFIX) || attempted.contains(&name)
                || host.as_deref().is_some_and(|host| name != session_name_for(Some(host))) {
                continue;
            }
            if destroy_remote(&url, &name).await {
                closed += 1;
            }
        }
    }
    (closed, busy)
}

async fn list_solver_sessions(url: &str) -> Option<Vec<String>> {
    call(url, json!({ "cmd": "sessions.list" }), 15_000)
        .await
        .and_then(|r| r.get("sessions").and_then(|s| s.as_array()).cloned())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
}

async fn solver_session_names(url: &str) -> Vec<String> {
    list_solver_sessions(url).await.unwrap_or_default()
}

/// FlareSolverr health for the admin panel: `GET` on the service root plus its session list.
pub async fn solver_info(url: &str) -> Value {
    if url.trim().is_empty() {
        return json!({ "url": url, "reachable": false, "error": "not configured" });
    }
    let root = url.trim_end_matches('/').trim_end_matches("/v1").to_string();
    let res = CLIENT.get(&root).timeout(std::time::Duration::from_secs(5)).send().await;
    match res {
        Ok(r) => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            json!({
                "url": url,
                "reachable": true,
                "version": val_str(v.get("version")),
                "userAgent": val_str(v.get("userAgent")),
                "message": val_str(v.get("msg")),
                "sessions": solver_session_names(url).await,
            })
        }
        Err(e) => json!({ "url": url, "reachable": false, "error": e.to_string() }),
    }
}

/// Destroy browser sessions idle for `sessionIdleMinutes` (frees Chromium memory).
pub async fn close_if_idle() {
    let c = conf();
    let fs = &c.flaresolverr;
    let mut attempted = Vec::new();
    let sessions: Vec<Arc<BrowserSession>> = SESSIONS.iter().map(|e| e.value().clone()).collect();
    for session in sessions {
        let solver = solver_gate(&session.solver_url);
        let Ok(_solver_guard) = solver.try_lock() else { continue };
        let Ok(_gate) = session.gate.try_lock() else { continue };
        {
            let st = session.st.lock();
            if !st.may_exist {
                continue;
            }
            if st.alive && fs.enable && (fs.sessionIdleMinutes <= 0 || Utc::now() < st.last_use + Duration::minutes(fs.sessionIdleMinutes as i64)) {
                continue;
            }
        }
        if session.solver_url.trim().is_empty() {
            continue;
        }
        let v = View::with_url(fs, &session.solver_url);
        attempted.push(format!("{}\n{}", session.solver_url, session.name));
        if destroy_session(&v, &session).await {
            stats::session_closed_idle(&session.host);
            log::warn(cat::HOST, format!("FlareSolverr: сессия {} закрыта уборщиком", session.name));
        }
    }
    let pending: Vec<_> = PENDING_DESTROYS.iter().map(|entry| entry.value().clone()).collect();
    for (url, name) in pending {
        if attempted.contains(&format!("{url}\n{name}")) {
            continue;
        }
        let solver = solver_gate(&url);
        let Ok(_solver_guard) = solver.try_lock() else { continue };
        destroy_remote(&url, &name).await;
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

enum PostOutcome {
    Ok(Value),
    /// Request refused; the session is still usable.
    Failed,
    /// The browser behind the session is dead (crashed tab, unknown session).
    SessionBroken,
}

/// One `request.post` on the session; records stats and marks a dead session.
async fn post_once(v: &View, session: &BrowserSession, host: &str, url: &str, form: &str) -> PostOutcome {
    let payload = json!({
        "cmd": "request.post",
        "session": session.name,
        "url": url,
        "postData": form,
        "maxTimeout": v.max_timeout_ms,
    });
    let started = std::time::Instant::now();
    let root = call(&v.url, payload, v.max_timeout_ms as i64 + 30_000).await;
    let ms = started.elapsed().as_millis() as u64;
    let Some(root) = root else {
        stats::browser(host, ms, Some("login POST: empty response / unreachable"));
        session.set_alive(false);
        return PostOutcome::Failed;
    };
    if !val_str(root.get("status")).map(|s| s.eq_ignore_ascii_case("ok")).unwrap_or(false) {
        let message = val_str(root.get("message")).unwrap_or_default();
        stats::browser(host, ms, Some(&message));
        note_backoff(host, false);
        log::error(cat::HOST, format!("{host}: FlareSolverr POST отказал: {message}"));
        if is_session_broken_message(&message) {
            session.set_alive(false);
            return PostOutcome::SessionBroken;
        }
        return PostOutcome::Failed;
    }
    stats::browser(host, ms, None);
    note_backoff(host, true);
    touch_session(v, session);
    PostOutcome::Ok(root)
}

/// Form POST from the host's browser session (FlareSolverr `request.post`), for logins behind
/// Cloudflare where a plain client gets 403. Returns the page after redirects and the
/// browser's cookie jar (it includes the cookies the login just set).
pub async fn post_form_async(url: &str, form: &str) -> Option<cf::BrowserPost> {
    let v = view()?;
    let url = url.to_string();
    let form = form.to_string();
    tokio::spawn(async move { post_form_inner(v, &url, &form).await }).await.ok().flatten()
}

async fn post_form_inner(v: View, url: &str, form: &str) -> Option<cf::BrowserPost> {
    let host = host_of(url)?;
    if cf::is_paused() || crate::backoff::skip(&host).is_some() {
        return None;
    }
    let solver = solver_gate(&v.url);
    let _solver_guard = solver.lock().await;
    if cf::is_paused() {
        return None;
    }
    let session = session_for_host(&v, &host);
    let _gate = session.gate.lock().await;

    if !session.alive() && !create_session(&v, &session).await {
        return None;
    }
    let root = match post_once(&v, &session, &host, url, form).await {
        PostOutcome::Ok(root) => root,
        PostOutcome::Failed => return None,
        PostOutcome::SessionBroken => {
            // A crashed tab stays crashed: every later call fails instantly. Recycle the
            // session and submit the form once more.
            log::warn(cat::HOST, format!("{host}: FlareSolverr session recycle after POST failure"));
            if !destroy_session(&v, &session).await {
                return None;
            }
            if !create_session(&v, &session).await {
                return None;
            }
            match post_once(&v, &session, &host, url, form).await {
                PostOutcome::Ok(root) => root,
                _ => return None,
            }
        }
    };

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
        assert!(is_session_broken_message("Error: Error solving the challenge. Message: tab crashed\n  (Session info: chrome=152.0.7977.82)"));
        assert!(is_session_broken_message("unknown error: session deleted because of page crash"));
        assert!(!is_session_broken_message("challenge html in solution"));
        assert!(!is_browser_timeout_message(""));
    }

    #[test]
    fn parse_cookies_skips_bad_parts() {
        let v = parse_cookies(Some("a=1; =x; b ; c= 3 "));
        assert_eq!(v, vec![json!({"name":"a","value":"1"}), json!({"name":"c","value":"3"})]);
    }
}
