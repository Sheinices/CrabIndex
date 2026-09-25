//! Shard rows stored as JSON `null`.
//!
//! The shard loader in core silently skips `null` rows. Maintenance and dev tools
//! still need to count and drop them, so they look at the raw JSON on disk here.

use crab_core::fdb::{self, ShardMap};
use indexmap::IndexMap;
use serde_json::Value;

fn raw_shard(key: &str) -> Option<IndexMap<String, Value>> {
    let path = fdb::path_for_key(key);
    if !std::path::Path::new(&path).exists() {
        return None;
    }
    fdb::read_gz_json::<IndexMap<String, Value>>(&path)
}

fn null_urls(key: &str) -> Vec<String> {
    raw_shard(key).map(|raw| raw.into_iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k).collect()).unwrap_or_default()
}

/// Rows of a shard (core skips `null` rows when loading) plus the urls stored as `null` on disk.
pub fn load_rows_lenient(key: &str) -> (ShardMap, Vec<String>) {
    let rows = fdb::open_read(key, false, false);
    let nulls = null_urls(key).into_iter().filter(|u| !rows.contains_key(u)).collect();
    (rows, nulls)
}

/// Rewrite the shard file without its `null` rows; returns how many were dropped.
pub fn purge_null_rows(key: &str) -> usize {
    let Some(raw) = raw_shard(key) else { return 0 };
    let before = raw.len();
    let kept: IndexMap<String, Value> = raw.into_iter().filter(|(_, v)| !v.is_null()).collect();
    let removed = before - kept.len();
    if removed > 0 && !kept.is_empty() {
        fdb::write_gz_json(&fdb::path_for_key(key), &kept);
    }
    removed
}

/// `open_write` that also drops `null` rows from the file on disk.
/// Returns the guard and the number of `null` rows removed.
pub fn open_write_clean(key: &str) -> (fdb::WriteGuard, usize) {
    let w = fdb::open_write(key);
    let n = null_urls(key).len();
    if n > 0 {
        if w.is_empty() {
            purge_null_rows(key);
        } else {
            // the loaded shard has no null rows; saving it rewrites the file without them
            w.mark_changed();
        }
    }
    (w, n)
}
