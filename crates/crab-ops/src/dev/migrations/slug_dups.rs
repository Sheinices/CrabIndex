// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! One torrent id, several rows. Trackers whose topic URL carries the numeric id plus a slug
//! (rutor `/torrent/{id}/{slug}`, selezen `/relizy-ot-selezen/{id}-{slug}.html`, …) rename the
//! slug whenever the title changes, so rows imported before id matching existed kept every old
//! spelling: `.../405179/druzja-1938-satrip`, `.../405179/druzja-1939-satrip`, … The parser now
//! updates such a row in place, but at most one old copy per pass - the rest never go away.
//! The copies usually sit in different buckets (an old copy's `originalname` is the whole old
//! title), so the scan is global, in two passes: count `(tracker, id)` over the base, then
//! collect only the repeated ones.
//!
//! Per `(tracker, id)` keep the most recently updated row (newest createTime, then the
//! alphabetically last url on ties), take its magnet from a loser when blank, drop the losers and
//! bump every touched bucket so sync clients receive it and prune the same rows.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use crab_core::{fdb, time, util};
use serde_json::{json, Value};

use super::try_rebuild_fast_db;

/// Trackers whose id names one torrent page. lostfilm/subsplease/ultradox ids or fragments can
/// legitimately repeat across rows, so they are left alone.
const TRACKERS: [&str; 14] = [
    "rutor", "megapeer", "torrentby", "selezen", "nnmclub", "anibelka", "korsars", "anistar", "leproduction", "viruseproject", "baibako",
    "rudub", "kinozal", "rutracker",
];

type GroupKey = (String, i32);

/// One candidate row: bucket, url, ranking fields and whether it has a magnet.
#[derive(Clone, Debug, PartialEq)]
pub struct Cand {
    pub key: String,
    pub url: String,
    pub update_time: DateTime<Utc>,
    pub create_time: DateTime<Utc>,
    pub magnet: String,
}

fn group_key(tracker: &str, url: &str) -> Option<GroupKey> {
    let tracker = tracker.to_lowercase();
    if !TRACKERS.contains(&tracker.as_str()) {
        return None;
    }
    let id = fdb::torrent_id_from_url(&tracker, url);
    (id > 0).then_some((tracker, id))
}

/// Decision for one group: `(keeper, losers, magnet for the keeper when it had none)`.
pub fn choose(mut rows: Vec<Cand>) -> (Cand, Vec<Cand>, Option<String>) {
    rows.sort_by(|a, b| (a.update_time, a.create_time, &a.url).cmp(&(b.update_time, b.create_time, &b.url)));
    let keep = rows.pop().expect("non-empty group");
    let magnet = if util::is_blank(&keep.magnet) { rows.iter().rev().find(|r| !util::is_blank(&r.magnet)).map(|r| r.magnet.clone()) } else { None };
    (keep, rows, magnet)
}

pub fn fix_slug_duplicates() -> Value {
    let keys: Vec<String> = fdb::master_db_snapshot().into_iter().map(|(k, _)| k).collect();

    // pass 1: how many rows share each (tracker, id)
    let mut counts: HashMap<GroupKey, u32> = HashMap::new();
    for key in &keys {
        for (url, t) in fdb::open_read(key, false, false) {
            if let Some(gk) = group_key(&t.trackerName, &url) {
                *counts.entry(gk).or_default() += 1;
            }
        }
    }
    let repeated: HashSet<GroupKey> = counts.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect();
    if repeated.is_empty() {
        return json!({ "ok": true, "groups": 0, "removed": 0, "magnetTaken": 0, "buckets": 0, "trackers": {} });
    }

    // pass 2: the repeated rows only
    let mut groups: HashMap<GroupKey, Vec<Cand>> = HashMap::new();
    for key in &keys {
        for (url, t) in fdb::open_read(key, false, false) {
            let Some(gk) = group_key(&t.trackerName, &url) else { continue };
            if !repeated.contains(&gk) {
                continue;
            }
            groups.entry(gk).or_default().push(Cand { key: key.clone(), url, update_time: t.updateTime, create_time: t.createTime, magnet: t.magnet });
        }
    }

    // decide: losers per bucket, magnet patches per bucket
    let mut losers_by_key: HashMap<String, Vec<String>> = HashMap::new();
    let mut patches_by_key: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut per_tracker: HashMap<String, i64> = HashMap::new();
    let (mut removed, mut magnet_taken, mut group_count) = (0i64, 0i64, 0i64);
    for ((tracker, _), rows) in groups {
        if rows.len() < 2 {
            continue;
        }
        group_count += 1;
        let (keep, losers, magnet) = choose(rows);
        if let Some(m) = magnet {
            patches_by_key.entry(keep.key.clone()).or_default().push((keep.url.clone(), m));
            magnet_taken += 1;
        }
        for l in losers {
            *per_tracker.entry(tracker.clone()).or_default() += 1;
            removed += 1;
            losers_by_key.entry(l.key).or_default().push(l.url);
        }
    }

    // apply
    let mut touched: HashSet<String> = HashSet::new();
    for (key, urls) in &losers_by_key {
        let drop: HashSet<&String> = urls.iter().collect();
        let n = fdb::retain_rows(key, |t| !drop.contains(&t.url));
        if n > 0 {
            touched.insert(key.clone());
        }
    }
    for (key, patches) in &patches_by_key {
        let w = fdb::open_write(key);
        let changed = w.modify(|db| {
            let mut changed = false;
            for (url, magnet) in patches {
                if let Some(t) = db.get_mut(url) {
                    t.magnet = magnet.clone();
                    changed = true;
                }
            }
            changed
        });
        if changed {
            touched.insert(key.clone());
        }
    }
    for key in &touched {
        if fdb::MASTER_DB.contains_key(key) {
            // Re-send the bucket: clients drop the same rows through the sync deletion rule.
            fdb::set_shard(key, time::now());
        }
    }
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({ "ok": true, "groups": group_count, "removed": removed, "magnetTaken": magnet_taken, "buckets": touched.len(), "trackers": per_tracker })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn cand(key: &str, url: &str, magnet: &str, age_h: i64) -> Cand {
        let now = Utc::now();
        Cand { key: key.into(), url: url.into(), update_time: now - Duration::hours(age_h), create_time: now, magnet: magnet.into() }
    }

    #[test]
    fn group_key_only_for_id_trackers() {
        assert_eq!(group_key("rutor", "http://rutor.info/torrent/405179/druzja-1939-satrip"), Some(("rutor".into(), 405179)));
        assert_eq!(group_key("Selezen", "https://open.selezen.org/relizy-ot-selezen/254-zveropolis.html"), Some(("selezen".into(), 254)));
        assert_eq!(group_key("ultradox", "https://ultradox.vip/x#h=a"), None);
        assert_eq!(group_key("rutor", "http://rutor.info/browse/0"), None);
    }

    #[test]
    fn keeps_newest_row_and_takes_missing_magnet_across_buckets() {
        let rows = vec![
            cand("друзья:друзья1939satrip", "http://rutor.info/torrent/405179/druzja-1939-satrip", "m1", 48),
            cand("друзья:друзья", "http://rutor.info/torrent/405179/druzja-1938-satrip", "", 1),
            cand("друзья:друзья1939dvdrip", "http://rutor.info/torrent/405179/druzja-1939-dvdrip", "m2", 24),
        ];
        let (keep, losers, magnet) = choose(rows);
        assert_eq!(keep.url, "http://rutor.info/torrent/405179/druzja-1938-satrip");
        assert_eq!(losers.len(), 2);
        assert!(losers.iter().all(|l| l.key != "друзья:друзья"));
        assert_eq!(magnet.as_deref(), Some("m2"));

        let mut a = cand("k", "a", "y", 1);
        let mut b = cand("k", "b", "x", 1);
        b.update_time = a.update_time;
        b.create_time = a.create_time;
        a.magnet = "y".into();
        let (keep, _, magnet) = choose(vec![b, a]);
        assert_eq!((keep.url.as_str(), magnet), ("b", None));
    }
}
