//! Export of all known tracks to a directory (`{aa}/{b}/{rest}.json`) and backfill of `Data/tracks`.

use crab_core::log::{self, cat};
use crab_core::models::FfprobeModel;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::db::{self, DATABASE};
use super::models::{
    self, format_utc, ErrorSample, TracksBackfillResult, TracksExportJobStatus, TracksExportResult, TracksExportStats,
};
use super::paths::{self, is_valid_infohash};
use super::{index, stats_cache};

pub const DEFAULT_EXPORT_DIR: &str = "Data/tracks-export";

static EXPORT_RUNNING: AtomicBool = AtomicBool::new(false);
static EXPORT_JOB: Lazy<Mutex<Arc<Mutex<TracksExportJobStatus>>>> =
    Lazy::new(|| Mutex::new(Arc::new(Mutex::new(TracksExportJobStatus::default()))));

/// Invalid export directory (must be a valid path inside `Data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidOutputDir(pub String);

impl std::fmt::Display for InvalidOutputDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Absolute export directory; must stay under `Data/`.
pub fn resolve_export_output_dir(output_dir: &str) -> Result<String, InvalidOutputDir> {
    if crab_core::util::is_blank(output_dir) {
        return Err(InvalidOutputDir("Export output directory is required.".into()));
    }
    if output_dir.contains('\0') {
        return Err(InvalidOutputDir("Export output directory contains invalid path characters.".into()));
    }
    let full = paths::full_path(output_dir).ok_or_else(|| InvalidOutputDir("Export output directory is not a valid path.".into()))?;
    let full = full.to_string_lossy().to_string();
    if !paths::is_path_within_directory("Data", &full) {
        return Err(InvalidOutputDir("Export output directory must be inside Data.".into()));
    }
    Ok(full)
}

/// infohash → model, keyed lowercase.
pub type TrackMap = IndexMap<String, Arc<FfprobeModel>>;

/// Everything known: track files, then memory, then (optionally) FileDB `ffprobe` fields.
pub fn collect_all(include_torrent_db: bool) -> TrackMap {
    let mut result = TrackMap::new();
    collect_all_into(&mut result, None, include_torrent_db);
    result
}

pub fn collect_all_into(result: &mut TrackMap, mut stats: Option<&mut TracksExportStats>, include_torrent_db: bool) {
    collect_from_tracks_dir(paths::TRACKS_DIR, result, stats.as_deref_mut());

    for e in DATABASE.iter() {
        let key = e.key().to_lowercase();
        if e.value().streams.as_ref().map(|s| !s.is_empty()).unwrap_or(false) && !result.contains_key(&key) {
            result.insert(key, e.value().clone());
            if let Some(s) = stats.as_deref_mut() {
                s.fromMemory += 1;
            }
        }
    }

    if include_torrent_db {
        collect_from_torrent_db(result, stats);
    }
}

fn collect_from_tracks_dir(tracks_dir: &str, result: &mut TrackMap, mut stats: Option<&mut TracksExportStats>) {
    paths::walk_tracks_dir(tracks_dir, |e| {
        if let Some(s) = stats.as_deref_mut() {
            s.filesScanned += 1;
        }
        if e.skip {
            return;
        }
        if !is_valid_infohash(&e.infohash) {
            if let Some(s) = stats.as_deref_mut() {
                s.invalidPath += 1;
            }
            return;
        }
        let parsed = std::fs::read_to_string(&e.path).ok().and_then(|t| models::parse_ffprobe(&t).ok());
        match parsed {
            None => {
                if let Some(s) = stats.as_deref_mut() {
                    s.readErrors += 1;
                }
            }
            Some(m) => match m.filter(|m| m.streams.as_ref().map(|s| !s.is_empty()).unwrap_or(false)) {
                None => {
                    if let Some(s) = stats.as_deref_mut() {
                        s.emptyStreams += 1;
                    }
                }
                Some(m) => {
                    result.insert(e.infohash, Arc::new(m));
                    if let Some(s) = stats.as_deref_mut() {
                        s.fromTracksFiles += 1;
                    }
                }
            },
        }
    });
}

fn collect_from_torrent_db(result: &mut TrackMap, mut stats: Option<&mut TracksExportStats>) {
    for (key, _) in crab_core::fdb::master_db_snapshot() {
        let shard = crab_core::fdb::open_read(&key, false, false);
        for t in shard.values() {
            if let Some(s) = stats.as_deref_mut() {
                s.torrentsScanned += 1;
            }
            let Some(ff) = t.ffprobe.as_ref().filter(|f| !f.is_empty()) else { continue };
            if t.magnet.is_empty() {
                continue;
            }
            let Some(h) = db::infohash_from_magnet(&t.magnet).filter(|h| is_valid_infohash(h)) else {
                if let Some(s) = stats.as_deref_mut() {
                    s.magnetErrors += 1;
                }
                continue;
            };
            if result.contains_key(&h) {
                continue;
            }
            result.insert(h, Arc::new(FfprobeModel { streams: Some(ff.clone()) }));
            if let Some(s) = stats.as_deref_mut() {
                s.fromTorrentDb += 1;
            }
        }
    }
}

pub fn get_export_job_status() -> TracksExportJobStatus {
    let job = EXPORT_JOB.lock().clone();
    let g = job.lock();
    g.clone()
}

/// Start `export_all` in a background thread. `Ok(false)` when an export is already running.
pub fn try_start_export(output_dir: &str, include_torrent_db: bool) -> Result<bool, InvalidOutputDir> {
    let output_dir = resolve_export_output_dir(output_dir)?;

    let job = {
        let mut slot = EXPORT_JOB.lock();
        if EXPORT_RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(false);
        }
        let job = Arc::new(Mutex::new(TracksExportJobStatus {
            running: true,
            phase: Some("collecting".into()),
            outputDir: Some(output_dir.clone()),
            includeTorrentDb: include_torrent_db,
            startedAt: Some(crab_core::time::now()),
            ..Default::default()
        }));
        *slot = job.clone();
        job
    };

    let spawn = std::thread::Builder::new().name("tracks-export".into()).spawn({
        let job = job.clone();
        move || {
            let res = export_all(&output_dir, false, include_torrent_db, Some(&job));
            {
                let mut g = job.lock();
                g.running = false;
                g.completedAt = Some(crab_core::time::now());
                match res {
                    Ok(result) => {
                        g.phase = Some("done".into());
                        g.written = result.written;
                        g.writeErrors = result.writeErrors;
                        g.total = result.stats.as_ref().map(|s| s.total).unwrap_or(0);
                        g.result = Some(result);
                    }
                    Err(e) => {
                        g.phase = Some("error".into());
                        g.error = Some(e.0.clone());
                        log::error(cat::TRACKS_EXPORT, e.0);
                    }
                }
            }
            EXPORT_RUNNING.store(false, Ordering::SeqCst);
        }
    });
    if let Err(e) = spawn {
        let mut g = job.lock();
        g.running = false;
        g.phase = Some("error".into());
        g.error = Some(e.to_string());
        EXPORT_RUNNING.store(false, Ordering::SeqCst);
    }
    Ok(true)
}

/// Export all tracks as JSON files (blocking). `dry_run` only computes stats.
pub fn export_all(
    output_dir: &str,
    dry_run: bool,
    include_torrent_db: bool,
    progress: Option<&Mutex<TracksExportJobStatus>>,
) -> Result<TracksExportResult, InvalidOutputDir> {
    let output_dir = resolve_export_output_dir(output_dir)?;
    let mut result = TracksExportResult {
        outputDir: output_dir.clone(),
        dryRun: dry_run,
        includeTorrentDb: include_torrent_db,
        ..Default::default()
    };

    if let Some(p) = progress {
        let mut g = p.lock();
        g.phase = Some("collecting".into());
        g.outputDir = Some(output_dir.clone());
        g.includeTorrentDb = include_torrent_db;
    }

    if dry_run {
        let stats = stats_cache::build_export_stats(include_torrent_db, None);
        if let Some(p) = progress {
            let mut g = p.lock();
            g.phase = Some("done".into());
            g.total = stats.total;
            g.stats = Some(stats.clone());
        }
        result.stats = Some(stats);
        return Ok(result);
    }

    let mut data = TrackMap::new();
    let mut stats = TracksExportStats::default();
    collect_all_into(&mut data, Some(&mut stats), include_torrent_db);
    stats.total = data.len() as i32;

    if let Some(p) = progress {
        let mut g = p.lock();
        g.phase = Some("writing".into());
        g.total = data.len() as i32;
        g.stats = Some(stats.clone());
    }

    let _ = std::fs::create_dir_all(&output_dir);

    for (hash, model) in &data {
        let Some(path) = paths::track_layout_path(&output_dir, hash, true).filter(|_| is_valid_infohash(hash)) else {
            result.writeErrors += 1;
            continue;
        };
        let write = (|| {
            if let Some(dir) = Path::new(&path).parent() {
                std::fs::create_dir_all(dir)?;
            }
            models::write_track_file(Path::new(&path), model)
        })();
        match write {
            Ok(()) => {
                result.written += 1;
                if let Some(p) = progress {
                    p.lock().written = result.written;
                }
            }
            Err(e) => {
                result.writeErrors += 1;
                if result.errorSamples.len() < 10 {
                    result.errorSamples.push(ErrorSample { hash: hash.clone(), error: e.to_string() });
                }
                if let Some(p) = progress {
                    p.lock().writeErrors = result.writeErrors;
                }
            }
        }
    }

    let manifest = json!({
        "exportedAt": format_utc(&crab_core::time::now()),
        "outputDir": output_dir,
        "includeTorrentDb": include_torrent_db,
        "stats": stats,
        "written": result.written,
        "writeErrors": result.writeErrors,
    });
    if let Ok(text) = serde_json::to_string_pretty(&manifest) {
        let _ = models::write_text_bom(&Path::new(&output_dir).join("export-manifest.json"), &text);
    }

    result.stats = Some(stats);
    Ok(result)
}

/// Backfill `tracks_dir`: migrate legacy layouts and write missing tracks from memory/FileDB (blocking).
pub fn backfill_tracks(tracks_dir: &str, dry_run: bool, include_torrent_db: bool, migrate_legacy: bool) -> TracksBackfillResult {
    let mut result = TracksBackfillResult {
        tracksDir: tracks_dir.to_string(),
        dryRun: dry_run,
        includeTorrentDb: include_torrent_db,
        migrateLegacy: migrate_legacy,
        ..Default::default()
    };

    let mut data = TrackMap::new();
    let mut stats = TracksExportStats::default();
    collect_all_into(&mut data, Some(&mut stats), include_torrent_db);
    stats.total = data.len() as i32;
    result.stats = stats;

    if migrate_legacy {
        result.migratedLegacy = paths::migrate_track_layout_in_place(tracks_dir, dry_run);
    }

    if dry_run {
        for hash in data.keys() {
            if paths::resolve_track_json_path(hash, tracks_dir).is_some() {
                result.skippedExisting += 1;
            } else {
                result.written += 1;
            }
        }
        return result;
    }

    let _ = std::fs::create_dir_all(tracks_dir);

    for (hash, model) in &data {
        if paths::resolve_track_json_path(hash, tracks_dir).is_some() {
            result.skippedExisting += 1;
            continue;
        }
        let write = (|| {
            let path = paths::track_layout_path(tracks_dir, hash, true)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid infohash. (Parameter 'infohash')"))?;
            if let Some(dir) = Path::new(&path).parent() {
                std::fs::create_dir_all(dir)?;
            }
            models::write_track_file(Path::new(&path), model)
        })();
        match write {
            Ok(()) => {
                DATABASE.insert(hash.clone(), model.clone());
                index::register_track_hash(hash);
                result.written += 1;
            }
            Err(e) => {
                result.writeErrors += 1;
                if result.errorSamples.len() < 10 {
                    result.errorSamples.push(ErrorSample { hash: hash.clone(), error: e.to_string() });
                }
            }
        }
    }

    let manifest = json!({
        "backfilledAt": format_utc(&crab_core::time::now()),
        "tracksDir": tracks_dir,
        "includeTorrentDb": include_torrent_db,
        "migrateLegacy": migrate_legacy,
        "stats": result.stats,
        "written": result.written,
        "migratedLegacy": result.migratedLegacy,
        "skippedExisting": result.skippedExisting,
        "writeErrors": result.writeErrors,
    });
    if let Ok(text) = serde_json::to_string_pretty(&manifest) {
        let _ = models::write_text_bom(&Path::new(tracks_dir).join("backfill-manifest.json"), &text);
    }

    result
}
