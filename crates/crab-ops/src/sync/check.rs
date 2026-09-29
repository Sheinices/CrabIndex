// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Integrity check of the local copy against `syncapi`.
//!
//! The incremental feed only carries buckets that changed after the client's cursor, so a
//! bucket the client missed (a page lost to a timeout, a rename delivered slim, a deletion made
//! before deletions were propagated) stays wrong until that bucket changes again. Once per
//! `timeSyncCheck` minutes the client downloads the host's bucket digest (`/sync/fdb/digest`,
//! `[key, fileTime]` per bucket), compares it with its own masterDb and repairs the difference:
//!
//! * buckets the host has and the client does not, and buckets whose `fileTime` differs, are
//!   fetched in full (`/sync/fdb?key=`) and imported; rows of served trackers missing from the
//!   fetched bucket are dropped;
//! * buckets only the client has are deleted when every row belongs to a tracker the host
//!   serves and this client accepts (rows of trackers parsed locally stay).
//!
//! At most [`MAX_FETCH`] / [`MAX_DELETE`] buckets per run; the rest waits for the next run.
//! The report is written to `Data/temp/sync_check.json` and shown in the admin overview.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Instant;

use chrono::Utc;
use crab_core::config::AppOptions;
use crab_core::fdb;
use crab_core::log::{self, cat};
use crab_core::trackers::WorkFlag;
use crab_core::{conf, net, util};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::cron;

pub const REPORT_PATH: &str = "Data/temp/sync_check.json";
pub const MAX_FETCH: usize = 20_000;
pub const MAX_DELETE: usize = 20_000;
/// First run this long after start (the regular sync gets a head start).
const FIRST_RUN_MINUTES: i64 = 60;

static RUNNING: WorkFlag = WorkFlag::new();
static WANT_NOW: AtomicBool = AtomicBool::new(false);
/// Unix seconds of the last run start (0 = never in this process).
static LAST_RUN: AtomicI64 = AtomicI64::new(0);

/// Where the running check is (`/cron/sync/checkstatus` → `progress`), `None` when idle.
static PROGRESS: parking_lot::Mutex<Option<Progress>> = parking_lot::Mutex::new(None);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub started_at: chrono::DateTime<Utc>,
    /// `conf`, `digest`, `compare`, `fetch`, `delete`, `save`.
    pub phase: &'static str,
    /// Items done / planned in the current phase (`fetch`, `delete`); zero elsewhere.
    pub done: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mismatched: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<usize>,
}

fn set_phase(phase: &'static str, total: usize) {
    if let Some(p) = PROGRESS.lock().as_mut() {
        p.phase = phase;
        p.done = 0;
        p.total = total;
    }
}

/// Live progress with `done` taken from the refetch counter while fetching.
pub fn progress() -> Option<Progress> {
    let mut p = PROGRESS.lock().clone()?;
    if p.phase == "fetch" {
        p.done = cron::REFETCH_DONE.load(Ordering::Relaxed).min(p.total);
    }
    Some(p)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DigestIn {
    count: i64,
    buckets: Vec<(String, i64)>,
}

/// Difference between the host digest and the local masterDb.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
    pub extra: Vec<String>,
}

pub fn plan(master: &[(String, i64)], local: &HashMap<String, i64>) -> Plan {
    let mut p = Plan::default();
    let mut seen: HashSet<&str> = HashSet::with_capacity(master.len());
    for (key, ft) in master {
        if key.is_empty() {
            continue;
        }
        seen.insert(key.as_str());
        match local.get(key) {
            None => p.missing.push(key.clone()),
            Some(lft) if lft != ft => p.mismatched.push(key.clone()),
            Some(_) => {}
        }
    }
    p.extra = local.keys().filter(|k| !seen.contains(k.as_str())).cloned().collect();
    p.missing.sort();
    p.mismatched.sort();
    p.extra.sort();
    p
}

/// A locally parsed tracker never loses rows to the host; only trackers this client takes
/// from the host and the host serves are authoritative there.
fn host_owns_tracker(tracker: &str, served: &HashSet<String>, c: &AppOptions) -> bool {
    let t = tracker.to_lowercase();
    served.contains(&t)
        && c.synctrackers.as_ref().map(|st| st.iter().any(|x| x.eq_ignore_ascii_case(&t))).unwrap_or(true)
        && !c.disable_trackers.iter().any(|d| d.eq_ignore_ascii_case(&t))
}

pub fn is_running() -> bool {
    RUNNING.is_busy()
}

pub fn last_report() -> Option<Value> {
    let s = std::fs::read_to_string(REPORT_PATH).ok()?;
    serde_json::from_str(&s).ok()
}

/// Ask the worker to run a check now. `ok`, `work` (already running) or `disabled` (no syncapi).
pub fn start_now() -> &'static str {
    if util::is_blank(conf().syncapi.as_deref().unwrap_or("")) {
        return "disabled";
    }
    if RUNNING.is_busy() {
        return "work";
    }
    WANT_NOW.store(true, Ordering::SeqCst);
    "ok"
}

fn due(c: &AppOptions, started: Instant) -> bool {
    if c.timeSyncCheck <= 0 {
        return false;
    }
    let last = LAST_RUN.load(Ordering::Relaxed);
    if last == 0 {
        return started.elapsed().as_secs() as i64 >= FIRST_RUN_MINUTES * 60;
    }
    Utc::now().timestamp() - last >= c.timeSyncCheck as i64 * 60
}

pub async fn run_worker(ct: CancellationToken) {
    let started = Instant::now();
    while !ct.is_cancelled() {
        if !crate::sleep_ct(std::time::Duration::from_secs(30), &ct).await {
            return;
        }
        let c = conf();
        let syncapi = c.syncapi.clone().unwrap_or_default();
        if util::is_blank(&syncapi) {
            continue;
        }
        if !(WANT_NOW.swap(false, Ordering::SeqCst) || due(&c, started)) {
            continue;
        }
        if !RUNNING.try_start() {
            continue;
        }
        LAST_RUN.store(Utc::now().timestamp(), Ordering::Relaxed);
        let report = run_check(&c, &syncapi, &ct).await;
        *PROGRESS.lock() = None;
        let _ = std::fs::create_dir_all("Data/temp");
        if let Ok(s) = serde_json::to_string_pretty(&report) {
            let _ = std::fs::write(REPORT_PATH, s);
        }
        RUNNING.end();
    }
}

async fn run_check(c: &AppOptions, syncapi: &str, ct: &CancellationToken) -> Value {
    let sw = Instant::now();
    let at = Utc::now();
    *PROGRESS.lock() = Some(Progress { started_at: at, phase: "conf", done: 0, total: 0, missing: None, mismatched: None, extra: None });
    log::info(cat::SYNC, "check: start");
    let fail = |error: &str| {
        log::warn(cat::SYNC, format!("check: {error}"));
        json!({ "at": at, "ok": false, "error": error, "tookSec": sw.elapsed().as_secs() })
    };

    let conf_json = cron::remote_conf(syncapi).await;
    let Some(served) = cron::served_from_conf(conf_json.as_ref()) else {
        return fail("host does not report served trackers (/sync/conf without `trackers`) - upgrade syncapi host");
    };
    set_phase("digest", 0);
    let req = cron::sync_req(900, 500_000_000).cancel(ct);
    let Some(digest) = net::get_json::<DigestIn>(&format!("{syncapi}/sync/fdb/digest"), &req).await else {
        return fail("digest request failed (/sync/fdb/digest)");
    };
    if digest.buckets.is_empty() {
        return fail("empty digest (opensync off on the host?)");
    }

    set_phase("compare", 0);
    let local: HashMap<String, i64> = fdb::master_db_snapshot().into_iter().map(|(k, s)| (k, s.fileTime)).collect();
    let p = plan(&digest.buckets, &local);
    if let Some(pr) = PROGRESS.lock().as_mut() {
        pr.missing = Some(p.missing.len());
        pr.mismatched = Some(p.mismatched.len());
        pr.extra = Some(p.extra.len());
    }
    log::info(
        cat::SYNC,
        format!("check: host {} buckets, local {}, missing {}, mismatched {}, extra {}", digest.count, local.len(), p.missing.len(), p.mismatched.len(), p.extra.len()),
    );

    let to_fetch: Vec<String> = p.missing.iter().chain(p.mismatched.iter()).take(MAX_FETCH).cloned().collect();
    let planned_fetch = to_fetch.len();
    let host_ft: HashMap<&str, i64> = digest.buckets.iter().map(|(k, ft)| (k.as_str(), *ft)).collect();
    let stamps: Vec<(String, i64)> = to_fetch.iter().filter_map(|k| host_ft.get(k.as_str()).map(|ft| (k.clone(), *ft))).collect();
    cron::REFETCH_DONE.store(0, Ordering::Relaxed);
    set_phase("fetch", planned_fetch);
    let (fetched, rows, pruned) = cron::refetch_buckets(syncapi, to_fetch, c, ct, Some(&served)).await;
    // Refetched buckets now equal the host's: carry its stamp so the next digest matches.
    tokio::task::spawn_blocking(move || cron::mirror_bucket_stamps(stamps)).await.ok();

    let mut deleted = 0usize;
    let mut kept_local = 0usize;
    let mut considered = 0usize;
    set_phase("delete", p.extra.len().min(MAX_DELETE));
    for key in p.extra.iter().take(MAX_DELETE) {
        if ct.is_cancelled() {
            break;
        }
        considered += 1;
        if let Some(pr) = PROGRESS.lock().as_mut() {
            pr.done = considered;
        }
        let key = key.clone();
        let served2 = served.clone();
        let c2 = c.clone();
        let removed = tokio::task::spawn_blocking(move || {
            let rows = fdb::open_read(&key, false, false);
            if rows.is_empty() || !rows.values().all(|t| host_owns_tracker(&t.trackerName, &served2, &c2)) {
                return None;
            }
            Some(fdb::retain_rows(&key, |_| false))
        })
        .await
        .ok()
        .flatten();
        match removed {
            Some(_) => deleted += 1,
            None => kept_local += 1,
        }
    }
    set_phase("save", 0);
    tokio::task::spawn_blocking(fdb::save_changes_to_file).await.ok();

    let remaining = (p.missing.len() + p.mismatched.len()).saturating_sub(planned_fetch) + p.extra.len().saturating_sub(considered);
    let took = sw.elapsed().as_secs();
    log::info(
        cat::SYNC,
        format!("check: fetched {fetched} buckets ({rows} rows, pruned {pruned}), deleted {deleted} extra buckets, kept {kept_local} local-only, remaining {remaining}, {took}s"),
    );
    json!({
        "at": at,
        "ok": true,
        "tookSec": took,
        "hostBuckets": digest.count,
        "localBuckets": local.len(),
        "missing": p.missing.len(),
        "mismatched": p.mismatched.len(),
        "extra": p.extra.len(),
        "fetchedBuckets": fetched,
        "importedRows": rows,
        "prunedRows": pruned,
        "deletedBuckets": deleted,
        "keptLocalBuckets": kept_local,
        "remaining": remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_splits_missing_mismatched_and_extra() {
        let master = vec![("a:a".to_string(), 10), ("b:b".to_string(), 20), ("c:c".to_string(), 30), (String::new(), 1)];
        let local: HashMap<String, i64> = [("a:a".to_string(), 10), ("b:b".to_string(), 21), ("d:d".to_string(), 5)].into_iter().collect();
        let p = plan(&master, &local);
        assert_eq!(p, Plan { missing: vec!["c:c".into()], mismatched: vec!["b:b".into()], extra: vec!["d:d".into()] });
    }

    #[test]
    fn host_owned_trackers_respect_client_filters() {
        let served: HashSet<String> = ["rutor".to_string()].into_iter().collect();
        let mut c = AppOptions::default();
        assert!(host_owns_tracker("Rutor", &served, &c));
        assert!(!host_owns_tracker("lostfilm", &served, &c));
        c.synctrackers = Some(vec!["kinozal".into()]);
        assert!(!host_owns_tracker("rutor", &served, &c));
        c.synctrackers = None;
        c.disable_trackers = vec!["rutor".into()];
        assert!(!host_owns_tracker("rutor", &served, &c));
    }
}
