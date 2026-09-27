//! One torrent id, several rows. Trackers whose topic URL carries the numeric id plus a slug
//! (rutor `/torrent/{id}/{slug}`, selezen `/relizy-ot-selezen/{id}-{slug}.html`, …) rename the
//! slug whenever the title changes, so rows imported before id matching existed kept every old
//! spelling: `.../405179/druzja-1938-satrip`, `.../405179/druzja-1939-satrip`, … The parser now
//! updates such a row in place, but at most one old copy per pass - the rest never go away.
//!
//! Per bucket, group rows of those trackers by `(tracker, id)`, keep the most recently updated
//! row (newest createTime on ties), take its magnet from a loser when blank, drop the losers and
//! bump the bucket so sync clients receive it and prune the same rows.

use std::collections::HashMap;

use crab_core::models::TorrentDetails;
use crab_core::{fdb, time, util};
use serde_json::{json, Value};

use super::try_rebuild_fast_db;

/// Trackers whose id names one torrent page. lostfilm/subsplease/ultradox ids or fragments can
/// legitimately repeat across rows, so they are left alone.
const TRACKERS: [&str; 14] = [
    "rutor", "megapeer", "torrentby", "selezen", "nnmclub", "anibelka", "korsars", "anistar", "leproduction", "viruseproject", "baibako",
    "rudub", "kinozal", "rutracker",
];

fn rank(t: &TorrentDetails) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    (t.updateTime, t.createTime)
}

/// Rows to drop from one bucket, with the keeper to patch (`url → keeper row`) when a loser had
/// the only magnet. Returns `(losers, keepers to rewrite)`.
pub fn plan(db: &fdb::ShardMap) -> (Vec<String>, Vec<TorrentDetails>) {
    let mut groups: HashMap<(String, i32), Vec<String>> = HashMap::new();
    for (url, t) in db.iter() {
        let tracker = t.trackerName.to_lowercase();
        if !TRACKERS.contains(&tracker.as_str()) {
            continue;
        }
        let id = fdb::torrent_id_from_url(&tracker, url);
        if id <= 0 {
            continue;
        }
        groups.entry((tracker, id)).or_default().push(url.clone());
    }
    let mut losers = Vec::new();
    let mut patched = Vec::new();
    for urls in groups.into_values().filter(|u| u.len() > 1) {
        let keep_url = urls.iter().max_by_key(|u| (rank(&db[*u]), std::cmp::Reverse((*u).clone()))).expect("non-empty group").clone();
        let mut keep = db[&keep_url].clone();
        let mut changed = false;
        for u in urls {
            if u == keep_url {
                continue;
            }
            let other = &db[&u];
            if util::is_blank(&keep.magnet) && !util::is_blank(&other.magnet) {
                keep.magnet = other.magnet.clone();
                changed = true;
            }
            losers.push(u);
        }
        if changed {
            patched.push(keep);
        }
    }
    losers.sort();
    (losers, patched)
}

pub fn fix_slug_duplicates() -> Value {
    let (mut buckets, mut removed, mut patched_n) = (0i64, 0i64, 0i64);
    let mut per_tracker: HashMap<String, i64> = HashMap::new();
    for (key, _) in fdb::master_db_snapshot() {
        let w = fdb::open_write(&key);
        let mut touched = false;
        w.modify(|db| {
            let (losers, patched) = plan(db);
            if losers.is_empty() {
                return false;
            }
            for u in &losers {
                if let Some(t) = db.shift_remove(u) {
                    *per_tracker.entry(t.trackerName.to_lowercase()).or_default() += 1;
                }
            }
            removed += losers.len() as i64;
            for t in patched {
                patched_n += 1;
                db.insert(t.url.clone(), t);
            }
            touched = true;
            true
        });
        if touched {
            buckets += 1;
            // Re-send the bucket: clients drop the same rows through the sync deletion rule.
            fdb::set_shard(&key, time::now());
        }
    }
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({ "ok": true, "buckets": buckets, "removed": removed, "magnetTaken": patched_n, "trackers": per_tracker })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn row(tracker: &str, url: &str, magnet: &str, age_h: i64) -> TorrentDetails {
        let mut t = TorrentDetails { trackerName: tracker.into(), url: url.into(), magnet: magnet.into(), ..Default::default() };
        t.updateTime = Utc::now() - Duration::hours(age_h);
        t
    }

    #[test]
    fn keeps_newest_row_per_id_and_takes_missing_magnet() {
        let mut db = fdb::ShardMap::new();
        for (u, m, age) in [
            ("http://rutor.info/torrent/405179/druzja-1939-satrip", "m1", 48),
            ("http://rutor.info/torrent/405179/druzja-1938-satrip", "", 1),
            ("http://rutor.info/torrent/405179/druzja-1939-dvdrip", "m2", 24),
            ("http://rutor.info/torrent/405258/other", "m3", 1),
        ] {
            db.insert(u.into(), row("rutor", u, m, age));
        }
        // ultradox fragments are never grouped
        db.insert("https://ultradox.vip/x#h=a".into(), row("ultradox", "https://ultradox.vip/x#h=a", "u1", 1));
        db.insert("https://ultradox.vip/x#h=b".into(), row("ultradox", "https://ultradox.vip/x#h=b", "u2", 1));

        let (losers, patched) = plan(&db);
        assert_eq!(losers, vec!["http://rutor.info/torrent/405179/druzja-1939-dvdrip".to_string(), "http://rutor.info/torrent/405179/druzja-1939-satrip".to_string()]);
        assert_eq!(patched.len(), 1);
        assert_eq!(patched[0].url, "http://rutor.info/torrent/405179/druzja-1938-satrip");
        assert!(!patched[0].magnet.is_empty());
    }
}
