// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Global `(tracker, torrent id) → bucket` index, so a torrent that re-appears under a new
//! name (different bucket) replaces its old row instead of adding a second one.
//!
//! [`add_or_update_core`](super::add_or_update_core) already merges rows with the same id
//! inside one bucket; the buckets differ whenever the parsed name changed (rutor edits titles
//! per episode, kinozal rows imported from jacred had other names). The index is built from
//! the whole FileDB in the background after start (~2 min per 2 M rows) and kept up to date by
//! [`FileDb::add_or_update`](super::FileDb::add_or_update); stale entries (row deleted by a
//! migration) are harmless: the removal step finds nothing. A daily rebuild drops them.
//!
//! Memory: about 30 bytes per row of an id-bearing tracker plus the interned bucket keys
//! (roughly 100 MB for 2.3 M rows).

use dashmap::DashMap;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::torrent_id_from_url;

static INDEX: Lazy<DashMap<u64, u32>> = Lazy::new(DashMap::new);
/// Interned bucket keys: index → key and key → index.
static KEYS: Lazy<RwLock<Vec<Arc<str>>>> = Lazy::new(|| RwLock::new(Vec::new()));
static KEY_IDS: Lazy<DashMap<Arc<str>, u32>> = Lazy::new(DashMap::new);
static READY: AtomicBool = AtomicBool::new(false);
static BUILDING: AtomicBool = AtomicBool::new(false);

fn fnv32(s: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

fn slot(tracker: &str, id: i32) -> u64 {
    ((fnv32(&tracker.to_ascii_lowercase()) as u64) << 32) | (id as u32 as u64)
}

fn intern(key: &str) -> u32 {
    if let Some(i) = KEY_IDS.get(key) {
        return *i;
    }
    let mut keys = KEYS.write();
    if let Some(i) = KEY_IDS.get(key) {
        return *i;
    }
    let arc: Arc<str> = Arc::from(key);
    let i = keys.len() as u32;
    keys.push(arc.clone());
    KEY_IDS.insert(arc, i);
    i
}

fn key_of(i: u32) -> Option<Arc<str>> {
    KEYS.read().get(i as usize).cloned()
}

/// True once the initial scan finished; before that [`lookup`] answers `None` and callers
/// fall back to the in-bucket match.
pub fn is_ready() -> bool {
    READY.load(Ordering::Relaxed)
}

pub fn len() -> usize {
    INDEX.len()
}

/// Remember that `(tracker, id)` lives in bucket `key`.
pub fn record(tracker: &str, id: i32, key: &str) {
    if id <= 0 || key.is_empty() {
        return;
    }
    INDEX.insert(slot(tracker, id), intern(key));
}

/// Bucket that holds `(tracker, id)`, when known.
pub fn lookup(tracker: &str, id: i32) -> Option<Arc<str>> {
    if id <= 0 || !is_ready() {
        return None;
    }
    INDEX.get(&slot(tracker, id)).and_then(|i| key_of(*i))
}

pub fn forget(tracker: &str, id: i32) {
    INDEX.remove(&slot(tracker, id));
}

/// One torrent id found in several buckets during [`build`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Duplicate {
    pub tracker: String,
    pub id: i32,
    pub keys: Vec<Arc<str>>,
}

/// Full scan of the FileDB: returns the number of indexed rows and every `(tracker, id)` that
/// sits in more than one bucket, i.e. duplicates that slipped in while the index was not ready
/// (rows written in the first minutes after a start) - [`super::dedupe_ids`] removes them. A
/// second concurrent call returns immediately with `(0, [])`.
pub fn build() -> (usize, Vec<Duplicate>) {
    if BUILDING.swap(true, Ordering::SeqCst) {
        return (0, Vec::new());
    }
    let _pace = super::pace::scan();
    let fresh: DashMap<u64, u32> = DashMap::new();
    // slot → (tracker, id, buckets) for ids seen in more than one bucket
    let dups: DashMap<u64, (String, i32, Vec<u32>)> = DashMap::new();
    let mut n = 0usize;
    for (key, _) in super::master_db_snapshot() {
        let idx = intern(&key);
        for (url, t) in super::open_read(&key, false, false) {
            if t.trackerName.is_empty() {
                continue;
            }
            let id = torrent_id_from_url(&t.trackerName, &url);
            if id > 0 {
                let sl = slot(&t.trackerName, id);
                n += 1;
                if let Some(prev) = fresh.insert(sl, idx) {
                    if prev != idx {
                        let mut e = dups.entry(sl).or_insert_with(|| (t.trackerName.to_lowercase(), id, vec![prev]));
                        if !e.2.contains(&idx) {
                            e.2.push(idx);
                        }
                    }
                }
            }
        }
    }
    // swap in: entries recorded while scanning win over the scan (they are newer); when the
    // scan saw the same id elsewhere, that older copy is a duplicate too
    let live: Vec<(u64, u32)> = INDEX.iter().map(|e| (*e.key(), *e.value())).collect();
    INDEX.clear();
    for e in fresh.iter() {
        INDEX.insert(*e.key(), *e.value());
    }
    for (k, v) in live {
        if let Some(scanned) = fresh.get(&k) {
            if *scanned != v {
                if let Some(mut e) = dups.get_mut(&k) {
                    if !e.2.contains(&v) {
                        e.2.push(v);
                    }
                }
                // a duplicate only known from the live entry: tracker name is not at hand, the
                // daily rebuild catches it once both copies are on disk
            }
        }
        INDEX.insert(k, v);
    }
    READY.store(true, Ordering::SeqCst);
    BUILDING.store(false, Ordering::SeqCst);
    let list = dups
        .into_iter()
        .map(|(_, (tracker, id, idxs))| Duplicate { tracker, id, keys: idxs.into_iter().filter_map(key_of).collect() })
        .filter(|d| d.keys.len() > 1)
        .collect();
    (n, list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_lookup_and_intern() {
        READY.store(true, Ordering::SeqCst);
        record("Rutor", 405179, "друзья:друзья");
        record("rutor", 405179, "друзья:друзья1939");
        assert_eq!(lookup("rutor", 405179).as_deref(), Some("друзья:друзья1939"));
        assert_eq!(lookup("kinozal", 405179), None);
        assert_eq!(intern("друзья:друзья"), intern("друзья:друзья"));
        forget("rutor", 405179);
        assert_eq!(lookup("rutor", 405179), None);
        record("x", 0, "k");
        assert_eq!(lookup("x", 0), None);
    }
}
