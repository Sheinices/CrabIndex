//! Persistent ParseAllTask cycle checkpoint: survives shutdown and stall cancel.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::{NestedTaskMap, TaskMap, TaskParse};
use crate::parsing::parser_log;
use crate::time;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ParseAllCycleState {
    pub CycleId: String,
    #[serde(with = "crate::time::net")]
    pub StartedAtUtc: DateTime<Utc>,
    pub MapFingerprint: Option<String>,
    pub MapCount: i64,
}

impl Default for ParseAllCycleState {
    fn default() -> Self {
        ParseAllCycleState { CycleId: String::new(), StartedAtUtc: time::min(), MapFingerprint: None, MapCount: 0 }
    }
}

pub const DEFAULT_FAIL_BUDGET: i32 = 3;

pub fn cycle_path_for_tracker(slug: &str) -> String {
    format!("Data/temp/{slug}_parseAllCycle.json")
}

pub fn task_parse_path_for_tracker(slug: &str) -> String {
    format!("Data/temp/{slug}_taskParse.json")
}

pub fn flat_map_keys(map: &TaskMap) -> Vec<String> {
    let mut cats: Vec<&String> = map.keys().collect();
    cats.sort();
    let mut out = Vec::new();
    for c in cats {
        let mut pages: Vec<i32> = map[c].iter().map(|p| p.page).collect();
        pages.sort();
        out.extend(pages.into_iter().map(|p| format!("{c}/{p}")));
    }
    out
}

pub fn nested_map_keys(map: &NestedTaskMap) -> Vec<String> {
    let mut cats: Vec<&String> = map.keys().collect();
    cats.sort();
    let mut out = Vec::new();
    for c in cats {
        let inner = &map[c];
        let mut args: Vec<&String> = inner.keys().collect();
        args.sort();
        for a in args {
            let mut pages: Vec<i32> = inner[a].iter().map(|p| p.page).collect();
            pages.sort();
            out.extend(pages.into_iter().map(|p| format!("{c}/{a}/{p}")));
        }
    }
    out
}

pub fn compute_fingerprint(keys: &[String]) -> String {
    let mut h = Sha256::new();
    h.update(keys.join("\n").as_bytes());
    hex::encode(h.finalize())
}

pub fn load_state(path: &str) -> Option<ParseAllCycleState> {
    super::read_json_file(path)
}

/// Indented JSON written via temp file + rename.
pub fn write_json_atomic<T: Serialize + ?Sized>(path: &str, value: &T) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    crate::config::write_atomically(path, &json)
}

pub fn save_state(path: &str, state: &ParseAllCycleState) {
    let _ = write_json_atomic(path, state);
}

pub fn is_pending_in_cycle(page: &TaskParse, cycle: &ParseAllCycleState) -> bool {
    if cycle.CycleId.is_empty() {
        return true;
    }
    page.parseAllCycleId.as_deref() != Some(cycle.CycleId.as_str())
}

pub fn count_pending_in_cycle<'a>(pages: impl IntoIterator<Item = &'a TaskParse>, cycle: &ParseAllCycleState) -> i64 {
    pages.into_iter().filter(|p| is_pending_in_cycle(p, cycle)).count() as i64
}

pub fn mark_done_in_cycle(page: &mut TaskParse, cycle: &ParseAllCycleState) {
    page.parseAllCycleId = Some(cycle.CycleId.clone());
    page.updateTime = time::today_local();
}

/// Settle a slot on success, or skip after [`DEFAULT_FAIL_BUDGET`] consecutive failures.
pub fn note_attempt(tracker: &str, page: &mut TaskParse, cycle: &ParseAllCycleState, ok: bool) -> bool {
    if ok {
        page.parseAllFailCount = 0;
        mark_done_in_cycle(page, cycle);
        return true;
    }
    page.parseAllFailCount += 1;
    if page.parseAllFailCount < DEFAULT_FAIL_BUDGET {
        return false;
    }
    page.parseAllFailCount = 0;
    mark_done_in_cycle(page, cycle);
    parser_log::write(tracker, format!("ParseAll skip slot page={} after {DEFAULT_FAIL_BUDGET} failures", page.page));
    true
}

pub fn create_cycle(fingerprint: &str, map_count: i64) -> ParseAllCycleState {
    ParseAllCycleState {
        CycleId: uuid::Uuid::new_v4().simple().to_string(),
        StartedAtUtc: Utc::now(),
        MapFingerprint: Some(fingerprint.to_string()),
        MapCount: map_count,
    }
}

fn migrate_today_stamps<'a>(pages: impl Iterator<Item = &'a mut TaskParse>, cycle: &ParseAllCycleState) {
    let today = chrono::Local::now().date_naive();
    for p in pages {
        if p.updateTime.with_timezone(&chrono::Local).date_naive() == today {
            p.parseAllCycleId = Some(cycle.CycleId.clone());
        }
    }
}

/// Load or rotate the cycle for a full ParseAllTask run.
pub fn begin_full_cycle<'a, I>(cycle_path: &str, fingerprint: &str, map_count: i64, pages: I, rotate_if_complete: bool) -> ParseAllCycleState
where
    I: Iterator<Item = &'a mut TaskParse>,
{
    let mut pages: Vec<&mut TaskParse> = pages.collect();
    let Some(mut state) = load_state(cycle_path) else {
        let state = create_cycle(fingerprint, map_count);
        migrate_today_stamps(pages.iter_mut().map(|p| &mut **p), &state);
        save_state(cycle_path, &state);
        return state;
    };
    state.MapFingerprint = Some(fingerprint.to_string());
    state.MapCount = map_count;
    let pending = pages.iter().filter(|p| is_pending_in_cycle(p, &state)).count();
    if rotate_if_complete && pending == 0 {
        let state = create_cycle(fingerprint, map_count);
        save_state(cycle_path, &state);
        return state;
    }
    save_state(cycle_path, &state);
    state
}

/// Load active cycle for ParseLatest (no rotation).
pub fn load_active_cycle<'a, I>(cycle_path: &str, fingerprint: &str, map_count: i64, pages: I) -> ParseAllCycleState
where
    I: Iterator<Item = &'a mut TaskParse>,
{
    let Some(mut state) = load_state(cycle_path) else {
        let state = create_cycle(fingerprint, map_count);
        migrate_today_stamps(pages, &state);
        save_state(cycle_path, &state);
        return state;
    };
    state.MapFingerprint = Some(fingerprint.to_string());
    state.MapCount = map_count;
    save_state(cycle_path, &state);
    state
}

pub fn persist_after_page<T: Serialize + ?Sized>(cycle_path: &str, cycle: Option<&ParseAllCycleState>, task_parse_path: &str, task_parse: &T) {
    let _ = write_json_atomic(task_parse_path, task_parse);
    if let Some(c) = cycle {
        save_state(cycle_path, c);
    }
}

pub fn persist_after_page_if_needed<T: Serialize + ?Sized>(
    cycle_path: &str,
    cycle: Option<&ParseAllCycleState>,
    task_parse_path: &str,
    task_parse: &T,
    completed: i64,
    total: i64,
) {
    if super::sync::should_persist_checkpoint(completed, total) {
        persist_after_page(cycle_path, cycle, task_parse_path, task_parse);
    }
}

pub fn format_start_log(cycle: &ParseAllCycleState, pending: i64, total: i64) -> String {
    let fp = cycle.MapFingerprint.as_deref().unwrap_or("");
    format!(
        "cycle={} pending={pending}/{total} started={}Z fingerprint={}",
        cycle.CycleId,
        cycle.StartedAtUtc.format("%Y-%m-%d %H:%M:%S"),
        &fp[..fp.len().min(12)]
    )
}

pub fn format_cancel_log(cycle: Option<&ParseAllCycleState>, pending_left: i64, total: i64) -> String {
    let map = cycle.map(|c| c.MapCount).filter(|m| *m > 0).unwrap_or(total);
    let cycle_part = cycle.filter(|c| !c.CycleId.is_empty()).map(|c| format!(" cycle={}", c.CycleId)).unwrap_or_default();
    format!("pending left={pending_left}/{map}{cycle_part}")
}

/// All TaskParse slots from a flat or nested taskParse JSON file.
pub fn load_task_parse_pages(slug: &str) -> Vec<TaskParse> {
    let Some(v) = super::read_json_file::<serde_json::Value>(&task_parse_path_for_tracker(slug)) else {
        return Vec::new();
    };
    flatten_task_parse_value(&v)
}

pub fn flatten_task_parse_value(v: &serde_json::Value) -> Vec<TaskParse> {
    let mut list = Vec::new();
    let Some(root) = v.as_object() else { return list };
    let add = |list: &mut Vec<TaskParse>, arr: &Vec<serde_json::Value>| {
        for item in arr {
            if let Ok(p) = serde_json::from_value::<TaskParse>(item.clone()) {
                list.push(p);
            }
        }
    };
    for (_, val) in root {
        if let Some(arr) = val.as_array() {
            add(&mut list, arr);
        } else if let Some(nested) = val.as_object() {
            for (_, inner) in nested {
                if let Some(arr) = inner.as_array() {
                    add(&mut list, arr);
                }
            }
        }
    }
    list
}

/// (cycle, pending, mapCount) from disk.
pub fn read_cycle_progress(slug: &str) -> (Option<ParseAllCycleState>, i64, i64) {
    let pages = load_task_parse_pages(slug);
    let cycle = load_state(&cycle_path_for_tracker(slug));
    let map_count = cycle.as_ref().map(|c| c.MapCount).filter(|m| *m > 0).unwrap_or(pages.len() as i64);
    let pending = match &cycle {
        None => pages.len() as i64,
        Some(c) => count_pending_in_cycle(pages.iter(), c),
    };
    (cycle, pending, map_count)
}

pub fn has_pending_work(slug: &str) -> bool {
    let (c, pending, _) = read_cycle_progress(slug);
    c.is_some() && pending > 0
}

/// Flat map: begin full run → (cycle, mapCount, pendingCount).
pub fn begin_flat_full_run(slug: &str, task_parse: &mut TaskMap) -> (ParseAllCycleState, i64, i64) {
    let fp = compute_fingerprint(&flat_map_keys(task_parse));
    let count = task_parse.values().map(|v| v.len()).sum::<usize>() as i64;
    let cycle = begin_full_cycle(&cycle_path_for_tracker(slug), &fp, count, task_parse.values_mut().flat_map(|v| v.iter_mut()), true);
    let pending = count_pending_in_cycle(task_parse.values().flat_map(|v| v.iter()), &cycle);
    (cycle, count, pending)
}

pub fn load_flat_active_cycle(slug: &str, task_parse: &mut TaskMap) -> ParseAllCycleState {
    let fp = compute_fingerprint(&flat_map_keys(task_parse));
    let count = task_parse.values().map(|v| v.len()).sum::<usize>() as i64;
    load_active_cycle(&cycle_path_for_tracker(slug), &fp, count, task_parse.values_mut().flat_map(|v| v.iter_mut()))
}

pub fn begin_nested_full_run(slug: &str, task_parse: &mut NestedTaskMap) -> (ParseAllCycleState, i64, i64) {
    let fp = compute_fingerprint(&nested_map_keys(task_parse));
    let count = task_parse.values().flat_map(|m| m.values()).map(|v| v.len()).sum::<usize>() as i64;
    let cycle = begin_full_cycle(
        &cycle_path_for_tracker(slug),
        &fp,
        count,
        task_parse.values_mut().flat_map(|m| m.values_mut()).flat_map(|v| v.iter_mut()),
        true,
    );
    let pending = count_pending_in_cycle(task_parse.values().flat_map(|m| m.values()).flat_map(|v| v.iter()), &cycle);
    (cycle, count, pending)
}

pub fn load_nested_active_cycle(slug: &str, task_parse: &mut NestedTaskMap) -> ParseAllCycleState {
    let fp = compute_fingerprint(&nested_map_keys(task_parse));
    let count = task_parse.values().flat_map(|m| m.values()).map(|v| v.len()).sum::<usize>() as i64;
    load_active_cycle(&cycle_path_for_tracker(slug), &fp, count, task_parse.values_mut().flat_map(|m| m.values_mut()).flat_map(|v| v.iter_mut()))
}
