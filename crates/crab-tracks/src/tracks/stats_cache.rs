//! Tracks export statistics cached in `Data/temp/tracks-stats.json`.

use chrono::{DateTime, Local, Utc};
use crab_core::log::{self, cat};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::db::{self, DATABASE};
use super::index::{self, TRACK_INDEX};
use super::models::{TracksExportStats, TracksStatsCacheEntry, TracksStatsCacheFile};
use super::paths;
use crate::stats::{self, StatsFdbScanResult};

pub const TRACKS_STATS_PATH: &str = "Data/temp/tracks-stats.json";

static STATS_CACHE_LOCK: Mutex<()> = parking_lot::const_mutex(());
static STATS_CACHE_UPDATED_AT: Mutex<Option<DateTime<Utc>>> = parking_lot::const_mutex(None);
static LAST_FROM_CACHE: AtomicBool = AtomicBool::new(false);

pub fn stats_cache_updated_at() -> Option<DateTime<Utc>> {
    *STATS_CACHE_UPDATED_AT.lock()
}

pub fn last_export_stats_from_cache() -> bool {
    LAST_FROM_CACHE.load(Ordering::SeqCst)
}

pub fn try_load_stats_cache_on_startup() {
    let _ = try_load_stats_cache(true);
}

/// Stats from the track index (or a directory scan), memory, and FileDB.
pub fn build_export_stats(include_torrent_db: bool, scan: Option<&StatsFdbScanResult>) -> TracksExportStats {
    let mut seen: HashSet<String> = HashSet::new();
    let mut stats = TracksExportStats::default();

    apply_track_index_to_seen(&mut seen, &mut stats);

    for e in DATABASE.iter() {
        if e.value().streams.as_ref().map(|s| !s.is_empty()).unwrap_or(false) && seen.insert(e.key().to_lowercase()) {
            stats.fromMemory += 1;
        }
    }

    if include_torrent_db {
        match scan {
            Some(scan) => {
                stats.torrentsScanned = scan.torrents_scanned;
                stats.torrentDbErrors = scan.torrent_db_errors;
                stats.magnetErrors = scan.magnet_errors;
                for h in &scan.ffprobe_hashes_from_fdb {
                    if seen.insert(h.to_lowercase()) {
                        stats.fromTorrentDb += 1;
                    }
                }
            }
            None => collect_stats_from_torrent_db(&mut seen, &mut stats),
        }
    }

    stats.total = seen.len() as i32;
    stats
}

fn apply_track_index_to_seen(seen: &mut HashSet<String>, stats: &mut TracksExportStats) {
    if index::track_index_count() > 0 {
        stats.filesScanned = index::track_index_count() as i32;
        for h in TRACK_INDEX.iter() {
            if seen.insert(h.to_lowercase()) {
                stats.fromTracksFiles += 1;
            }
        }
        return;
    }
    collect_stats_from_tracks_dir(paths::TRACKS_DIR, seen, stats);
}

fn collect_stats_from_tracks_dir(tracks_dir: &str, seen: &mut HashSet<String>, stats: &mut TracksExportStats) {
    paths::walk_tracks_dir(tracks_dir, |e| {
        stats.filesScanned += 1;
        if e.skip {
            return;
        }
        if !paths::is_valid_infohash(&e.infohash) {
            stats.invalidPath += 1;
            return;
        }
        if !paths::track_file_has_streams(&e.path) {
            stats.emptyStreams += 1;
            return;
        }
        if seen.insert(e.infohash) {
            stats.fromTracksFiles += 1;
        }
    });
}

fn collect_stats_from_torrent_db(seen: &mut HashSet<String>, stats: &mut TracksExportStats) {
    for (key, _) in crab_core::fdb::master_db_snapshot() {
        let shard = crab_core::fdb::open_read(&key, false, false);
        for t in shard.values() {
            stats.torrentsScanned += 1;
            if t.ffprobe.as_ref().map(|f| f.is_empty()).unwrap_or(true) || t.magnet.is_empty() {
                continue;
            }
            match db::infohash_from_magnet(&t.magnet).filter(|h| paths::is_valid_infohash(h)) {
                Some(h) => {
                    if seen.insert(h) {
                        stats.fromTorrentDb += 1;
                    }
                }
                None => stats.magnetErrors += 1,
            }
        }
    }
}

/// Write `tracks-stats.json` from a shared FDB scan.
pub fn publish_export_stats_cache(updated_at: DateTime<Utc>, scan: &StatsFdbScanResult) -> DateTime<Utc> {
    let _g = STATS_CACHE_LOCK.lock();
    let cache = TracksStatsCacheFile {
        updatedAt: updated_at,
        entries: Some(vec![
            TracksStatsCacheEntry { includeTorrentDb: true, stats: Some(build_export_stats(true, Some(scan))) },
            TracksStatsCacheEntry { includeTorrentDb: false, stats: Some(build_export_stats(false, Some(scan))) },
        ]),
    };
    write_stats_cache_file(&cache);
    let total = cache.entries.as_ref().and_then(|e| e[0].stats.as_ref()).map(|s| s.total).unwrap_or(0);
    log::info(
        cat::TRACKS_STATS,
        format!("wrote cache to {TRACKS_STATS_PATH} / total={total} / {}", Local::now().format("%Y-%m-%d %H:%M:%S")),
    );
    updated_at
}

/// Cached entry for `include_torrent_db`, if the cache file is readable.
pub fn try_load_stats_cache(include_torrent_db: bool) -> Option<TracksExportStats> {
    if !Path::new(TRACKS_STATS_PATH).exists() {
        return None;
    }
    let text = std::fs::read_to_string(TRACKS_STATS_PATH).ok()?;
    let cache: TracksStatsCacheFile = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let entries = cache.entries.filter(|e| !e.is_empty())?;
    *STATS_CACHE_UPDATED_AT.lock() = Some(cache.updatedAt);
    entries.into_iter().find(|e| e.includeTorrentDb == include_torrent_db).and_then(|e| e.stats)
}

fn write_stats_cache_file(cache: &TracksStatsCacheFile) {
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        if let Err(e) = stats::write_text_atomic(TRACKS_STATS_PATH, &json) {
            log::error(cat::TRACKS_STATS, format!("write error / {e}"));
        }
    }
    *STATS_CACHE_UPDATED_AT.lock() = Some(cache.updatedAt);
}

/// Force a full stats collection; returns its timestamp.
pub fn refresh_export_stats_cache() -> DateTime<Utc> {
    stats::collect_and_write(true).unwrap_or_else(Utc::now)
}

/// Cached stats unless `refresh`; otherwise (or on a cache miss) run a full collection. Blocking.
pub fn get_export_stats(include_torrent_db: bool, refresh: bool) -> TracksExportStats {
    if !refresh {
        if let Some(s) = try_load_stats_cache(include_torrent_db) {
            LAST_FROM_CACHE.store(true, Ordering::SeqCst);
            return s;
        }
        let _g = STATS_CACHE_LOCK.lock();
        if let Some(s) = try_load_stats_cache(include_torrent_db) {
            LAST_FROM_CACHE.store(true, Ordering::SeqCst);
            return s;
        }
    }

    LAST_FROM_CACHE.store(false, Ordering::SeqCst);
    stats::collect_and_write(true);

    let _g = STATS_CACHE_LOCK.lock();
    try_load_stats_cache(include_torrent_db).unwrap_or_else(|| build_export_stats(include_torrent_db, None))
}
