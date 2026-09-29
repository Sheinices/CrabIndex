// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Collapse duplicates left by tracker domain changes and rewrite lone rows to the
//! canonical host from config.
//!
//! * kinozal: grouped by `details.php?id=` (`.tv` → `.guru`); `userdetails` rows are dropped.
//! * rutracker: grouped by `viewtopic.php?t=` (`.net` and mirrors → `.org`).
//! * selezen: grouped by the release id in `/relizy-ot-selezen/{id}-…` (`selezen.org`,
//!   `use.selezen.club` → `Selezen.host`); the kept row's path is preserved.
//! * ultradox: grouped by host-independent path + `#h=` fragment (`.onl` → `.vip`, `00N.` mirrors),
//!   so different qualities on one page stay separate rows.

use crab_core::models::TorrentDetails;
use crab_core::{conf, fdb, rx, time, util};
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::collections::HashSet;

use super::parsers::{host_of, ultradox};

/// Merge a loser into the kept row: best sid/pir, newest updateTime, magnet if missing.
pub fn merge_fields(keep: &mut TorrentDetails, other: &TorrentDetails) {
    if other.sid > keep.sid {
        keep.sid = other.sid;
        keep.pir = other.pir;
    } else if other.sid == keep.sid && other.pir > keep.pir {
        keep.pir = other.pir;
    }
    if other.updateTime > keep.updateTime {
        keep.updateTime = other.updateTime;
    }
    if util::is_blank(&keep.magnet) && !util::is_blank(&other.magnet) {
        keep.magnet = other.magnet.clone();
    }
}

struct Spec<'a> {
    tracker: &'a str,
    canonical_host: String,
    drop_userdetails: bool,
    group_key: &'a dyn Fn(&str) -> Option<String>,
    /// `(group key, kept URL)` → canonical URL.
    canonical_url: &'a dyn Fn(&str, &str) -> Option<String>,
}

#[derive(Default)]
struct Counts {
    scanned: i64,
    rewritten: i64,
    merged: i64,
    removed: i64,
}

/// Case-insensitive set that remembers the first spelling added.
#[derive(Default)]
struct CiSet {
    seen: HashSet<String>,
    items: Vec<String>,
}

impl CiSet {
    fn add(&mut self, s: &str) {
        if self.seen.insert(s.to_lowercase()) {
            self.items.push(s.to_string());
        }
    }
    fn contains(&self, s: &str) -> bool {
        self.seen.contains(&s.to_lowercase())
    }
}

/// Pick the row to keep: first on the canonical host, else best (magnet, sid, updateTime),
/// earliest on ties.
fn pick_keep(urls: &[String], db: &fdb::ShardMap, canonical_host: &str) -> Option<String> {
    if let Some(u) = urls.iter().find(|u| host_of(u).map(|h| h.eq_ignore_ascii_case(canonical_host)).unwrap_or(false)) {
        return Some(u.clone());
    }
    let rank = |u: &String| {
        let t = &db[u];
        (!util::is_blank(&t.magnet), t.sid, t.updateTime)
    };
    let mut best: Option<&String> = None;
    for u in urls {
        match best {
            Some(b) if rank(u) <= rank(b) => {}
            _ => best = Some(u),
        }
    }
    best.cloned()
}

/// Process one shard in place. Returns true when it changed.
fn process_shard(db: &mut fdb::ShardMap, spec: &Spec, c: &mut Counts) -> bool {
    let mut groups: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut to_remove = CiSet::default();

    for (url, t) in db.iter() {
        if !t.trackerName.eq_ignore_ascii_case(spec.tracker) {
            continue;
        }
        c.scanned += 1;
        if spec.drop_userdetails && url.to_lowercase().contains("userdetails") {
            to_remove.add(url);
            c.removed += 1;
            continue;
        }
        let Some(gk) = (spec.group_key)(url) else { continue };
        groups.entry(gk).or_default().push(url.clone());
    }

    let mut to_write: IndexMap<String, (String, TorrentDetails)> = IndexMap::new();
    for (gk, urls) in &groups {
        let Some(keep_url) = pick_keep(urls, db, &spec.canonical_host) else { continue };
        let Some(canonical) = (spec.canonical_url)(gk, &keep_url) else { continue };
        let mut keep = db[&keep_url].clone();
        if keep.size <= 0.0 && !util::is_blank(&keep.sizeName) {
            // rows written before the size label parser accepted non-breaking spaces
            keep.size = fdb::size_from_name(&keep.sizeName) as f64;
        }
        let mut losers = 0;
        for u in urls {
            if u == &keep_url {
                continue;
            }
            let other = db[u].clone();
            merge_fields(&mut keep, &other);
            to_remove.add(u);
            losers += 1;
            c.merged += 1;
            c.removed += 1;
        }

        if !keep_url.eq_ignore_ascii_case(&canonical) {
            if db.contains_key(&canonical) && !to_remove.contains(&canonical) {
                // unexpected conflict: keep the current key (merged fields stay)
                db.insert(keep_url.clone(), keep);
                continue;
            }
            to_remove.add(&keep_url);
            keep.url = canonical.clone();
            // A new url must reach sync clients as a full row (see fdb::add_or_update_core).
            keep.updateTime = time::now();
            to_write.insert(canonical.to_lowercase(), (canonical, keep));
            c.rewritten += 1;
        } else {
            if losers > 0 {
                keep.url = canonical;
                keep.updateTime = time::now();
            }
            db.insert(keep_url.clone(), keep);
        }
    }

    if to_remove.items.is_empty() && to_write.is_empty() {
        return false;
    }
    for u in &to_remove.items {
        db.shift_remove(u);
    }
    for (_, (canonical, mut t)) in to_write {
        t.url = canonical.clone();
        db.insert(canonical, t);
    }
    true
}

fn run(spec: Spec) -> Value {
    let mut c = Counts::default();
    for (key, _) in fdb::master_db_snapshot() {
        let w = fdb::open_write(&key);
        w.modify(|db| process_shard(db, &spec, &mut c));
    }
    fdb::save_changes_to_file();
    json!({
        "ok": true,
        "scanned": c.scanned,
        "rewritten": c.rewritten,
        "merged": c.merged,
        "removed": c.removed,
        "canonicalHost": spec.canonical_host
    })
}

fn base_or(host: &str, default: &str) -> String {
    if host.is_empty() { default } else { host }.trim_end_matches('/').to_string()
}

fn kinozal_spec_parts() -> (String, String) {
    let host = conf().Kinozal.host.clone();
    (host_of(&host).unwrap_or_else(|| "kinozal.guru".into()), base_or(&host, "https://kinozal.guru"))
}

fn kinozal_group_key(url: &str) -> Option<String> {
    let id = rx::group(url, r"(?i)/details\.php\?id=(\d+)", 1);
    id.parse::<i32>().ok().filter(|i| *i > 0).map(|i| i.to_string())
}

pub fn fix_kinozal() -> Value {
    let (canonical_host, base) = kinozal_spec_parts();
    let canonical_url = move |id: &str, _: &str| Some(format!("{base}/details.php?id={id}"));
    run(Spec {
        tracker: "kinozal",
        canonical_host,
        drop_userdetails: true,
        group_key: &kinozal_group_key,
        canonical_url: &canonical_url,
    })
}

fn rutracker_group_key(url: &str) -> Option<String> {
    let id = rx::group(url, r"(?i)/viewtopic\.php\?t=(\d+)", 1);
    id.parse::<i32>().ok().filter(|i| *i > 0).map(|i| i.to_string())
}

pub fn fix_rutracker() -> Value {
    let host = conf().Rutracker.host.clone();
    let canonical_host = host_of(&host).unwrap_or_else(|| "rutracker.org".into());
    let base = base_or(&host, "https://rutracker.org");
    let canonical_url = move |id: &str, _: &str| Some(format!("{base}/forum/viewtopic.php?t={id}"));
    run(Spec {
        tracker: "rutracker",
        canonical_host,
        drop_userdetails: false,
        group_key: &rutracker_group_key,
        canonical_url: &canonical_url,
    })
}

fn selezen_group_key(url: &str) -> Option<String> {
    let id = rx::group(url, r"(?i)/relizy-ot-selezen/(\d+)-", 1);
    id.parse::<i32>().ok().filter(|i| *i > 0).map(|i| i.to_string())
}

/// Same path on the configured host (the slug is part of the URL, so it is taken from the kept row).
fn path_on_host(base: &str, url: &str) -> Option<String> {
    let path = rx::group(url, r"^https?://[^/]+(/.*)$", 1);
    (!path.is_empty()).then(|| format!("{base}{path}"))
}

pub fn fix_selezen() -> Value {
    let host = conf().Selezen.host.clone();
    let canonical_host = host_of(&host).unwrap_or_else(|| "open.selezen.org".into());
    let base = base_or(&host, "https://open.selezen.org");
    let canonical_url = move |_: &str, keep_url: &str| path_on_host(&base, keep_url);
    run(Spec {
        tracker: "selezen",
        canonical_host,
        drop_userdetails: false,
        group_key: &selezen_group_key,
        canonical_url: &canonical_url,
    })
}

fn ultradox_group_key(url: &str) -> Option<String> {
    let k = ultradox::canonical_path_and_fragment(url);
    (!k.is_empty() && k != "/").then_some(k)
}

pub fn fix_ultradox() -> Value {
    let host = conf().Ultradox.host.clone();
    let canonical_host = host_of(&host).unwrap_or_else(|| "ultradox.vip".into());
    let base = base_or(&host, "https://ultradox.vip");
    let canonical_url = move |path: &str, _: &str| {
        let u = ultradox::canonical_torrent_url(&base, path);
        (!u.is_empty()).then_some(u)
    };
    run(Spec {
        tracker: ultradox::TRACKER_NAME,
        canonical_host,
        drop_userdetails: false,
        group_key: &ultradox_group_key,
        canonical_url: &canonical_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn row(url: &str, magnet: &str, sid: i32) -> TorrentDetails {
        TorrentDetails { trackerName: "kinozal".into(), url: url.into(), magnet: magnet.into(), sid, ..Default::default() }
    }

    fn kinozal_spec<'a>(cu: &'a dyn Fn(&str, &str) -> Option<String>) -> Spec<'a> {
        Spec { tracker: "kinozal", canonical_host: "kinozal.guru".into(), drop_userdetails: true, group_key: &kinozal_group_key, canonical_url: cu }
    }

    #[test]
    fn kinozal_collapses_domains() {
        let mut db = fdb::ShardMap::new();
        let old = "https://kinozal.tv/details.php?id=5";
        let new = "https://kinozal.guru/details.php?id=5";
        let mut a = row(old, "magnet:?xt=urn:btih:aa", 10);
        a.updateTime = Utc::now() + Duration::hours(1);
        db.insert(old.into(), a);
        db.insert(new.into(), row(new, "", 3));
        db.insert("https://kinozal.tv/userdetails.php?id=1".into(), row("https://kinozal.tv/userdetails.php?id=1", "", 0));
        db.insert("https://kinozal.tv/details.php?id=7".into(), row("https://kinozal.tv/details.php?id=7", "m", 1));

        let cu = |id: &str, _: &str| Some(format!("https://kinozal.guru/details.php?id={id}"));
        let mut c = Counts::default();
        assert!(process_shard(&mut db, &kinozal_spec(&cu), &mut c));
        assert_eq!((c.scanned, c.rewritten, c.merged, c.removed), (4, 1, 1, 2));
        assert_eq!(db.len(), 2);
        let kept = &db[new];
        assert_eq!(kept.sid, 10);
        assert_eq!(kept.magnet, "magnet:?xt=urn:btih:aa");
        assert_eq!(kept.url, new);
        let moved = &db["https://kinozal.guru/details.php?id=7"];
        assert_eq!(moved.url, "https://kinozal.guru/details.php?id=7");
    }

    #[test]
    fn rutracker_collapses_domains_and_recomputes_size() {
        let mut db = fdb::ShardMap::new();
        let old = "https://rutracker.net/forum/viewtopic.php?t=6011397";
        let new = "https://rutracker.org/forum/viewtopic.php?t=6011397";
        let mut a = row(old, "magnet:?xt=urn:btih:aa", 21);
        a.trackerName = "rutracker".into();
        a.sizeName = "4.24 GB".into();
        a.size = 4552665333.0;
        db.insert(old.into(), a);
        let mut b = row(new, "magnet:?xt=urn:btih:aa", 14);
        b.trackerName = "rutracker".into();
        b.sizeName = "4.24\u{a0}GB".into();
        db.insert(new.into(), b);

        let cu = |id: &str, _: &str| Some(format!("https://rutracker.org/forum/viewtopic.php?t={id}"));
        let spec = Spec { tracker: "rutracker", canonical_host: "rutracker.org".into(), drop_userdetails: false, group_key: &rutracker_group_key, canonical_url: &cu };
        let mut c = Counts::default();
        assert!(process_shard(&mut db, &spec, &mut c));
        assert_eq!((c.scanned, c.rewritten, c.merged, c.removed), (2, 0, 1, 1));
        assert_eq!(db.len(), 1);
        let kept = &db[new];
        assert_eq!(kept.sid, 21);
        assert_eq!(kept.size, 4552665333.0);
        assert_eq!(rutracker_group_key("https://rutracker.org/forum/viewtopic.php?t=42"), Some("42".into()));
        assert_eq!(rutracker_group_key("https://rutracker.org/forum/viewforum.php?f=42"), None);
    }

    #[test]
    fn selezen_collapses_domains_keeping_path() {
        let mut db = fdb::ShardMap::new();
        let old = "https://selezen.org/relizy-ot-selezen/254-zveropolis-zootopia-2016.html";
        let new = "https://open.selezen.org/relizy-ot-selezen/254-zveropolis-zootopia-2016.html";
        let lone = "https://use.selezen.club/relizy-ot-selezen/887-holodnoe-serdce.html";
        for (u, sid) in [(old, 9), (new, 3), (lone, 1)] {
            let mut r = row(u, "magnet:?xt=urn:btih:aa", sid);
            r.trackerName = "selezen".into();
            db.insert(u.into(), r);
        }
        let cu = |_: &str, keep: &str| path_on_host("https://open.selezen.org", keep);
        let spec = Spec { tracker: "selezen", canonical_host: "open.selezen.org".into(), drop_userdetails: false, group_key: &selezen_group_key, canonical_url: &cu };
        let mut c = Counts::default();
        assert!(process_shard(&mut db, &spec, &mut c));
        assert_eq!((c.scanned, c.rewritten, c.merged, c.removed), (3, 1, 1, 1));
        assert_eq!(db.len(), 2);
        assert_eq!(db[new].sid, 9);
        let moved = "https://open.selezen.org/relizy-ot-selezen/887-holodnoe-serdce.html";
        assert_eq!(db[moved].url, moved);
    }

    #[test]
    fn pick_prefers_magnet_then_sid() {
        let mut db = fdb::ShardMap::new();
        db.insert("a".into(), row("a", "", 50));
        db.insert("b".into(), row("b", "m", 1));
        db.insert("c".into(), row("c", "m", 1));
        let now = Utc::now();
        for t in db.values_mut() {
            t.updateTime = now;
        }
        let urls: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(pick_keep(&urls, &db, "none").as_deref(), Some("b"));
    }
}
