// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Read-only data-quality report over the whole FileDB: what the fix migrations in this
//! module would touch. Meant for the weekly cron (`/dev/checkdata`) and the admin panel
//! (**Maintenance → Data check**); nothing is changed here.
//!
//! Per tracker: rows, rows with a readable `sizeName` but `size` 0 (`FixZeroSizes`), rows whose
//! url host differs from the configured tracker host (domain migrations), torrent ids with
//! several rows (`FixSlugDuplicates`), rutracker rows whose `originalname` equals `name` although
//! the title has a `/`-separated original (`FixRutrackerNames`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use crab_core::log::{self, cat};
use crab_core::{conf, fdb, util};
use serde_json::{json, Value};

static RUNNING: AtomicBool = AtomicBool::new(false);

use super::migrations::parsers::host_of;

pub const REPORT_PATH: &str = "Data/temp/datacheck.json";

/// Trackers whose numeric id names one torrent page (same list as `slug_dups`).
const ID_TRACKERS: [&str; 14] = [
    "rutor", "megapeer", "torrentby", "selezen", "nnmclub", "anibelka", "korsars", "anistar", "leproduction", "viruseproject", "baibako",
    "rudub", "kinozal", "rutracker",
];

#[derive(Default, Clone)]
struct Counts {
    rows: i64,
    zero_size: i64,
    foreign_host: i64,
    dup_ids: i64,
    bad_names: i64,
}

impl Counts {
    fn issues(&self) -> i64 {
        self.zero_size + self.foreign_host + self.dup_ids + self.bad_names
    }
}

/// Aggregators whose rows legitimately point at other sites (`host` is only their API).
const NO_HOST_CHECK: [&str; 1] = ["knaben"];

/// Configured host (lowercase) of every tracker, for the foreign-host check.
fn configured_hosts() -> HashMap<String, String> {
    let c = conf();
    crab_core::config::TRACKER_SLUGS
        .iter()
        .filter(|slug| !NO_HOST_CHECK.contains(slug))
        .filter_map(|slug| c.tracker(slug).and_then(|t| host_of(&t.host)).map(|h| (slug.to_string(), h)))
        .collect()
}

/// rutracker title `Name / Original (…) […]` whose original was not extracted.
pub fn rutracker_name_suspect(title: &str, name: &str, originalname: &str) -> bool {
    if util::is_blank(name) || name != originalname {
        return false;
    }
    let head = title.split(['(', '[']).next().unwrap_or("");
    head.contains(" / ")
}

/// Slice of a report kept as `previous` in the next one, so the panel shows before / after.
fn summary_of(r: &Value) -> Value {
    json!({ "at": r["at"], "total": r["total"], "trackers": r["trackers"], "trigger": r["trigger"] })
}

/// Run the report in the background (after a fix migration) unless one is running.
pub fn spawn_after_fix(migration: &'static str) {
    if RUNNING.load(Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let _ = tokio::task::spawn_blocking(move || run_with_trigger(migration)).await;
    });
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub fn run() -> Value {
    run_with_trigger("manual")
}

/// `trigger`: `manual`, `cron` or the name of the migration that just ran.
pub fn run_with_trigger(trigger: &str) -> Value {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return json!({ "ok": false, "error": "check already running" });
    }
    let previous = last_report().map(|r| summary_of(&r));
    let mut report = run_inner();
    report["trigger"] = json!(trigger);
    if let Some(p) = previous {
        report["previous"] = p;
    }
    let _ = std::fs::create_dir_all("Data/temp");
    if let Ok(s) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(REPORT_PATH, s);
    }
    log::info(cat::FDB, format!("data check ({trigger}): issues {} in {}s", report["total"]["issues"], report["tookSec"]));
    RUNNING.store(false, Ordering::SeqCst);
    report
}

fn run_inner() -> Value {
    let sw = Instant::now();
    let hosts = configured_hosts();
    let mut per: HashMap<String, Counts> = HashMap::new();
    let mut ids: HashMap<(String, i32), u32> = HashMap::new();
    let mut buckets = 0i64;

    for (key, _) in fdb::master_db_snapshot() {
        buckets += 1;
        for (url, t) in fdb::open_read(&key, false, false) {
            let tracker = t.trackerName.to_lowercase();
            if tracker.is_empty() {
                continue;
            }
            let c = per.entry(tracker.clone()).or_default();
            c.rows += 1;
            if t.size <= 0.0 && !util::is_blank(&t.sizeName) && fdb::size_from_name(&t.sizeName) > 0 {
                c.zero_size += 1;
            }
            if let (Some(want), Some(have)) = (hosts.get(&tracker), host_of(&url)) {
                if &have != want {
                    c.foreign_host += 1;
                }
            }
            if ID_TRACKERS.contains(&tracker.as_str()) {
                let id = fdb::torrent_id_from_url(&tracker, &url);
                if id > 0 {
                    *ids.entry((tracker.clone(), id)).or_default() += 1;
                }
            }
            if tracker == "rutracker" && rutracker_name_suspect(&t.title, &t.name, &t.originalname) {
                c.bad_names += 1;
            }
        }
    }
    for ((tracker, _), n) in ids {
        if n > 1 {
            per.entry(tracker).or_default().dup_ids += (n - 1) as i64;
        }
    }

    let mut trackers: Vec<(String, Counts)> = per.into_iter().collect();
    trackers.sort_by(|a, b| b.1.issues().cmp(&a.1.issues()).then_with(|| a.0.cmp(&b.0)));
    let total = trackers.iter().fold(Counts::default(), |mut acc, (_, c)| {
        acc.rows += c.rows;
        acc.zero_size += c.zero_size;
        acc.foreign_host += c.foreign_host;
        acc.dup_ids += c.dup_ids;
        acc.bad_names += c.bad_names;
        acc
    });
    let row = |c: &Counts| json!({ "rows": c.rows, "zeroSize": c.zero_size, "foreignHost": c.foreign_host, "dupIds": c.dup_ids, "badNames": c.bad_names, "issues": c.issues() });
    let report = json!({
        "ok": true,
        "at": Utc::now(),
        "tookSec": sw.elapsed().as_secs(),
        "buckets": buckets,
        "total": row(&total),
        "trackers": trackers.iter().map(|(t, c)| { let mut v = row(c); v["tracker"] = json!(t); v }).collect::<Vec<_>>(),
        // which migration heals which column
        "fixes": {
            "zeroSize": "dev/fixzerosizes",
            "dupIds": "dev/fixslugduplicates",
            "badNames": "dev/fixrutrackernames",
            "foreignHost": { "kinozal": "dev/fixkinozaldomainduplicates", "rutracker": "dev/fixrutrackerdomainduplicates", "selezen": "dev/fixselezendomainduplicates", "ultradox": "dev/fixultradoxdomainduplicates" },
        },
    });
    report
}

pub fn last_report() -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(REPORT_PATH).ok()?).ok()
}

pub fn status() -> Value {
    json!({ "ok": true, "running": is_running(), "last": last_report() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rutracker_name_suspects() {
        assert!(rutracker_name_suspect("Матрица / The Matrix (Братья Вачовски) [1999, США]", "Матрица", "Матрица"));
        assert!(!rutracker_name_suspect("Матрица / The Matrix (…) [1999]", "Матрица", "The Matrix"));
        assert!(!rutracker_name_suspect("Дневной дозор (Тимур Бекмамбетов) [2006]", "Дневной дозор", "Дневной дозор"));
        assert!(!rutracker_name_suspect("Что-то (Автор / Author) [2020]", "Что-то", "Что-то"));
    }
}
