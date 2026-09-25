//! crab-tracks - media track analysis (TorrServer `/ffp`), the `Data/tracks` store,
//! per-tracker statistics and their HTTP endpoints.
#![allow(non_snake_case)]

pub mod http;
pub mod stats;
pub mod tracks;

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Register the tracks lookup hook and start the (non-blocking) track store initialization:
/// the stats cache and compact index load on a background thread, followed by an index
/// rebuild when needed.
pub fn init() {
    crab_core::hooks::register_tracks(Arc::new(tracks::db::Lookup));
    let _ = std::thread::Builder::new().name("tracks-startup".into()).spawn(tracks::startup_init);
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    http::router()
}

/// Start background workers: five track-analysis loops (typetask 1..=5), the TorrServer
/// orphan sweep and the stats collector.
pub fn spawn_workers(shutdown: CancellationToken) {
    crab_core::log::info(crab_core::log::cat::TRACKS, "tracks worker started");
    for typetask in 1..=5 {
        tokio::spawn(tracks::cron::run(typetask, shutdown.clone()));
    }
    tokio::spawn(tracks::cron::orphan_cleanup_loop(shutdown.clone()));

    crab_core::log::info(crab_core::log::cat::STATS, "stats worker started");
    tokio::spawn(stats::run_cron(shutdown));
}
