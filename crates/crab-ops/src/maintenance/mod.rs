//! FileDB integrity maintenance (`/cron/maintenance/*`, `maintain` CLI).
//!
//! Modes:
//! * `report` - read-only scan, report written to `Data/temp/maintenance-last.json`;
//! * `safe`   - scan + fill empty `_sn/_so`/name fields, drop null rows, migrate rows whose
//!   bucket key changed, drop empty buckets from masterDb;
//! * `full`   - `safe` + remove rows without magnet and types, re-key rows whose dict key
//!   differs from `url`, remigrate bucket mismatches, drop masterDb keys without shard file,
//!   delete orphan shard files.

pub mod resume;

use axum::extract::Query;
use axum::routing::any;
use axum::Router;
use chrono::{DateTime, Utc};
use crab_core::fdb;
use crab_core::log::{self, cat};
use crab_core::models::TorrentDetails;
use crab_core::trackers::{self, Cancelled, WorkFlag};
use crab_core::{index, time, util};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::nulls;
use crate::query::{self, s_or_null, Params};

pub const REPORT_PATH: &str = "Data/temp/maintenance-last.json";
const MAX_DURATION: Duration = Duration::from_secs(6 * 3600);

static WORK_FLAG: WorkFlag = WorkFlag::new();

#[derive(Default)]
struct RunState {
    running: bool,
    mode: Option<String>,
    started_at: Option<DateTime<Utc>>,
    detail: Option<String>,
}

static STATE: Lazy<Mutex<RunState>> = Lazy::new(|| Mutex::new(RunState::default()));
static PROGRESS_CURRENT: AtomicI64 = AtomicI64::new(0);
static PROGRESS_TOTAL: AtomicI64 = AtomicI64::new(0);
static LAST_REPORT: Lazy<Mutex<Option<Value>>> = Lazy::new(|| Mutex::new(load_last_report()));

fn load_last_report() -> Option<Value> {
    let s = std::fs::read_to_string(REPORT_PATH).ok()?;
    serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/maintenance/check", any(check_handler))
        .route("/cron/maintenance/status", any(status_handler))
        .route("/cron/maintenance/resumeparseall", any(resume_handler))
        .route("/cron/maintenance/parseallstatus", any(parse_all_status_handler))
}

async fn check_handler(q: Query<HashMap<String, String>>) -> String {
    let p = Params::from_query(q);
    check(p.str("mode").unwrap_or("report"), p.i32("samplesize", 20), p.bool("excludenumericxx", true))
}

async fn status_handler() -> axum::Json<Value> {
    query::json(status())
}

async fn resume_handler() -> axum::Json<Value> {
    query::json(resume::resume().await)
}

async fn parse_all_status_handler() -> axum::Json<Value> {
    query::json(serde_json::to_value(resume::status()).unwrap_or(Value::Null))
}

pub fn normalize_mode(mode: &str) -> String {
    if util::is_blank(mode) {
        return "report".into();
    }
    let m = mode.trim().to_lowercase();
    if m == "safe" || m == "full" {
        m
    } else {
        "report".into()
    }
}

pub fn clamp_sample_size(n: i32) -> i32 {
    if n < 1 {
        20
    } else if n > 200 {
        200
    } else {
        n
    }
}

/// `name:name` bucket keys (optionally ignoring purely numeric names like `1899:1899`).
pub fn is_xx_key(key: &str, exclude_numeric: bool) -> bool {
    if key.is_empty() {
        return false;
    }
    let Some(colon) = key.find(':') else { return false };
    if colon == 0 || colon >= key.len() - 1 {
        return false;
    }
    let (p1, p2) = (&key[..colon], &key[colon + 1..]);
    if p1.to_lowercase() != p2.to_lowercase() {
        return false;
    }
    if exclude_numeric && !p1.is_empty() && p1.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

/// Start the check in the background. Returns `ok` / `work`.
pub fn check(mode: &str, sample_size: i32, exclude_numeric_xx: bool) -> String {
    let mode = normalize_mode(mode);
    let sample_size = clamp_sample_size(sample_size);
    trackers::run_in_background(
        "maintenance",
        "Check",
        &WORK_FLAG,
        false,
        move |ct: CancellationToken| async move {
            let r = tokio::task::spawn_blocking(move || run(&mode, sample_size, exclude_numeric_xx, &ct, false)).await;
            match r {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(c)) => Err(c.into()),
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        },
        Some(MAX_DURATION),
    )
}

/// In-progress state + last completed report (nulls omitted by the handler).
pub fn status() -> Value {
    let st = STATE.lock();
    let progress = if st.running {
        json!({
            "current": PROGRESS_CURRENT.load(Ordering::SeqCst),
            "total": PROGRESS_TOTAL.load(Ordering::SeqCst),
            "detail": st.detail,
        })
    } else {
        Value::Null
    };
    json!({
        "ok": true,
        "running": st.running,
        "mode": st.mode,
        "startedAt": st.started_at.map(|d| time::format_net(&d)),
        "progress": progress,
        "last": LAST_REPORT.lock().clone(),
    })
}

fn log_progress(console: bool, msg: &str) {
    if console {
        println!("[{}] {msg}", chrono::Local::now().format("%H:%M:%S"));
    }
}

fn set_detail(d: &str) {
    STATE.lock().detail = Some(d.to_string());
}

/// Issue counter with a bounded sample list.
pub struct IssueBucket {
    sample_size: usize,
    sample: Vec<Value>,
    pub count: i64,
}

impl IssueBucket {
    pub fn new(sample_size: i32) -> Self {
        IssueBucket { sample_size: sample_size.max(0) as usize, sample: Vec::new(), count: 0 }
    }

    pub fn add(&mut self, v: Value) {
        self.count += 1;
        if self.sample.len() < self.sample_size {
            self.sample.push(v);
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "count": self.count, "sample": self.sample })
    }
}

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct FixedCounts {
    pub nullRemoved: i64,
    pub searchFieldsFixed: i64,
    pub migrated: i64,
    pub emptyBucketsRemoved: i64,
    pub missingShardKeysRemoved: i64,
    pub orphansDeleted: i64,
    pub urlKeyFixed: i64,
    pub incompleteRemoved: i64,
}

fn full_path_lower(p: &str) -> String {
    let path = Path::new(p);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = std::path::PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            x => out.push(x.as_os_str()),
        }
    }
    out.to_string_lossy().to_lowercase()
}

/// Every file under `Data/fdb` (relative paths like `Data/fdb/ab/cdef…`), skipping `*.tmp`.
fn shard_files(ct: &CancellationToken) -> Result<Vec<String>, Cancelled> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("Data/fdb")];
    while let Some(dir) = stack.pop() {
        trackers::check(ct)?;
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let s = p.to_string_lossy().to_string();
                if !s.to_lowercase().ends_with(".tmp") {
                    out.push(s);
                }
            }
        }
    }
    Ok(out)
}

fn row_ref(fdb_key: &str, url: &str, t: &TorrentDetails) -> Value {
    json!({ "fdbKey": fdb_key, "url": url, "title": s_or_null(&t.title) })
}

fn row_names(fdb_key: &str, url: &str, t: &TorrentDetails) -> Value {
    json!({
        "fdbKey": fdb_key, "url": url, "title": s_or_null(&t.title),
        "name": s_or_null(&t.name), "originalname": s_or_null(&t.originalname)
    })
}

fn scan(sample_size: i32, exclude_numeric_xx: bool, ct: &CancellationToken, console: bool) -> Result<Map<String, Value>, Cancelled> {
    let mut null_value = IssueBucket::new(sample_size);
    let mut missing_name = IssueBucket::new(sample_size);
    let mut missing_originalname = IssueBucket::new(sample_size);
    let mut missing_tracker_name = IssueBucket::new(sample_size);
    let mut empty_sn = IssueBucket::new(sample_size);
    let mut empty_so = IssueBucket::new(sample_size);
    let mut empty_both = IssueBucket::new(sample_size);
    let mut bucket_mismatch = IssueBucket::new(sample_size);
    let mut url_key_mismatch = IssueBucket::new(sample_size);
    let mut empty_magnet_or_types = IssueBucket::new(sample_size);
    let mut missing_shard_file = IssueBucket::new(sample_size);
    let mut empty_shard_listed = IssueBucket::new(sample_size);
    let mut xx_keys = IssueBucket::new(sample_size);

    let mut expected: HashSet<String> = HashSet::new();
    let mut total_torrents: i64 = 0;
    let master = fdb::master_db_snapshot();
    let n = master.len();
    PROGRESS_TOTAL.store(n as i64, Ordering::SeqCst);
    let mut last_log = Instant::now();

    for (i, (fdb_key, _)) in master.iter().enumerate() {
        trackers::check(ct)?;
        PROGRESS_CURRENT.store(i as i64 + 1, Ordering::SeqCst);
        if console && (i == 0 || (i + 1) % 1000 == 0 || last_log.elapsed().as_secs_f64() >= 5.0 || i + 1 == n) {
            log_progress(true, &format!("maintenance: scan {}/{}", i + 1, n));
            last_log = Instant::now();
        }

        let shard_path = fdb::path_for_key(fdb_key);
        expected.insert(full_path_lower(&shard_path));

        if is_xx_key(fdb_key, exclude_numeric_xx) {
            xx_keys.add(json!({ "key": fdb_key }));
        }

        if !Path::new(&shard_path).exists() {
            missing_shard_file.add(json!({ "fdbKey": fdb_key, "path": shard_path }));
            continue;
        }

        let (db, null_urls) = nulls::load_rows_lenient(fdb_key);
        if db.is_empty() && null_urls.is_empty() {
            empty_shard_listed.add(json!({ "fdbKey": fdb_key, "path": shard_path }));
            continue;
        }
        for url in &null_urls {
            total_torrents += 1;
            null_value.add(json!({ "fdbKey": fdb_key, "url": url }));
        }

        for (url, t) in &db {
            total_torrents += 1;
            if util::is_blank(&t.trackerName) {
                missing_tracker_name.add(row_ref(fdb_key, url, t));
            }
            if util::is_blank(&t.name) {
                missing_name.add(row_ref(fdb_key, url, t));
            }
            if util::is_blank(&t.originalname) {
                missing_originalname.add(row_ref(fdb_key, url, t));
            }

            let esn = util::is_blank(&t._sn);
            let eso = util::is_blank(&t._so);
            if esn && eso {
                empty_both.add(row_names(fdb_key, url, t));
            } else if esn {
                empty_sn.add(row_names(fdb_key, url, t));
            } else if eso {
                empty_so.add(row_names(fdb_key, url, t));
            }

            let expected_key = fdb::key_for_torrent(&t.name, &t.originalname);
            if !expected_key.is_empty() && &expected_key != fdb_key {
                bucket_mismatch.add(json!({
                    "fdbKey": fdb_key, "expectedKey": expected_key, "url": url, "title": s_or_null(&t.title),
                    "name": s_or_null(&t.name), "originalname": s_or_null(&t.originalname)
                }));
            }

            if !t.url.is_empty() && &t.url != url {
                url_key_mismatch.add(json!({
                    "fdbKey": fdb_key, "dictKey": url, "torrentUrl": t.url, "title": s_or_null(&t.title)
                }));
            }

            let empty_magnet = util::is_blank(&t.magnet);
            let empty_types = t.types.is_empty();
            if empty_magnet || empty_types {
                empty_magnet_or_types.add(json!({
                    "fdbKey": fdb_key, "url": url, "title": s_or_null(&t.title),
                    "emptyMagnet": empty_magnet, "emptyTypes": empty_types
                }));
            }
        }
    }

    let mut orphans = IssueBucket::new(sample_size);
    if Path::new("Data/fdb").is_dir() {
        for f in shard_files(ct)? {
            if !expected.contains(&full_path_lower(&f)) {
                orphans.add(json!({ "path": f }));
            }
        }
    }

    let mut report = Map::new();
    report.insert("totals".into(), json!({ "fdbKeys": n, "torrents": total_torrents }));
    report.insert(
        "issues".into(),
        json!({
            "nullValue": null_value.to_value(),
            "missingName": missing_name.to_value(),
            "missingOriginalname": missing_originalname.to_value(),
            "missingTrackerName": missing_tracker_name.to_value(),
            "emptySearchFields": {
                "emptySn": empty_sn.to_value(),
                "emptySo": empty_so.to_value(),
                "emptyBoth": empty_both.to_value(),
                "total": empty_sn.count + empty_so.count + empty_both.count
            },
            "xxKeys": xx_keys.to_value(),
            "bucketMismatch": bucket_mismatch.to_value(),
            "urlKeyMismatch": url_key_mismatch.to_value(),
            "missingShardFile": missing_shard_file.to_value(),
            "emptyShardListed": empty_shard_listed.to_value(),
            "orphanShardFiles": orphans.to_value(),
            "emptyMagnetOrTypes": empty_magnet_or_types.to_value()
        }),
    );
    Ok(report)
}

/// Fill empty `_sn` / `_so` (and name / originalname). Returns (fixedSn, fixedSo).
pub fn fix_search_fields(t: &mut TorrentDetails) -> (bool, bool) {
    let mut fixed_sn = false;
    let mut fixed_so = false;
    if util::is_blank(&t._sn) {
        if !util::is_blank(&t.name) {
            t._sn = util::search_name_or_empty(&t.name);
            fixed_sn = true;
        } else if !util::is_blank(&t.title) {
            t._sn = util::search_name_or_empty(&t.title);
            fixed_sn = true;
        }
    }
    if util::is_blank(&t._so) {
        if !util::is_blank(&t.originalname) {
            t._so = util::search_name_or_empty(&t.originalname);
            fixed_so = true;
        } else if !util::is_blank(&t.name) {
            t._so = util::search_name_or_empty(&t.name);
            fixed_so = true;
        } else if !util::is_blank(&t.title) {
            t._so = util::search_name_or_empty(&t.title);
            fixed_so = true;
        }
    }
    if util::is_blank(&t.name) {
        t.name = t.title.clone();
    }
    if util::is_blank(&t.originalname) {
        t.originalname = t.name.clone();
    }
    if util::is_blank(&t._sn) && !util::is_blank(&t.name) {
        t._sn = util::search_name_or_empty(&t.name);
        fixed_sn = true;
    }
    if util::is_blank(&t._so) && !util::is_blank(&t.originalname) {
        t._so = util::search_name_or_empty(&t.originalname);
        fixed_so = true;
    }
    (fixed_sn, fixed_so)
}

/// Target bucket when a row belongs elsewhere (`name:originalname` with a non-empty name part).
pub fn migration_target(t: &TorrentDetails, current_key: &str) -> Option<String> {
    let nk = fdb::key_for_torrent(&t.name, &t.originalname);
    if !nk.is_empty() && nk != current_key && nk.find(':').map(|i| i > 0).unwrap_or(false) {
        Some(nk)
    } else {
        None
    }
}

fn apply_safe_fixes(fixed: &mut FixedCounts, ct: &CancellationToken) -> Result<(), Cancelled> {
    for (key, _) in fdb::master_db_snapshot() {
        trackers::check(ct)?;
        let (w, nulls_removed) = nulls::open_write_clean(&key);
        let mut changed = false;
        if nulls_removed > 0 {
            fixed.nullRemoved += nulls_removed as i64;
            changed = true;
        }
        let mut to_migrate: Vec<(TorrentDetails, String)> = Vec::new();
        w.modify(|db| {
            let mut touched = false;
            let mut remove: Vec<String> = Vec::new();
            for (url, t) in db.iter_mut() {
                let (sn, so) = fix_search_fields(t);
                if sn || so {
                    fixed.searchFieldsFixed += 1;
                    touched = true;
                    if let Some(nk) = migration_target(t, &key) {
                        to_migrate.push((t.clone(), nk));
                        remove.push(url.clone());
                    }
                }
            }
            for u in remove {
                db.shift_remove(&u);
            }
            touched
        });
        if !to_migrate.is_empty() {
            changed = true;
        }
        for (t, nk) in to_migrate {
            fdb::migrate_torrent_to_new_key(&t, &nk);
            fixed.migrated += 1;
        }
        if w.is_empty() {
            fdb::remove_key_from_master_db(&key);
            fixed.emptyBucketsRemoved += 1;
            changed = true;
        }
        if changed {
            w.mark_changed();
        }
    }
    Ok(())
}

fn apply_full_fixes(fixed: &mut FixedCounts, ct: &CancellationToken) -> Result<(), Cancelled> {
    for (key, _) in fdb::master_db_snapshot() {
        trackers::check(ct)?;
        if !Path::new(&fdb::path_for_key(&key)).exists() {
            fdb::remove_key_from_master_db(&key);
            fixed.missingShardKeysRemoved += 1;
            continue;
        }
        let (w, nulls_removed) = nulls::open_write_clean(&key);
        let mut changed = nulls_removed > 0;
        let mut to_migrate: Vec<(TorrentDetails, String)> = Vec::new();
        w.modify(|db| {
            let mut remove: Vec<String> = Vec::new();
            let mut rekey: Vec<(String, String, TorrentDetails)> = Vec::new();
            let mut migrate_urls: Vec<String> = Vec::new();
            for (url, t) in db.iter() {
                if util::is_blank(&t.magnet) && t.types.is_empty() {
                    remove.push(url.clone());
                    fixed.incompleteRemoved += 1;
                    continue;
                }
                if let Some(nk) = migration_target(t, &key) {
                    to_migrate.push((t.clone(), nk));
                    migrate_urls.push(url.clone());
                    continue;
                }
                if !t.url.is_empty() && &t.url != url {
                    if db.contains_key(&t.url) {
                        remove.push(url.clone());
                        fixed.urlKeyFixed += 1;
                    } else {
                        rekey.push((url.clone(), t.url.clone(), t.clone()));
                    }
                }
            }
            let touched = !remove.is_empty() || !rekey.is_empty() || !migrate_urls.is_empty();
            for u in remove {
                db.shift_remove(&u);
            }
            for (old, new_url, t) in rekey {
                db.shift_remove(&old);
                if !db.contains_key(&new_url) {
                    db.insert(new_url, t);
                }
                fixed.urlKeyFixed += 1;
            }
            for u in migrate_urls {
                db.shift_remove(&u);
            }
            touched
        });
        if !to_migrate.is_empty() {
            changed = true;
        }
        for (t, nk) in to_migrate {
            fdb::migrate_torrent_to_new_key(&t, &nk);
            fixed.migrated += 1;
        }
        if w.is_empty() {
            fdb::remove_key_from_master_db(&key);
            fixed.emptyBucketsRemoved += 1;
            changed = true;
        }
        if changed {
            w.mark_changed();
        }
    }

    if !Path::new("Data/fdb").is_dir() {
        return Ok(());
    }
    let expected: HashSet<String> = fdb::master_db_snapshot().iter().map(|(k, _)| full_path_lower(&fdb::path_for_key(k))).collect();
    for f in shard_files(ct)? {
        trackers::check(ct)?;
        if expected.contains(&full_path_lower(&f)) {
            continue;
        }
        match std::fs::remove_file(&f) {
            Ok(()) => fixed.orphansDeleted += 1,
            Err(e) => log::warn(cat::FDB, format!("maintenance: failed to delete orphan {f}: {e}")),
        }
    }
    Ok(())
}

fn write_report(report: &Value) {
    let _ = std::fs::create_dir_all("Data/temp");
    let res = serde_json::to_string_pretty(report).map_err(std::io::Error::other).and_then(|s| std::fs::write(REPORT_PATH, s));
    if let Err(e) = res {
        log::warn(cat::FDB, format!("maintenance: failed to write {REPORT_PATH}: {e}"));
    }
}

fn run_inner(
    mode: &str,
    sample_size: i32,
    exclude_numeric_xx: bool,
    ct: &CancellationToken,
    console: bool,
    started: DateTime<Utc>,
) -> Result<Value, Cancelled> {
    let sw = Instant::now();
    log_progress(console, &format!("maintenance: started mode={mode} sampleSize={sample_size} keys={}", fdb::master_db().len()));
    log::info(cat::FDB, format!("maintenance: Check started mode={mode} sampleSize={sample_size}"));

    let mut report = scan(sample_size, exclude_numeric_xx, ct, console)?;
    trackers::check(ct)?;

    let mut fixed = FixedCounts::default();
    if mode == "safe" || mode == "full" {
        set_detail("fix");
        log_progress(console, "maintenance: applying safe fixes…");
        apply_safe_fixes(&mut fixed, ct)?;
    }
    if mode == "full" {
        set_detail("full-fix");
        log_progress(console, "maintenance: applying full fixes…");
        apply_full_fixes(&mut fixed, ct)?;
    }
    if mode == "safe" || mode == "full" {
        set_detail("save");
        log_progress(console, "maintenance: saving masterDb + rebuilding fastdb…");
        fdb::save_changes_to_file();
        let _ = std::panic::catch_unwind(index::rebuild);
    }

    let elapsed = sw.elapsed().as_secs_f64();
    let fixed_v = serde_json::to_value(&fixed).unwrap_or(Value::Null);
    report.insert("ok".into(), Value::Bool(true));
    report.insert("mode".into(), Value::String(mode.to_string()));
    report.insert("running".into(), Value::Bool(false));
    report.insert("startedAt".into(), Value::String(time::format_net(&started)));
    report.insert("finishedAt".into(), Value::String(time::format_net(&Utc::now())));
    report.insert("durationSec".into(), query::num_f64((elapsed * 10.0).round() / 10.0));
    report.insert("fixed".into(), fixed_v.clone());

    let totals = report.get("totals").cloned().unwrap_or(Value::Null);
    let report = Value::Object(report);
    *LAST_REPORT.lock() = Some(report.clone());
    write_report(&report);

    let summary = format!(
        "maintenance: finished mode={mode} duration={elapsed:.1}s keys={{ fdbKeys = {}, torrents = {} }} fixed={}",
        totals.get("fdbKeys").cloned().unwrap_or(Value::Null),
        totals.get("torrents").cloned().unwrap_or(Value::Null),
        serde_json::to_string(&fixed_v).unwrap_or_default()
    );
    log_progress(console, &summary);
    log_progress(console, &format!("maintenance: report written to {REPORT_PATH}"));
    log::info(cat::FDB, summary);
    Ok(report)
}

/// Run the integrity job synchronously. `Ok(true)` when the report finished with ok=true;
/// `Err(Cancelled)` when `ct` fired.
pub fn run(mode: &str, sample_size: i32, exclude_numeric_xx: bool, ct: &CancellationToken, console: bool) -> Result<bool, Cancelled> {
    let mode = normalize_mode(mode);
    let sample_size = clamp_sample_size(sample_size);
    let started = Utc::now();
    {
        let mut st = STATE.lock();
        st.running = true;
        st.mode = Some(mode.clone());
        st.started_at = Some(started);
        st.detail = Some("scan".into());
    }
    PROGRESS_CURRENT.store(0, Ordering::SeqCst);
    PROGRESS_TOTAL.store(fdb::master_db().len() as i64, Ordering::SeqCst);

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_inner(&mode, sample_size, exclude_numeric_xx, ct, console, started)));
    let out = match res {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(Cancelled)) => {
            log_progress(console, "maintenance: cancelled");
            log::warn(cat::FDB, "maintenance: Check cancelled");
            Err(Cancelled)
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            log_progress(console, &format!("maintenance: error: {msg}"));
            log::error(cat::FDB, format!("maintenance: Check error: {msg}"));
            let r = json!({
                "ok": false,
                "mode": mode,
                "error": msg,
                "startedAt": time::format_net(&started),
                "finishedAt": time::format_net(&Utc::now()),
            });
            *LAST_REPORT.lock() = Some(r.clone());
            write_report(&r);
            Ok(false)
        }
    };
    {
        let mut st = STATE.lock();
        st.running = false;
        st.detail = None;
        st.mode = None;
        st.started_at = None;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_normalize() {
        assert_eq!(normalize_mode(""), "report");
        assert_eq!(normalize_mode(" SAFE "), "safe");
        assert_eq!(normalize_mode("full"), "full");
        assert_eq!(normalize_mode("nope"), "report");
    }

    #[test]
    fn sample_size_clamped() {
        assert_eq!(clamp_sample_size(0), 20);
        assert_eq!(clamp_sample_size(500), 200);
        assert_eq!(clamp_sample_size(7), 7);
    }

    #[test]
    fn xx_keys() {
        assert!(is_xx_key("ponies:ponies", true));
        assert!(is_xx_key("Ponies:ponies", true));
        assert!(!is_xx_key("1899:1899", true));
        assert!(is_xx_key("1899:1899", false));
        assert!(!is_xx_key("a:b", true));
        assert!(!is_xx_key(":a", true));
        assert!(!is_xx_key("a:", true));
        assert!(!is_xx_key("nocolon", true));
    }

    #[test]
    fn issue_bucket_samples() {
        let mut b = IssueBucket::new(2);
        for i in 0..5 {
            b.add(json!(i));
        }
        assert_eq!(b.to_value(), json!({"count": 5, "sample": [0, 1]}));
    }

    #[test]
    fn search_fields_fill() {
        let mut t = TorrentDetails { title: "Title X".into(), ..Default::default() };
        assert_eq!(fix_search_fields(&mut t), (true, true));
        assert_eq!(t.name, "Title X");
        assert_eq!(t.originalname, "Title X");
        assert_eq!(t._sn, "titlex");
        assert_eq!(t._so, "titlex");

        let mut t = TorrentDetails { name: "Имя".into(), originalname: "Name".into(), _sn: "имя".into(), ..Default::default() };
        assert_eq!(fix_search_fields(&mut t), (false, true));
        assert_eq!(t._so, "name");
    }

    #[test]
    fn fixed_counts_shape() {
        let v = serde_json::to_string(&FixedCounts::default()).unwrap();
        assert_eq!(
            v,
            r#"{"nullRemoved":0,"searchFieldsFixed":0,"migrated":0,"emptyBucketsRemoved":0,"missingShardKeysRemoved":0,"orphansDeleted":0,"urlKeyFixed":0,"incompleteRemoved":0}"#
        );
    }
}
