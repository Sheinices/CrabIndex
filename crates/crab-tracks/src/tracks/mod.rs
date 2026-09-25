//! Media track (ffprobe) analysis via TorrServer and the `Data/tracks` store.

pub mod cron;
pub mod db;
pub mod export;
pub mod index;
pub mod logging;
pub mod models;
pub mod paths;
pub mod remote;
pub mod selector;
pub mod stats_cache;
pub mod workflow;

use chrono::Local;
use crab_core::log::{self, cat};
use std::sync::Once;
use std::time::Instant;

static STARTUP: Once = Once::new();

/// Load the stats cache and the compact track index, then schedule a background index
/// rebuild (when needed) and the periodic index persist loop. Runs once.
pub fn startup_init() {
    STARTUP.call_once(|| {
        let sw = Instant::now();
        log::info(cat::TRACKS, format!("startup init / {}", Local::now().format("%Y-%m-%d %H:%M:%S")));
        stats_cache::try_load_stats_cache_on_startup();
        index::load_tracks_index();
        log::info(
            cat::TRACKS,
            format!("startup init done / index={} / {:.1}s", index::track_index_count(), sw.elapsed().as_secs_f64()),
        );
        index::schedule_index_rebuild_if_needed();
        index::start_index_persist_loop();
    });
}
