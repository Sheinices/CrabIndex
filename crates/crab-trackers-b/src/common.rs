//! Helpers shared by the trackers in this crate: a small TTL cache for login cookies,
//! task-map persistence, lenient date parsing and an async FileDB upsert loop.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

use crab_core::models::{TaskMap, TaskParse};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// In-memory TTL cache (cookies, login back-off markers)
// ---------------------------------------------------------------------------

static MEM: Lazy<DashMap<String, (String, Instant)>> = Lazy::new(DashMap::new);

/// Value stored under `key` unless expired.
pub fn cache_get(key: &str) -> Option<String> {
    let hit = MEM.get(key).map(|e| e.value().clone());
    match hit {
        Some((v, exp)) if Instant::now() < exp => Some(v),
        Some(_) => {
            MEM.remove(key);
            None
        }
        None => None,
    }
}

pub fn cache_has(key: &str) -> bool {
    cache_get(key).is_some()
}

pub fn cache_set(key: &str, value: impl Into<String>, ttl: Duration) {
    MEM.insert(key.to_string(), (value.into(), Instant::now() + ttl));
}

// ---------------------------------------------------------------------------
// Task maps
// ---------------------------------------------------------------------------

pub fn load_task_map(path: &str) -> TaskMap {
    crab_core::trackers::read_json_file::<TaskMap>(path).unwrap_or_default()
}

pub fn persist_task_map(path: &str, map: &TaskMap) {
    let _ = crab_core::trackers::cycle::write_json_atomic(path, map);
}

/// Drop slots at or past the live page count (exclusive `page < page_count`). Returns removed count.
pub fn prune_pages_beyond_page_count(tasks: Option<&mut Vec<TaskParse>>, page_count: i32) -> i32 {
    let Some(tasks) = tasks else { return 0 };
    if tasks.is_empty() {
        return 0;
    }
    let page_count = page_count.max(1);
    let before = tasks.len();
    tasks.retain(|t| t.page < page_count);
    (before - tasks.len()) as i32
}

/// Add slots `0..page_count` that are missing.
pub fn ensure_pages(val: &mut Vec<TaskParse>, page_count: i32) {
    for page in 0..page_count {
        if !val.iter().any(|i| i.page == page) {
            val.push(TaskParse::new(page));
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Text before the first `[`, `/`, `(` or `|`, trimmed.
pub fn first_title_segment(title: &str) -> String {
    let end = title.find(['[', '/', '(', '|']).unwrap_or(title.len());
    title[..end].trim().to_string()
}

/// Lenient "yyyy-MM-dd HH:mm" / "yyyy.MM.dd HH:mm" parse. The wall-clock value is kept as UTC.
pub fn parse_ymd_hm(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for f in ["%Y-%m-%d %H:%M", "%Y.%m.%d %H:%M", "%Y/%m/%d %H:%M", "%Y-%m-%d %H:%M:%S", "%Y.%m.%d %H:%M:%S"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return Some(Utc.from_utc_datetime(&n));
        }
    }
    None
}

/// `int.TryParse`-like: trimmed, optional sign, digits only.
pub fn parse_int(s: &str) -> i32 {
    s.trim().parse::<i32>().unwrap_or(0)
}

/// Elapsed time as `hh:mm:ss.fffffff`.
pub fn fmt_elapsed(d: Duration) -> String {
    let total = d.as_secs();
    let ticks = d.subsec_nanos() / 100;
    let days = total / 86400;
    let (h, m, s) = ((total % 86400) / 3600, (total % 3600) / 60, total % 60);
    if days > 0 {
        format!("{days}.{h:02}:{m:02}:{s:02}.{ticks:07}")
    } else {
        format!("{h:02}:{m:02}:{s:02}.{ticks:07}")
    }
}

/// Values of every `Set-Cookie` header.
pub fn set_cookies(headers: &reqwest::header::HeaderMap) -> Vec<String> {
    headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(|s| s.to_string())).collect()
}

/// Client for login POSTs: no redirects, invalid certs accepted.
pub fn login_client(timeout_secs: u64) -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .ok()
}

// ---------------------------------------------------------------------------
// FileDB upsert with an async per-item step
// ---------------------------------------------------------------------------

pub use crab_core::fdb::{add_or_update_async, by_url};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_segment() {
        assert_eq!(first_title_segment("Name [2020] x"), "Name");
        assert_eq!(first_title_segment("  plain  "), "plain");
    }

    #[test]
    fn elapsed_format() {
        assert_eq!(fmt_elapsed(Duration::from_millis(83_456)), "00:01:23.4560000");
    }

    #[test]
    fn ymd() {
        assert!(parse_ymd_hm("2024-07-16 12:00").is_some());
        assert!(parse_ymd_hm("2024.07.16 12:00").is_some());
        assert!(parse_ymd_hm("x").is_none());
    }
}

/// Lenient integer query parameter: missing or unparsable → `default`.
pub fn q_int(v: &Option<String>, default: i32) -> i32 {
    v.as_deref().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(default)
}

/// Optional non-empty string query parameter.
pub fn q_str(v: &Option<String>) -> Option<String> {
    v.as_ref().filter(|s| !s.is_empty()).cloned()
}

/// Split a comma-separated list, trimming entries and dropping empties.
pub fn split_csv(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).map(|x| x.to_string()).collect()
}

/// Capitalised bool text (`True` / `False`).
pub fn bool_text(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

/// Groups of the first match (index 0 = whole match; missing groups ""), `None` when no match.
pub fn match_groups(text: &str, pattern: &str, ignore_case: bool) -> Option<Vec<String>> {
    let c = if ignore_case { crab_core::rx::captures_i(text, pattern) } else { crab_core::rx::captures(text, pattern) }?;
    Some((0..c.len()).map(|i| c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default()).collect())
}
