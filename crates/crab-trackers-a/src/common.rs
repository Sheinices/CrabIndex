//! Helpers shared by the trackers in this crate: row field matching, title fallback,
//! task-map persistence, generic flat ParseAllTask / ParseLatest loops, async FileDB upsert
//! and small HTTP query helpers for the cron routes.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, TimeZone, Utc};
use indexmap::IndexMap;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crab_core::fdb;
use crab_core::models::{TaskMap, TaskParse, TorrentDetails};
use crab_core::parsing::parser_log;
use crab_core::rx;
use crab_core::trackers::{self, cycle, Cancelled, ParseLock};
use crab_core::util;

// ---------------------------------------------------------------------------
// Row matching
// ---------------------------------------------------------------------------

/// First match of `pattern` (case-insensitive) in `row`, group `index`, HTML-decoded,
/// whitespace runs collapsed to one space and trimmed. `""` when nothing matched.
pub fn match_row(row: &str, pattern: &str, index: usize) -> String {
    let raw = rx::group_i(row, pattern, index);
    let res = util::html_decode(raw.trim());
    let res = rx::replace(&res, "[\n\r\t ]+", " ");
    res.trim().to_string()
}

/// [`match_row`] that also turns non-breaking spaces into plain spaces.
pub fn match_row_nbsp(row: &str, pattern: &str, index: usize) -> String {
    match_row(row, pattern, index).replace('\u{00a0}', " ").trim().to_string()
}

/// Fallback name: the title up to the first `[`, `/`, `(` or `|`.
pub fn name_before_brackets(title: &str) -> String {
    rx::split_i(title, r"(\[|\/|\(|\|)").into_iter().next().unwrap_or_default().trim().to_string()
}

/// Lenient integer parse (0 on failure).
pub fn parse_int(s: &str) -> i32 {
    s.trim().parse().unwrap_or(0)
}

/// Year from a regex group ("" → 0).
pub fn year(s: &str) -> i32 {
    s.parse().unwrap_or(0)
}

/// Regex groups of the first match (case-sensitive); all "" when no match.
pub fn g(text: &str, pattern: &str) -> Vec<String> {
    rx::groups(text, pattern)
}

/// Non-blank check used by title parsers.
pub fn nb(s: &str) -> bool {
    !util::is_blank(s)
}

/// Drop slots whose page is past `max_page` (inclusive bound); returns how many were removed.
pub fn prune_pages_beyond_max(tasks: &mut Vec<TaskParse>, max_page: i32) -> i32 {
    if tasks.is_empty() {
        return 0;
    }
    let max_page = max_page.max(0);
    let before = tasks.len();
    tasks.retain(|t| t.page <= max_page);
    (before - tasks.len()) as i32
}

/// UTC midnight of the given date plus hours/minutes; `None` for invalid components.
pub fn utc_datetime(y: i32, m: u32, d: u32, h: u32, min: u32) -> Option<DateTime<Utc>> {
    Utc.with_ymd_and_hms(y, m, d, h, min, 0).single()
}

// ---------------------------------------------------------------------------
// Task map persistence
// ---------------------------------------------------------------------------

/// Load a flat task map from disk (empty on missing/invalid file).
pub fn load_task_map(path: &str) -> TaskMap {
    trackers::read_json_file::<TaskMap>(path).unwrap_or_default()
}

/// Write a task map (indented JSON, atomic). Errors are ignored.
pub fn persist<T: serde::Serialize + ?Sized>(path: &str, value: &T) {
    let _ = cycle::write_json_atomic(path, value);
}

/// Seconds elapsed, one decimal.
pub fn took(start: Instant) -> String {
    format!("{:.1}", start.elapsed().as_secs_f64())
}

fn find_slot<'a>(map: &'a mut TaskMap, cat: &str, page: i32) -> Option<&'a mut TaskParse> {
    map.get_mut(cat).and_then(|v| v.iter_mut().find(|x| x.page == page))
}

/// Resumable full crawl over every pending slot of a flat task map.
///
/// `delay_ms` is re-read before every page; `parse_page(cat, page, ct)` returns whether the
/// slot counts as a success for the cycle.
pub async fn flat_parse_all<D, F, Fut>(
    tracker: &'static str,
    tasks: &'static Mutex<TaskMap>,
    task_path: &'static str,
    lock: &'static ParseLock,
    delay_ms: D,
    ct: CancellationToken,
    parse_page: F,
) -> anyhow::Result<()>
where
    D: Fn() -> i32,
    F: Fn(String, i32, CancellationToken) -> Fut,
    Fut: Future<Output = Result<bool, Cancelled>>,
{
    let cycle_path = cycle::cycle_path_for_tracker(tracker);
    let run = async {
        let (cyc, pending) = {
            let mut g = tasks.lock();
            let (cyc, map_count, pending_count) = cycle::begin_flat_full_run(tracker, &mut g);
            parser_log::write(tracker, format!("ParseAllTask start {}", cycle::format_start_log(&cyc, pending_count, map_count)));
            let pending: Vec<(String, i32)> = g
                .iter()
                .flat_map(|(k, v)| v.iter().filter(|p| cycle::is_pending_in_cycle(p, &cyc)).map(move |p| (k.clone(), p.page)))
                .collect();
            (cyc, pending)
        };
        let total = pending.len() as i64;
        let mut done = 0i64;
        trackers::report_progress(tracker, "ParseAllTask", 0, total, None, None);

        for (cat, page) in pending {
            trackers::check(&ct)?;
            trackers::yield_to_hourly_parse_and_throttle(lock, tracker, delay_ms(), &ct).await?;

            let res = parse_page(cat.clone(), page, ct.clone()).await?;
            trackers::note_request(tracker);

            let mut g = tasks.lock();
            if let Some(slot) = find_slot(&mut g, &cat, page) {
                cycle::note_attempt(tracker, slot, &cyc, res);
            }
            done += 1;
            trackers::report_progress(tracker, "ParseAllTask", done, total, Some(&cat), Some(page));
            cycle::persist_after_page_if_needed(&cycle_path, Some(&cyc), task_path, &*g, done, total);
        }
        Ok::<(), anyhow::Error>(())
    };
    let r = run.await;
    persist(task_path, &*tasks.lock());
    r
}

/// First `pages` slots (by page) of every category; marks successful pages done in the
/// active cycle. Returns one `"{cat} - {page}"` line per success.
pub async fn flat_parse_latest<D, F, Fut>(
    tracker: &'static str,
    tasks: &'static Mutex<TaskMap>,
    task_path: &'static str,
    lock: &'static ParseLock,
    delay_ms: D,
    pages: i32,
    parse_page: F,
) -> String
where
    D: Fn() -> i32,
    F: Fn(String, i32, CancellationToken) -> Fut,
    Fut: Future<Output = Result<bool, Cancelled>>,
{
    let mut log = String::new();
    let ct = CancellationToken::new();
    let run = async {
        let sw = Instant::now();
        parser_log::write(tracker, format!("Starting ParseLatest pages={pages}"));

        let (cyc, plan) = {
            let mut g = tasks.lock();
            let cyc = cycle::load_flat_active_cycle(tracker, &mut g);
            let plan: Vec<(String, Vec<i32>)> = g
                .iter()
                .map(|(k, v)| {
                    let mut ps: Vec<i32> = v.iter().map(|x| x.page).collect();
                    ps.sort();
                    ps.truncate(pages.max(0) as usize);
                    (k.clone(), ps)
                })
                .collect();
            (cyc, plan)
        };

        for (cat, ps) in plan {
            for page in ps {
                trackers::yield_to_hourly_parse_and_throttle(lock, tracker, delay_ms(), &ct).await?;
                let res = parse_page(cat.clone(), page, ct.clone()).await?;
                trackers::note_request(tracker);
                if res {
                    if let Some(slot) = find_slot(&mut tasks.lock(), &cat, page) {
                        cycle::mark_done_in_cycle(slot, &cyc);
                    }
                    log.push_str(&format!("{cat} - {page}\n"));
                }
            }
        }

        persist(task_path, &*tasks.lock());
        cycle::save_state(&cycle::cycle_path_for_tracker(tracker), &cyc);
        parser_log::write(tracker, format!("ParseLatest completed successfully (took {}s)", took(sw)));
        Ok::<(), Cancelled>(())
    };
    if let Err(e) = run.await {
        parser_log::write(tracker, format!("ParseLatest Error: {e}"));
    }
    log
}

// ---------------------------------------------------------------------------
// FileDB
// ---------------------------------------------------------------------------

/// Upsert with an async predicate that owns the item: the future gets the row and the
/// cached row with the same url (if any) and returns `Some(row)` to save it (possibly
/// modified, e.g. with a magnet filled in) or `None` to skip it.
pub async fn add_or_update_async<T, F, Fut>(torrents: Vec<T>, mut pred: F)
where
    T: AsRef<TorrentDetails>,
    F: FnMut(T, Option<TorrentDetails>) -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let mut groups: IndexMap<String, Vec<T>> = IndexMap::new();
    for t in torrents {
        let key = {
            let r = t.as_ref();
            fdb::key_db(&r.name, &r.originalname)
        };
        groups.entry(key).or_default().push(t);
    }
    for (key, list) in groups {
        let w = fdb::open_write(&key);
        for t in list {
            let url = t.as_ref().url.clone();
            let cached = w.with_db(|db| db.get(&url).cloned());
            if let Some(t) = pred(t, cached).await {
                w.add_or_update(t.as_ref());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cloudflare session recycle hook
// ---------------------------------------------------------------------------

type RecycleFn = Arc<dyn Fn(String) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

static RECYCLE_SESSION: OnceCell<RecycleFn> = OnceCell::new();

/// Install the browser-session recycler (destroy + recreate the FlareSolverr session for a host).
/// Without a hook, recycling is a no-op.
pub fn set_recycle_session_hook(f: RecycleFn) {
    let _ = RECYCLE_SESSION.set(f);
}

pub async fn recycle_session(host: &str) {
    if let Some(f) = RECYCLE_SESSION.get() {
        f(host.to_string()).await;
    }
}

// ---------------------------------------------------------------------------
// HTTP query helpers
// ---------------------------------------------------------------------------

/// Lenient integer query value: missing or unparsable → `default`.
pub fn q_i32(q: &HashMap<String, String>, name: &str, default: i32) -> i32 {
    q.get(name).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}
