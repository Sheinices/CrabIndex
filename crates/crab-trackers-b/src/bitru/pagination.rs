//! BitRu api.php date cursors.
//!
//! Live behaviour (the official docs label these the other way round):
//! request `after_date=X` → items with added strictly less than X (older);
//! request `before_date=X` → items with added strictly greater than X (newer).
//! The response `after_date` = max(added), `before_date` = min(added) on the page.
//! Older pagination: next request `after_date` = previous result `before_date`.

use serde_json::{Map, Value};
use std::collections::HashSet;

use super::categories;
use super::models::BitruApiResult;
use crab_core::rx;

pub const MAX_PAGES_HARD_LIMIT: i32 = 50;
pub const AFTER_DATE_PARAM: &str = "after_date";

pub fn clamp_pages(pages: i32) -> i32 {
    pages.clamp(1, MAX_PAGES_HARD_LIMIT)
}

pub fn clamp_limit(limit: i32) -> i32 {
    limit.clamp(1, 100)
}

/// Non-zero unix seconds from a JSON number or numeric string.
pub fn try_parse_unix(value: &Value) -> Option<i64> {
    let v = match value {
        Value::Number(n) => n.as_i64()?,
        Value::String(s) => s.parse::<i64>().ok()?,
        _ => return None,
    };
    (v != 0).then_some(v)
}

/// `{"limit":N,"category":[...],"after_date":"X"?}` in that key order.
pub fn build_request_params(limit: i32, older_than_unix: Option<i64>) -> Map<String, Value> {
    let mut p = Map::new();
    p.insert("limit".into(), Value::from(clamp_limit(limit)));
    p.insert("category".into(), Value::from(categories::REQUEST_CATEGORIES.to_vec()));
    if let Some(u) = older_than_unix {
        p.insert(AFTER_DATE_PARAM.into(), Value::from(u.to_string()));
    }
    p
}

/// Cursor for the next older page; `None` when missing, zero or unchanged vs `previous_cursor`.
pub fn try_get_next_older_page_cursor(result: Option<&BitruApiResult>, previous_cursor: Option<i64>) -> Option<i64> {
    let next = try_parse_unix(&result?.before_date)?;
    if previous_cursor == Some(next) {
        return None;
    }
    Some(next)
}

/// True when every id of `current` was already on the previous page.
pub fn is_duplicate_page(previous: Option<&HashSet<i64>>, current: Option<&HashSet<i64>>) -> bool {
    let (Some(prev), Some(cur)) = (previous, current) else { return false };
    if cur.is_empty() || prev.is_empty() {
        return false;
    }
    cur.iter().all(|id| prev.contains(id))
}

pub fn try_extract_torrent_id(url: &str) -> Option<i64> {
    if url.trim().is_empty() {
        return None;
    }
    let id: i64 = rx::group_i(url, r"[?&]id=(\d+)", 1).parse().ok()?;
    (id != 0).then_some(id)
}

pub fn collect_torrent_ids<'a>(urls: impl IntoIterator<Item = &'a str>) -> HashSet<i64> {
    urls.into_iter().filter_map(try_extract_torrent_id).collect()
}
