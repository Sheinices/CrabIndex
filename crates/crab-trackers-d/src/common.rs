//! Helpers shared by the trackers in this crate.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};
use indexmap::IndexMap;

use crab_core::fdb;
use crab_core::models::{TaskMap, TaskParse, TorrentDetails};
use crab_core::parsing::parser_log;

/// Write a parser log line with `key=value` pairs (values already formatted).
pub fn log_kv(tracker: &str, message: &str, data: &[(&str, String)]) {
    let pairs: Vec<(String, String)> = data.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    parser_log::write_kv(tracker, message, &pairs);
}

/// Boolean rendered as `True` / `False` (log format used across the parser logs).
pub fn bool_str(b: bool) -> String {
    if b { "True".into() } else { "False".into() }
}

/// Seconds with one decimal, e.g. `12.3`.
pub fn secs_f1(started: std::time::Instant) -> String {
    format!("{:.1}", started.elapsed().as_secs_f64())
}

/// Trim trailing '/' characters.
pub fn trim_host(host: &str) -> String {
    host.trim_end_matches('/').to_string()
}

/// Lenient integer parse: optional sign, digits only, surrounding whitespace allowed.
pub fn try_int(s: &str) -> Option<i32> {
    s.trim().parse::<i32>().ok()
}

/// Group rows by FileDB bucket key, preserving first-seen order.
pub fn group_by_key<T: AsRef<TorrentDetails>>(torrents: Vec<T>) -> IndexMap<String, Vec<T>> {
    let mut groups: IndexMap<String, Vec<T>> = IndexMap::new();
    for t in torrents {
        let key = {
            let r = t.as_ref();
            fdb::key_for_torrent(&r.name, &r.originalname)
        };
        groups.entry(key).or_default().push(t);
    }
    groups
}

/// Current row stored under `url` in an open shard.
pub fn cached_row(w: &fdb::WriteGuard, url: &str) -> Option<TorrentDetails> {
    w.with_db(|db| db.get(url).cloned())
}

/// Case-sensitive trimmed comparison of two optional-ish strings.
pub fn same_trimmed(a: &str, b: &str) -> bool {
    a.trim() == b.trim()
}

/// True when the string contains a Cyrillic letter (А..я, Ё, ё).
pub fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|r| ('А'..='я').contains(&r) || r == 'Ё' || r == 'ё')
}

/// True when the string contains an ASCII Latin letter.
pub fn has_latin(s: &str) -> bool {
    s.chars().any(|r| r.is_ascii_alphabetic())
}

// ---------------------------------------------------------------------------
// Task slot maps (flat: section → pages)
// ---------------------------------------------------------------------------

/// Load a flat task map from `path` (empty on missing/invalid file).
pub fn load_task_map(path: &str) -> TaskMap {
    crab_core::trackers::read_json_file::<TaskMap>(path).unwrap_or_default()
}

/// Persist a flat task map atomically (errors ignored).
pub fn persist_task_map(path: &str, map: &TaskMap) {
    let _ = crab_core::trackers::cycle::write_json_atomic(path, map);
}

/// Drop map slots past the live 0-based last index (inclusive `page <= max_page`).
pub fn prune_pages_beyond_max(tasks: &mut Vec<TaskParse>, max_page: i32) -> i32 {
    if tasks.is_empty() {
        return 0;
    }
    let max_page = max_page.max(0);
    let before = tasks.len();
    tasks.retain(|t| t.page <= max_page);
    (before - tasks.len()) as i32
}

/// Ensure slots `0..=max_page` exist for `section`, prune the tail, sort by page.
/// Returns the number of pruned slots.
pub fn merge_forum_pages(map: &mut TaskMap, section: &str, max_page: i32) -> i32 {
    let max_page = max_page.max(0);
    let val = map.entry(section.to_string()).or_default();
    for page in 0..=max_page {
        if !val.iter().any(|i| i.page == page) {
            val.push(TaskParse::new(page));
        }
    }
    let pruned = prune_pages_beyond_max(val, max_page);
    val.sort_by_key(|x| x.page);
    pruned
}

// ---------------------------------------------------------------------------
// Europe/Moscow wall clock → UTC
// ---------------------------------------------------------------------------

fn last_sunday(year: i32, month: u32) -> Option<NaiveDate> {
    let first_next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1)? } else { NaiveDate::from_ymd_opt(year, month + 1, 1)? };
    let mut d = first_next.pred_opt()?;
    while d.weekday() != Weekday::Sun {
        d = d.pred_opt()?;
    }
    Some(d)
}

/// Offset in hours of Europe/Moscow for a local wall-clock time.
/// `None` for times skipped by the spring-forward transition.
pub fn moscow_offset_hours(local: NaiveDateTime) -> Option<i64> {
    let t2014 = NaiveDate::from_ymd_opt(2014, 10, 26)?.and_hms_opt(2, 0, 0)?;
    let t2011 = NaiveDate::from_ymd_opt(2011, 3, 27)?.and_hms_opt(2, 0, 0)?;
    if local >= t2014 {
        return Some(3);
    }
    if local >= t2011 {
        if local < t2011 + Duration::hours(1) {
            return None;
        }
        return Some(4);
    }
    let y = local.year();
    let start = last_sunday(y, 3)?.and_hms_opt(2, 0, 0)?;
    let end = last_sunday(y, 10)?.and_hms_opt(2, 0, 0)?;
    if local >= start && local < start + Duration::hours(1) {
        return None;
    }
    if local >= start + Duration::hours(1) && local < end {
        Some(4)
    } else {
        Some(3)
    }
}

/// Interpret `local` as Europe/Moscow wall-clock time and convert to UTC.
pub fn moscow_to_utc(local: NaiveDateTime) -> Option<DateTime<Utc>> {
    let off = moscow_offset_hours(local)?;
    Some(Utc.from_utc_datetime(&(local - Duration::hours(off))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moscow_offsets() {
        let d = |y, m, dd, h| NaiveDate::from_ymd_opt(y, m, dd).unwrap().and_hms_opt(h, 0, 0).unwrap();
        assert_eq!(moscow_offset_hours(d(2026, 7, 23, 8)), Some(3));
        assert_eq!(moscow_offset_hours(d(2013, 1, 1, 0)), Some(4));
        assert_eq!(moscow_offset_hours(d(2010, 7, 1, 0)), Some(4));
        assert_eq!(moscow_offset_hours(d(2010, 1, 1, 0)), Some(3));
    }

    #[test]
    fn merge_and_prune() {
        let mut m = TaskMap::new();
        assert_eq!(merge_forum_pages(&mut m, "1", 3), 0);
        assert_eq!(m["1"].len(), 4);
        assert_eq!(merge_forum_pages(&mut m, "1", 1), 2);
        assert_eq!(m["1"].iter().map(|t| t.page).collect::<Vec<_>>(), vec![0, 1]);
    }
}

/// Per-page upsert counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub fetched: i32,
    pub added: i32,
    pub updated: i32,
    pub skipped: i32,
    pub failed: i32,
}

impl Counts {
    pub fn add(&mut self, o: &Counts) {
        self.fetched += o.fetched;
        self.added += o.added;
        self.updated += o.updated;
        self.skipped += o.skipped;
        self.failed += o.failed;
    }
}

/// Lenient integer query value (unparsable → `default`).
pub fn q_int(v: &Option<String>, default: i32) -> i32 {
    v.as_deref().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(default)
}

/// Lenient boolean query value (`true`/`false`, case-insensitive; anything else → `default`).
pub fn q_bool(v: &Option<String>, default: bool) -> bool {
    match v.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        Some(s) if s == "true" => true,
        Some(s) if s == "false" => false,
        _ => default,
    }
}

/// First line of an error's text (used in "Error" log entries).
pub fn first_line(s: &str) -> String {
    s.split('\n').next().unwrap_or("").to_string()
}
