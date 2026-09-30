// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Per-tracker statistics: one FileDB pass writes `Data/temp/stats.json` + `stats-meta.json`
//! and the tracks stats cache (`tracks-stats.json`) with the same `updatedAt`.

use chrono::{DateTime, Local, TimeZone, Utc};
use crab_core::conf;
use crab_core::log::{self, cat};
use crab_core::models::TorrentDetails;
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::tracks::models::{format_local, format_utc};
use crate::tracks::{db, index, stats_cache};

pub const STATS_PATH: &str = "Data/temp/stats.json";
pub const STATS_META_PATH: &str = "Data/temp/stats-meta.json";
/// Per-day, per-tracker `new` (torrents created that day) and `all` counts, kept 60 days.
pub const STATS_HISTORY_PATH: &str = "Data/temp/stats-history.json";
const HISTORY_DAYS: i64 = 60;

static COLLECT_LOCK: Mutex<()> = parking_lot::const_mutex(());
static LAST_COLLECTED_AT: Mutex<Option<DateTime<Utc>>> = parking_lot::const_mutex(None);

/// Result of one FileDB pass.
#[derive(Debug, Default)]
pub struct StatsFdbScanResult {
    /// Tracker name (case-insensitive, first spelling kept) → counters, in discovery order.
    pub trackers: IndexMap<String, TrackerStatsRow>,
    /// Lowercase infohashes of rows that carry `ffprobe` streams.
    pub ffprobe_hashes_from_fdb: HashSet<String>,
    pub torrents_scanned: i32,
    pub torrent_db_errors: i32,
    pub magnet_errors: i32,
}

#[derive(Debug, Clone)]
pub struct TrackerStatsRow {
    pub name: String,
    pub last_new_tor: DateTime<Utc>,
    pub new_tor: i32,
    pub update: i32,
    pub check: i32,
    pub all_torrents: i32,
    pub trk_confirm: i32,
    pub trk_wait: i32,
    pub trk_error: i32,
}

pub fn last_collected_at() -> Option<DateTime<Utc>> {
    *LAST_COLLECTED_AT.lock()
}

/// `updatedAt` from `stats-meta.json`.
pub fn try_read_stats_meta_updated_at() -> Option<DateTime<Utc>> {
    let text = std::fs::read_to_string(STATS_META_PATH).ok()?;
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    match v.get("updatedAt")? {
        serde_json::Value::String(s) => crab_core::time::parse_net(s),
        other => crab_core::time::parse_net(&other.to_string()),
    }
}

/// Full collection (blocking): one FDB pass, writes `stats.json`, `stats-meta.json` and `tracks-stats.json`.
/// Deferred (returns the previous timestamp) while the track index is still empty, unless `force`.
pub fn collect_and_write(force: bool) -> Option<DateTime<Utc>> {
    let _g = COLLECT_LOCK.lock();
    if !force && !db::is_track_index_ready_for_stats() {
        log::info(
            cat::STATS,
            format!(
                "deferred - tracks index empty (index={}), waiting for index rebuild / {}",
                index::track_index_count(),
                Local::now().format("%Y-%m-%d %H:%M:%S")
            ),
        );
        return last_collected_at();
    }

    let sw = Instant::now();
    let updated_at = Utc::now();
    let today = Utc.from_utc_datetime(&updated_at.date_naive().and_hms_opt(0, 0, 0).unwrap_or_default());

    let scan = scan_fdb(today);
    write_tracker_stats(&scan.trackers, updated_at);
    write_history(&scan.trackers, updated_at);
    stats_cache::publish_export_stats_cache(updated_at, &scan);

    *LAST_COLLECTED_AT.lock() = Some(updated_at);
    log::info(
        cat::STATS,
        format!(
            "collected {} trackers, torrents={}, tracks total index={} / {:.1}s / {}",
            scan.trackers.len(),
            scan.torrents_scanned,
            index::track_index_count(),
            sw.elapsed().as_secs_f64(),
            Local::now().format("%Y-%m-%d %H:%M:%S")
        ),
    );
    Some(updated_at)
}

/// Async wrapper: runs [`collect_and_write`] on the blocking pool.
pub async fn collect_and_write_async(force: bool) -> Option<DateTime<Utc>> {
    tokio::task::spawn_blocking(move || collect_and_write(force)).await.ok().flatten()
}

/// Walk every FileDB bucket and accumulate tracker counters and ffprobe hashes.
pub fn scan_fdb(today_utc: DateTime<Utc>) -> StatsFdbScanResult {
    let mut result = StatsFdbScanResult::default();
    let _pace = crab_core::fdb::pace::scan();
    for (key, _) in crab_core::fdb::master_db_snapshot() {
        let shard = crab_core::fdb::open_read(&key, false, false);
        for t in shard.values() {
            if t.trackerName.is_empty() {
                continue;
            }
            accumulate_tracker(&mut result, t, today_utc);
            result.torrents_scanned += 1;
            if t.ffprobe.as_ref().map(|f| !f.is_empty()).unwrap_or(false) && !t.magnet.is_empty() {
                match db::infohash_from_magnet(&t.magnet) {
                    Some(h) => {
                        result.ffprobe_hashes_from_fdb.insert(h);
                    }
                    None => result.magnet_errors += 1,
                }
            }
        }
    }
    result
}

/// Add one row to its tracker's counters.
pub fn accumulate_tracker(result: &mut StatsFdbScanResult, t: &TorrentDetails, today_utc: DateTime<Utc>) {
    let row = result.trackers.entry(t.trackerName.to_lowercase()).or_insert_with(|| TrackerStatsRow {
        name: t.trackerName.clone(),
        last_new_tor: t.createTime,
        new_tor: 0,
        update: 0,
        check: 0,
        all_torrents: 0,
        trk_confirm: 0,
        trk_wait: 0,
        trk_error: 0,
    });
    row.all_torrents += 1;
    if t.createTime > row.last_new_tor {
        row.last_new_tor = t.createTime;
    }
    if t.createTime >= today_utc {
        row.new_tor += 1;
    }
    if t.updateTime >= today_utc {
        row.update += 1;
    }
    if t.checkTime >= today_utc {
        row.check += 1;
    }
    if !db::the_bad(&t.types) && !t.magnet.is_empty() {
        if t.ffprobe_tryingdata >= conf().tracksatempt {
            row.trk_error += 1;
        } else if db::has_track_for_torrent(t) {
            row.trk_confirm += 1;
        } else {
            row.trk_wait += 1;
        }
    }
}

/// `stats.json` payload (sorted by total torrents, descending).
pub fn tracker_stats_json(trackers: &IndexMap<String, TrackerStatsRow>) -> serde_json::Value {
    let mut rows: Vec<&TrackerStatsRow> = trackers.values().collect();
    rows.sort_by(|a, b| b.all_torrents.cmp(&a.all_torrents));
    serde_json::Value::Array(
        rows.into_iter()
            .map(|r| {
                json!({
                    "trackerName": r.name,
                    "lastnewtor": r.last_new_tor.format("%d.%m.%Y").to_string(),
                    "newtor": r.new_tor,
                    "update": r.update,
                    "check": r.check,
                    "alltorrents": r.all_torrents,
                    "tracks": { "wait": r.trk_wait, "confirm": r.trk_confirm, "skip": r.trk_error }
                })
            })
            .collect(),
    )
}

fn write_tracker_stats(trackers: &IndexMap<String, TrackerStatsRow>, updated_at: DateTime<Utc>) {
    let payload = tracker_stats_json(trackers);
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        if let Err(e) = write_text_atomic(STATS_PATH, &text) {
            log::error(cat::STATS, format!("error / {e}"));
        }
    }
    let meta = json!({
        "updatedAt": format_utc(&updated_at),
        "updatedAtLocal": format_local(&updated_at),
        "trackerCount": trackers.len(),
    });
    if let Ok(text) = serde_json::to_string_pretty(&meta) {
        if let Err(e) = write_text_atomic(STATS_META_PATH, &text) {
            log::error(cat::STATS, format!("error / {e}"));
        }
    }
}

fn read_history_file() -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(STATS_HISTORY_PATH)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("days").and_then(|d| d.as_object().cloned()))
        .unwrap_or_default()
}

/// Today's row of the history: the last collection of a day is its final value (`newtor`
/// counts rows created that day, so it only grows during the day). Older days are kept as
/// written; days beyond [`HISTORY_DAYS`] are dropped.
fn write_history(trackers: &IndexMap<String, TrackerStatsRow>, updated_at: DateTime<Utc>) {
    let mut days = read_history_file();
    let today = updated_at.format("%Y-%m-%d").to_string();
    let mut row = serde_json::Map::new();
    for (slug, r) in trackers {
        row.insert(slug.clone(), json!({ "new": r.new_tor, "all": r.all_torrents }));
    }
    days.insert(today, serde_json::Value::Object(row));
    let cutoff = (updated_at - chrono::Duration::days(HISTORY_DAYS)).format("%Y-%m-%d").to_string();
    days.retain(|d, _| d.as_str() >= cutoff.as_str());
    if let Ok(text) = serde_json::to_string(&json!({ "days": days })) {
        if let Err(e) = write_text_atomic(STATS_HISTORY_PATH, &text) {
            log::error(cat::STATS, format!("history: {e}"));
        }
    }
}

/// `{ days: [dates asc], trackers: { slug: { new: [...], all: [...] } } }` for the last `n`
/// days (gaps are `null`), for the fill charts in the admin panel.
pub fn history_json(n: usize) -> serde_json::Value {
    let days = read_history_file();
    let mut dates: Vec<&String> = days.keys().collect();
    dates.sort();
    let n = n.clamp(1, HISTORY_DAYS as usize);
    let dates: Vec<&String> = dates.into_iter().rev().take(n).rev().collect();
    let mut slugs: Vec<String> = days.values().filter_map(|v| v.as_object()).flat_map(|o| o.keys().cloned()).collect();
    slugs.sort();
    slugs.dedup();
    let mut trackers = serde_json::Map::new();
    for slug in slugs {
        let pick = |field: &str| -> Vec<serde_json::Value> { dates.iter().map(|d| days[*d].get(&slug).and_then(|r| r.get(field)).cloned().unwrap_or(serde_json::Value::Null)).collect() };
        trackers.insert(slug.clone(), json!({ "new": pick("new"), "all": pick("all") }));
    }
    json!({ "days": dates, "trackers": trackers })
}

/// Write via `{path}.tmp` + rename.
pub fn write_text_atomic(path: &str, content: &str) -> std::io::Result<()> {
    let p = std::path::Path::new(path);
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Raw `stats.json` contents, or `[]`.
pub fn read_all_json() -> String {
    std::fs::read_to_string(STATS_PATH).unwrap_or_else(|_| "[]".to_string())
}

/// Background loop: first collection after 20s, then every `timeStatsUpdate` minutes (-1 = paused).
pub async fn run_cron(shutdown: CancellationToken) {
    let wait = |d: Duration| {
        let s = shutdown.clone();
        async move {
            tokio::select! {
                _ = s.cancelled() => false,
                _ = tokio::time::sleep(d) => true,
            }
        }
    };
    if !wait(Duration::from_secs(20)).await {
        return;
    }
    collect_and_write_async(false).await;

    loop {
        let minutes = conf().timeStatsUpdate;
        if minutes == -1 {
            if !wait(Duration::from_secs(60)).await {
                return;
            }
            continue;
        }
        if !wait(Duration::from_secs(minutes.max(0) as u64 * 60)).await {
            return;
        }
        collect_and_write_async(false).await;
    }
}
