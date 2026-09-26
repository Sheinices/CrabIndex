//! `/cron/cloudflare/*` - FlareSolverr warm-up, status and controls for the admin panel.
//!
//! Solving a challenge takes ~80-180 s and loads the CPU; call warm-up from a separate cron
//! a few minutes before the Rutracker crawl.

use axum::extract::Query;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

use crab_core::log::{self, cat};
use crab_core::net::cf;

use crate::{cffetch, clearance, stats};

pub const DEFAULT_WARMUP_URL: &str = "https://rutracker.org/forum/tracker.php?nm=";

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct WarmupQuery {
    pub url: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct WarmupResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub length: usize,
    pub tookSeconds: f64,
}

/// Opens `url` in the browser so the session is ready for the crawl. Default: rutracker
/// tracker search (reliably triggers "Just a moment…").
pub async fn warmup(url: &str) -> WarmupResult {
    let started = Instant::now();
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string()));

    let html = clearance::fetch_async(url, None, None, &[]).await;
    let ok = html.as_ref().map(|h| !h.trim().is_empty()).unwrap_or(false);

    // Mark the host only after a successful warm-up - otherwise, with FlareSolverr down,
    // every GET would go to the browser for `guardedHours`.
    let h = host.as_deref().unwrap_or_default();
    if ok {
        cf::mark_guarded(h);
    } else {
        cf::unguard(h);
    }

    WarmupResult {
        ok,
        host,
        length: html.as_deref().map(|h| h.encode_utf16().count()).unwrap_or(0),
        tookSeconds: (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
    }
}

async fn warmup_handler(Query(q): Query<WarmupQuery>) -> Json<WarmupResult> {
    let url = q.url.filter(|u| !u.is_empty()).unwrap_or_else(|| DEFAULT_WARMUP_URL.to_string());
    Json(warmup(&url).await)
}

/// `GET /cron/cloudflare/status` - FlareSolverr / cffetch state and per-host stats (admin panel).
async fn status_handler() -> Json<Value> {
    let c = crab_core::conf();
    let fs = c.flaresolverr.clone();
    let cff = c.cffetch.clone();
    let solver = clearance::solver_info(&fs.url).await;
    let crawl = if !fs.crawlUrl.trim().is_empty() && fs.crawlUrl != fs.url { Some(clearance::solver_info(&fs.crawlUrl).await) } else { None };
    let guarded: Vec<Value> = cf::guarded_hosts().into_iter().map(|(host, since)| json!({ "host": host, "since": since })).collect();
    Json(json!({
        "enabled": fs.enable,
        "paused": cf::is_paused(),
        "settings": {
            "url": fs.url,
            "crawlUrl": fs.crawlUrl,
            "maxTimeoutMs": fs.maxTimeoutMs,
            "sessionIdleMinutes": fs.sessionIdleMinutes,
            "browserTimeoutRetries": fs.browserTimeoutRetries,
            "recycleAfterTimeouts": fs.recycleAfterTimeouts,
            "guardedHours": fs.guardedHours,
            "recheckMinutes": fs.recheckMinutes,
        },
        "solver": solver,
        "crawlSolver": crawl,
        "cffetch": {
            "enabled": cff.enable,
            "url": cff.url,
            "impersonate": cff.impersonate,
            "maxConcurrent": cff.maxConcurrent,
            "clearanceMinutes": cff.clearanceMinutes,
            "hosts": cffetch::snapshot(),
        },
        "sessions": clearance::sessions_snapshot(),
        "guarded": guarded,
        "stats": {
            "since": stats::since(),
            "hosts": stats::hosts(),
            "recentErrors": stats::recent_errors(100),
        },
    }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HostQuery {
    host: Option<String>,
}

/// `POST /cron/cloudflare/sessions/close[?host=]` - free browser memory.
async fn close_sessions_handler(Query(q): Query<HostQuery>) -> Json<Value> {
    let (closed, busy) = clearance::close_sessions(q.host.as_deref()).await;
    Json(json!({ "ok": true, "closed": closed, "busy": busy }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PauseQuery {
    value: Option<bool>,
}

/// `POST /cron/cloudflare/pause?value=true|false` - stop using the browser until unpaused or
/// restart (config stays as is); pausing also closes all sessions.
async fn pause_handler(Query(q): Query<PauseQuery>) -> Json<Value> {
    let pause = q.value.unwrap_or(true);
    cf::set_paused(pause);
    let (closed, busy) = if pause { clearance::close_sessions(None).await } else { (0, 0) };
    log::warn(cat::HOST, if pause { "FlareSolverr приостановлен из админ-панели" } else { "FlareSolverr снова включён из админ-панели" });
    Json(json!({ "ok": true, "paused": pause, "closed": closed, "busy": busy }))
}

/// `POST /cron/cloudflare/stats/reset`
async fn reset_stats_handler() -> Json<Value> {
    stats::reset();
    Json(json!({ "ok": true }))
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/cloudflare/warmup", get(warmup_handler).post(warmup_handler))
        .route("/cron/cloudflare/status", get(status_handler))
        .route("/cron/cloudflare/sessions/close", post(close_sessions_handler))
        .route("/cron/cloudflare/pause", post(pause_handler))
        .route("/cron/cloudflare/stats/reset", post(reset_stats_handler))
}
