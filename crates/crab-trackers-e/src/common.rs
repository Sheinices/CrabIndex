//! Small helpers shared by the trackers in this crate.

use std::collections::HashMap;

use chrono::{DateTime, Timelike, Utc};
use crab_core::fdb::{self, ShardMap};
use crab_core::models::{TaskMap, TorrentDetails};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};

/// Build `[(String, String); N]` key/value pairs for `parser_log::write_kv`.
macro_rules! kv {
    ($(($k:expr, $v:expr)),* $(,)?) => {
        [$(($k.to_string(), $v.to_string())),*]
    };
}
pub(crate) use kv;

/// Boolean text as written in tracker logs and status strings ("True"/"False").
pub fn bool_text(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

/// Round-trip timestamp with 7 fractional digits and `Z` (e.g. `2026-09-25T10:11:12.1234567Z`).
pub fn iso_o(dt: &DateTime<Utc>) -> String {
    format!("{}.{:07}Z", dt.format("%Y-%m-%dT%H:%M:%S"), dt.nanosecond() % 1_000_000_000 / 100)
}

/// Same as [`iso_o`] but with trailing fractional zeros trimmed (JSON checkpoint style).
pub fn iso_trimmed(dt: &DateTime<Utc>) -> String {
    let frac = format!("{:07}", dt.nanosecond() % 1_000_000_000 / 100);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{}Z", dt.format("%Y-%m-%dT%H:%M:%S"))
    } else {
        format!("{}.{frac}Z", dt.format("%Y-%m-%dT%H:%M:%S"))
    }
}

/// Load a flat task map (`Data/temp/{tracker}_taskParse.json`); empty on any error.
pub fn load_task_map(path: &str) -> TaskMap {
    crab_core::trackers::read_json_file::<TaskMap>(path).unwrap_or_default()
}

/// `null` / missing → `T::default()`.
pub fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Query helpers (lenient: unparsable values fall back to the default)
// ---------------------------------------------------------------------------

pub type QueryMap = HashMap<String, String>;

pub fn q_i32(q: &QueryMap, name: &str, default: i32) -> i32 {
    q.get(name).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(default)
}

pub fn q_bool(q: &QueryMap, name: &str, default: bool) -> bool {
    match q.get(name).map(|v| v.trim()) {
        Some(v) if v.eq_ignore_ascii_case("true") => true,
        Some(v) if v.eq_ignore_ascii_case("false") => false,
        _ => default,
    }
}

pub fn q_str(q: &QueryMap, name: &str) -> Option<String> {
    q.get(name).cloned()
}

// ---------------------------------------------------------------------------
// FileDB upsert helpers
// ---------------------------------------------------------------------------

/// Group rows by FileDB bucket key (name/originalname), preserving input order.
pub fn group_by_bucket<T: AsRef<TorrentDetails>>(torrents: Vec<T>) -> IndexMap<String, Vec<T>> {
    let mut groups: IndexMap<String, Vec<T>> = IndexMap::new();
    for t in torrents {
        let key = {
            let r = t.as_ref();
            fdb::key_db(&r.name, &r.originalname)
        };
        groups.entry(key).or_default().push(t);
    }
    groups
}

/// Cached row for `url` in an open shard.
pub fn cached_row(w: &fdb::WriteGuard, url: &str) -> Option<TorrentDetails> {
    w.with_db(|db: &mut ShardMap| db.get(url).cloned())
}

/// Human seconds with one decimal, as used in "took N.Ns" log lines.
pub fn secs(start: std::time::Instant) -> String {
    format!("{:.1}", start.elapsed().as_secs_f64())
}
