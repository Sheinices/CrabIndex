//! cffetch fast path: `cf_clearance` cookie from FlareSolverr + Chrome TLS via localhost
//! cffetch (`:8192/fetch`). Cookie jars are merged, a fresh clearance is checked with
//! [`validate_async`] (failure → [`block_fast_path`]); a bare 403 does not revoke the cookie,
//! three `cf-mitigated` within a minute do.

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::sync::Semaphore;

use crab_core::config::CfFetchSettings;
use crab_core::conf;
use crab_core::log::{self, cat};

use crate::json_util::{error_type_name, val_bool, val_i32, val_str};

/// Cookies + UA captured from the browser.
#[derive(Clone, Debug, PartialEq)]
pub struct Clearance {
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub at: DateTime<Utc>,
}

static CLEARANCE: Lazy<DashMap<String, Clearance>> = Lazy::new(DashMap::new);
static BLOCKED: Lazy<DashMap<String, DateTime<Utc>>> = Lazy::new(DashMap::new);
static MITIGATED: Lazy<DashMap<String, Arc<Mutex<MitigationRun>>>> = Lazy::new(DashMap::new);
static GATE: Lazy<Mutex<Option<(usize, Arc<Semaphore>)>>> = Lazy::new(|| Mutex::new(None));
static LAST_DOWN_LOG: Lazy<Mutex<Option<DateTime<Utc>>>> = Lazy::new(|| Mutex::new(None));

static CLIENT: Lazy<reqwest::Client> =
    Lazy::new(|| reqwest::Client::builder().no_proxy().build().unwrap_or_else(|_| reqwest::Client::new()));

const BLOCKED_MINUTES: i64 = 30;
const MITIGATIONS_TO_DROP: i32 = 3;
const MITIGATION_WINDOW_SECS: i64 = 60;

fn key(host: &str) -> String {
    host.to_lowercase()
}

fn blank(s: Option<&str>) -> bool {
    s.map(|s| s.trim().is_empty()).unwrap_or(true)
}

fn settings() -> Option<CfFetchSettings> {
    let c = conf();
    let s = &c.cffetch;
    if !s.enable || s.url.trim().is_empty() {
        None
    } else {
        Some(s.clone())
    }
}

/// cffetch is enabled and has a URL.
pub fn enabled() -> bool {
    settings().is_some()
}

// ---------------------------------------------------------------- cookie from the browser

/// Store browser cookies for a host - merged into the host's jar (not replaced).
pub fn remember(host: &str, cookies: Option<&str>, user_agent: Option<&str>) {
    if host.trim().is_empty() || blank(cookies) {
        return;
    }
    let cookies = cookies.unwrap_or_default();
    let k = key(host);
    let had = CLEARANCE.get(&k).map(|e| e.value().clone());
    let merged = merge_cookie_jars(had.as_ref().and_then(|h| h.cookies.as_deref()), Some(cookies));
    let ua = if blank(user_agent) { had.as_ref().and_then(|h| h.user_agent.clone()) } else { user_agent.map(str::to_string) };

    CLEARANCE.insert(k, Clearance { cookies: Some(merged), user_agent: ua, at: Utc::now() });

    if had.is_none() {
        log::warn(cat::HOST, format!("{host}: cookie от браузера получена, дальше идём быстрым путём"));
    }
}

/// Cookie jar merge: `name=value` pairs, names case-insensitive, later source wins,
/// first-seen order and name casing kept.
pub fn merge_cookie_jars(old: Option<&str>, fresh: Option<&str>) -> String {
    if blank(old) {
        return fresh.unwrap_or_default().to_string();
    }
    let mut jar: IndexMap<String, (String, String)> = IndexMap::new();
    for source in [old, fresh] {
        let Some(source) = source.filter(|s| !s.trim().is_empty()) else { continue };
        for part in source.split(';') {
            let Some(eq) = part.find('=') else { continue };
            if eq == 0 {
                continue;
            }
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
    jar.values().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ")
}

/// The browser cookie does not work in cffetch; 30 min browser only.
pub fn block_fast_path(host: &str) {
    if host.trim().is_empty() {
        return;
    }
    let now = Utc::now();
    let until = now + Duration::minutes(BLOCKED_MINUTES);
    let k = key(host);
    let first = BLOCKED.get(&k).map(|u| *u < now).unwrap_or(true);
    BLOCKED.insert(k, until);
    if first {
        log::warn(cat::HOST, format!("{host}: cookie от браузера помощнику не годится, {BLOCKED_MINUTES} мин ходим браузером"));
    }
}

pub fn fast_path_blocked(host: &str) -> bool {
    !host.trim().is_empty() && BLOCKED.get(&key(host)).map(|u| Utc::now() < *u).unwrap_or(false)
}

/// Live clearance for the host, `None` when absent/expired/blocked/disabled.
pub fn for_host(host: &str) -> Option<Clearance> {
    let s = settings()?;
    if host.trim().is_empty() || fast_path_blocked(host) {
        return None;
    }
    let c = CLEARANCE.get(&key(host))?.value().clone();
    if s.clearanceMinutes > 0 && Utc::now() > c.at + Duration::minutes(s.clearanceMinutes as i64) {
        return None;
    }
    Some(c)
}

/// Clearance without cookies (TLS impersonation only).
pub fn for_uncleared(host: &str) -> Option<Clearance> {
    if settings().is_none() || host.trim().is_empty() || fast_path_blocked(host) {
        return None;
    }
    Some(Clearance { cookies: None, user_agent: None, at: Utc::now() })
}

pub fn forget(host: &str) {
    if !host.trim().is_empty() && CLEARANCE.remove(&key(host)).is_some() {
        log::warn(cat::HOST, format!("{host}: cookie больше не проходит, возвращаемся к браузеру"));
    }
}

/// Fast-path state of one host (admin panel).
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FastHost {
    pub host: String,
    /// When the browser cookie was taken over.
    pub clearance_at: Option<DateTime<Utc>>,
    /// Fast path disabled for this host until then (the cookie did not work in cffetch).
    pub blocked_until: Option<DateTime<Utc>>,
}

pub fn snapshot() -> Vec<FastHost> {
    let now = Utc::now();
    let mut hosts: Vec<String> = CLEARANCE.iter().map(|e| e.key().clone()).collect();
    hosts.extend(BLOCKED.iter().filter(|e| *e.value() > now).map(|e| e.key().clone()));
    hosts.sort();
    hosts.dedup();
    hosts
        .into_iter()
        .map(|h| FastHost {
            clearance_at: CLEARANCE.get(&h).map(|c| c.at),
            blocked_until: BLOCKED.get(&h).map(|u| *u).filter(|u| *u > now),
            host: h,
        })
        .collect()
}

/// Clear all jars, mitigation counters and blocks (tests).
pub fn reset() {
    CLEARANCE.clear();
    MITIGATED.clear();
    BLOCKED.clear();
}

/// Check a candidate clearance against the page; cffetch unreachable counts as valid.
pub async fn validate_async(url: &str, candidate: &Clearance) -> bool {
    if url.trim().is_empty() {
        return false;
    }
    let (status, body, mitigated) = get_async(url, candidate, None).await;
    if status == 0 {
        return true;
    }
    !clearance_lost(status, body.as_deref(), mitigated)
}

// ---------------------------------------------------------------- tolerance to single refusals

struct MitigationRun {
    since: DateTime<Utc>,
    count: i32,
}

/// Three `cf-mitigated` for a host within 60 s → drop the clearance.
pub fn should_drop_clearance(host: &str) -> bool {
    if host.trim().is_empty() {
        return false;
    }
    let run = MITIGATED
        .entry(key(host))
        .or_insert_with(|| Arc::new(Mutex::new(MitigationRun { since: Utc::now(), count: 0 })))
        .value()
        .clone();
    let mut run = run.lock();
    let now = Utc::now();
    if now - run.since > Duration::seconds(MITIGATION_WINDOW_SECS) {
        run.since = now;
        run.count = 0;
    }
    run.count += 1;
    if run.count < MITIGATIONS_TO_DROP {
        return false;
    }
    run.since = now;
    run.count = 0;
    true
}

/// Clearance is lost only on `cf-mitigated` or interstitial markup; a naked 403 is not.
pub fn clearance_lost(_status: i32, body: Option<&str>, cf_mitigated: bool) -> bool {
    cf_mitigated || crab_core::net::cf::is_challenge_body(body.unwrap_or_default())
}

fn gate(s: &CfFetchSettings) -> Arc<Semaphore> {
    let size = if s.maxConcurrent > 0 { s.maxConcurrent as usize } else { 1 };
    let mut g = GATE.lock();
    match g.as_ref() {
        Some((n, sem)) if *n == size => sem.clone(),
        _ => {
            let sem = Arc::new(Semaphore::new(size));
            *g = Some((size, sem.clone()));
            sem
        }
    }
}

/// GET through cffetch → `(status, body, cfMitigated)`; status 0 = cffetch unavailable / failed.
pub async fn get_async(url: &str, clearance: &Clearance, extra_headers: Option<&IndexMap<String, String>>) -> (i32, Option<String>, bool) {
    let Some(s) = settings() else { return (0, None, false) };
    if url.trim().is_empty() {
        return (0, None, false);
    }

    let sem = gate(&s);
    let Ok(_permit) = sem.acquire_owned().await else { return (0, None, false) };

    let mut payload = Map::new();
    payload.insert("url".into(), json!(url));
    payload.insert("cookies".into(), json!(clearance.cookies));
    payload.insert("userAgent".into(), json!(clearance.user_agent.clone().unwrap_or_default()));
    payload.insert("impersonate".into(), json!(s.impersonate));
    payload.insert("timeout".into(), json!(s.timeoutSeconds));
    if !s.proxy.trim().is_empty() {
        payload.insert("proxy".into(), json!(s.proxy));
    }
    if let Some(h) = extra_headers.filter(|h| !h.is_empty()) {
        payload.insert("headers".into(), json!(h));
    }

    let timeout = std::time::Duration::from_secs((s.timeoutSeconds as i64 + 10).max(1) as u64);
    let res: Result<Value, String> = async {
        let resp = CLIENT
            .post(&s.url)
            .header("content-type", "application/json; charset=utf-8")
            .body(Value::Object(payload).to_string())
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| error_type_name(&e).to_string())?;
        let text = resp.text().await.map_err(|e| error_type_name(&e).to_string())?;
        match serde_json::from_str::<Value>(&text) {
            Ok(v @ Value::Object(_)) => Ok(v),
            _ => Err("InvalidJson".to_string()),
        }
    }
    .await;

    match res {
        Ok(root) => {
            let status = val_i32(root.get("status")).unwrap_or(0);
            let body = val_str(root.get("body"));
            if status == 0 {
                if let Some(error) = val_str(root.get("error")).filter(|e| !e.trim().is_empty()) {
                    log::warn(cat::HOST, format!("cffetch: {url}: {error}"));
                }
            }
            let mitigated = val_bool(root.get("cfMitigated")).unwrap_or(false);
            (status, body, mitigated)
        }
        Err(type_name) => {
            let now = Utc::now();
            let mut last = LAST_DOWN_LOG.lock();
            if last.map(|l| now - l > Duration::minutes(1)).unwrap_or(true) {
                *last = Some(now);
                log::warn(cat::HOST, format!("cffetch недоступен ({type_name}), уходим на браузер"));
            }
            (0, None, false)
        }
    }
}
