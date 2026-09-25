//! `/cron/cloudflare/*` - FlareSolverr warm-up.
//!
//! Solving a challenge takes ~80-180 s and loads the CPU; call it from a separate cron
//! a few minutes before the Rutracker crawl.

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crab_core::net::cf;

use crate::clearance;

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

pub fn router() -> Router {
    Router::new().route("/cron/cloudflare/warmup", get(warmup_handler).post(warmup_handler))
}
