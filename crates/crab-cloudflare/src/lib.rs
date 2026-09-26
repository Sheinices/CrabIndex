//! crab-cloudflare - browser (FlareSolverr) + cffetch fallback for hosts behind Cloudflare.
//!
//! * [`clearance`] - per-host FlareSolverr sessions, crawl lane routing, timeout retries/recycle.
//! * [`cffetch`] - fast path through localhost cffetch with the browser's `cf_clearance` jar.
//! * [`controller`] - `/cron/cloudflare/warmup`.
#![allow(non_snake_case)]

use std::sync::Arc;

pub mod cffetch;
pub mod clearance;
pub mod controller;
pub mod stats;
mod json_util;

pub use clearance::{extra_browser_headers, fetch_async, recycle_session, session_name_for, should_skip_fast_path_for_origin_503, FlareSolverrSolver};

/// Registers the FlareSolverr/cffetch solver with core networking.
pub fn init() {
    crab_core::net::cf::register_solver(Arc::new(FlareSolverrSolver));
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    controller::router()
}
