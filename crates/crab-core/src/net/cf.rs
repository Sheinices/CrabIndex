//! Cloudflare integration points.
//!
//! Core keeps the cheap, pure parts (challenge detection, "guarded host" bookkeeping,
//! crawl lane flag). The browser/FlareSolverr + cffetch implementation lives in
//! `crab-cloudflare` and is plugged in with [`register_solver`].

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use once_cell::sync::{Lazy, OnceCell};
use std::future::Future;
use std::sync::Arc;

use crate::conf;
use crate::log::{self, cat};

#[async_trait]
pub trait ChallengeSolver: Send + Sync {
    /// Fetch a page through the browser / cffetch fast path. `None` when it failed.
    async fn fetch(&self, url: &str, cookie: Option<&str>, referer: Option<&str>, headers: &[(String, String)]) -> Option<String>;
}

static SOLVER: OnceCell<Arc<dyn ChallengeSolver>> = OnceCell::new();

pub fn register_solver(s: Arc<dyn ChallengeSolver>) {
    let _ = SOLVER.set(s);
}

tokio::task_local! {
    static CRAWL_LANE: bool;
}

/// Run `fut` in the crawl lane (FlareSolverr `crawlUrl`).
pub async fn with_crawl_lane<F: Future>(fut: F) -> F::Output {
    CRAWL_LANE.scope(true, fut).await
}

pub fn is_crawl_lane() -> bool {
    CRAWL_LANE.try_with(|v| *v).unwrap_or(false)
}

fn solver_enabled() -> bool {
    let c = conf();
    c.flaresolverr.enable && !c.flaresolverr.url.trim().is_empty()
}

/// 403/503 with `cf-mitigated` header.
pub fn is_challenge(status: u16, headers: &reqwest::header::HeaderMap) -> bool {
    (status == 403 || status == 503) && headers.contains_key("cf-mitigated")
}

/// Cloudflare interstitial markup (not the jsd/main.js embedded on normal pages).
pub fn is_challenge_body(body: &str) -> bool {
    if body.is_empty() || body.len() > 200_000 {
        return false;
    }
    let l = body.to_lowercase();
    l.contains("cf-browser-verification")
        || l.contains("cf_chl_opt")
        || l.contains("just a moment")
        || l.contains("один момент")
        || l.contains("orchestrate/chl_page")
        || l.contains("challenge-platform/h/")
}

struct GuardState {
    since: DateTime<Utc>,
    last_probe: DateTime<Utc>,
}

static GUARDED: Lazy<DashMap<String, GuardState>> = Lazy::new(DashMap::new);

pub fn is_guarded(host: &str) -> bool {
    if !solver_enabled() || host.trim().is_empty() || SOLVER.get().is_none() {
        return false;
    }
    let key = host.to_ascii_lowercase();
    let c = conf();
    let now = Utc::now();
    let mut expired = false;
    let res = match GUARDED.get_mut(&key) {
        None => false,
        Some(mut st) => {
            if now > st.since + Duration::hours(c.flaresolverr.guardedHours as i64) {
                expired = true;
                false
            } else if now > st.last_probe + Duration::minutes(c.flaresolverr.recheckMinutes as i64) {
                st.last_probe = now;
                false
            } else {
                true
            }
        }
    };
    if expired {
        GUARDED.remove(&key);
    }
    res
}

pub fn unguard(host: &str) {
    if host.trim().is_empty() {
        return;
    }
    if GUARDED.remove(&host.to_ascii_lowercase()).is_some() {
        log::info(cat::HOST, format!("{host} отвечает обычному клиенту, браузер больше не нужен"));
    }
}

pub fn mark_guarded(host: &str) {
    if host.trim().is_empty() {
        return;
    }
    let now = Utc::now();
    let key = host.to_ascii_lowercase();
    if let Some(mut st) = GUARDED.get_mut(&key) {
        st.since = now;
        st.last_probe = now;
        return;
    }
    GUARDED.insert(key, GuardState { since: now, last_probe: now });
    log::warn(cat::HOST, format!("{host} закрыт проверкой Cloudflare, переходим на браузер"));
}

/// Guarded hosts snapshot (diagnostics): (host, since).
pub fn guarded_hosts() -> Vec<(String, DateTime<Utc>)> {
    GUARDED.iter().map(|e| (e.key().clone(), e.since)).collect()
}

/// Fetch via registered solver (browser). `None` when disabled / not registered / failed.
pub async fn fetch(url: &str, cookie: Option<&str>, referer: Option<&str>, headers: &[(String, String)]) -> Option<String> {
    if !solver_enabled() {
        return None;
    }
    let s = SOLVER.get()?.clone();
    s.fetch(url, cookie, referer, headers).await.filter(|b| !b.trim().is_empty())
}
