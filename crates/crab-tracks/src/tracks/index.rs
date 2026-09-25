//! Compact set of infohashes that have a track file, persisted to `Data/temp/tracks-index.bz`.

use chrono::Local;
use crab_core::fdb::{read_gz_json, write_gz_json};
use crab_core::log::{self, cat};
use dashmap::DashSet;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::models::TracksIndexFile;
use super::paths::{self, is_valid_infohash, normalize_infohash, TRACKS_DIR};

pub const TRACKS_INDEX_PATH: &str = "Data/temp/tracks-index.bz";

/// Lowercase infohashes known to have a non-empty track file.
pub static TRACK_INDEX: Lazy<DashSet<String>> = Lazy::new(DashSet::new);

static INDEX_DIRTY: AtomicBool = AtomicBool::new(false);
static INDEX_BUILD_RUNNING: AtomicBool = AtomicBool::new(false);
static PERSIST_LOOP_STARTED: AtomicBool = AtomicBool::new(false);
static PERSIST_LOCK: Mutex<()> = parking_lot::const_mutex(());

pub fn track_index_count() -> usize {
    TRACK_INDEX.len()
}

pub fn contains(infohash: &str) -> bool {
    TRACK_INDEX.contains(&normalize_infohash(infohash))
}

pub fn load_tracks_index() {
    if !Path::new(TRACKS_INDEX_PATH).exists() {
        return;
    }
    match read_gz_json::<TracksIndexFile>(TRACKS_INDEX_PATH) {
        Some(data) => {
            let Some(hashes) = data.hashes.filter(|h| !h.is_empty()) else { return };
            for h in hashes.iter().filter(|h| is_valid_infohash(h)) {
                TRACK_INDEX.insert(normalize_infohash(h));
            }
            log::info(
                cat::TRACKS_INDEX,
                format!("loaded {} hashes (built {} UTC)", TRACK_INDEX.len(), data.builtAt.format("%Y-%m-%d %H:%M:%S")),
            );
        }
        None => log::info(cat::TRACKS_INDEX, "load error / invalid or unreadable index file"),
    }
}

pub fn persist_tracks_index() {
    let _g = PERSIST_LOCK.lock();
    let _ = std::fs::create_dir_all("Data/temp");
    let file = TracksIndexFile { builtAt: crab_core::time::now(), hashes: Some(TRACK_INDEX.iter().map(|h| h.clone()).collect()) };
    let count = file.hashes.as_ref().map(Vec::len).unwrap_or(0);
    write_gz_json(TRACKS_INDEX_PATH, &file);
    INDEX_DIRTY.store(false, Ordering::SeqCst);
    log::info(cat::TRACKS_INDEX, format!("saved {count} hashes / {}", Local::now().format("%Y-%m-%d %H:%M:%S")));
}

pub fn register_track_hash(infohash: &str) {
    let h = normalize_infohash(infohash);
    if !is_valid_infohash(&h) {
        return;
    }
    if TRACK_INDEX.insert(h) {
        INDEX_DIRTY.store(true, Ordering::SeqCst);
    }
}

fn tracks_dir_has_subdirs() -> std::io::Result<bool> {
    let mut rd = std::fs::read_dir(TRACKS_DIR)?;
    Ok(rd.any(|e| e.map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).unwrap_or(false)))
}

/// Rebuild in a background thread when the index is empty but `Data/tracks` has content.
pub fn schedule_index_rebuild_if_needed() {
    if !TRACK_INDEX.is_empty() || !Path::new(TRACKS_DIR).is_dir() {
        return;
    }
    match tracks_dir_has_subdirs() {
        Ok(false) => return,
        Ok(true) => {}
        Err(e) => {
            log::info(cat::TRACKS_INDEX, format!("schedule rebuild skipped / {e}"));
            return;
        }
    }
    let _ = std::thread::Builder::new().name("tracks-index".into()).spawn(build_tracks_index);
}

/// Persist the index every 30 minutes when it changed.
pub fn start_index_persist_loop() {
    if PERSIST_LOOP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("tracks-index-persist".into()).spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30 * 60));
        if INDEX_DIRTY.load(Ordering::SeqCst) {
            persist_tracks_index();
        }
    });
}

/// Full scan of `Data/tracks` (blocking). No-op when a build is already running.
pub fn build_tracks_index() {
    if INDEX_BUILD_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let sw = Instant::now();
    log::info(cat::TRACKS_INDEX, format!("rebuild start / {}", Local::now().format("%Y-%m-%d %H:%M:%S")));

    let mut built = Vec::new();
    scan_tracks_dir_for_index(TRACKS_DIR, &mut built);
    for h in built {
        TRACK_INDEX.insert(h);
    }
    INDEX_DIRTY.store(true, Ordering::SeqCst);
    persist_tracks_index();

    log::info(
        cat::TRACKS_INDEX,
        format!("rebuild done / count={} / {:.1} min", TRACK_INDEX.len(), sw.elapsed().as_secs_f64() / 60.0),
    );
    INDEX_BUILD_RUNNING.store(false, Ordering::SeqCst);

    let _ = std::thread::Builder::new().name("stats-post-index".into()).spawn(|| {
        crate::stats::collect_and_write(false);
    });
}

pub fn scan_tracks_dir_for_index(tracks_dir: &str, target: &mut Vec<String>) {
    paths::walk_tracks_dir(tracks_dir, |e| {
        if e.skip || !is_valid_infohash(&e.infohash) {
            return;
        }
        if !paths::track_file_has_streams(&e.path) {
            return;
        }
        target.push(e.infohash);
    });
}
