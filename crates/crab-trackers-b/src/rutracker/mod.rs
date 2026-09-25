//! Rutracker: hourly first-page parse of quick forums, page-map rebuild, resumable ParseAll
//! and ParseLatest. Topic pages are fetched for magnets (Cloudflare fallback via core HTTP).

pub mod categories;
pub mod parser;

use async_trait::async_trait;
use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use crab_core::models::TaskMap;
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, cycle, Cancelled, LatestLock, ParseAllStarter, ParseLock, WorkFlag};
use crab_core::{conf, time};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::common;

pub const TRACKER_NAME: &str = "rutracker";
const TASK_PARSE_PATH: &str = "Data/temp/rutracker_taskParse.json";

fn cycle_path() -> String {
    cycle::cycle_path_for_tracker(TRACKER_NAME)
}

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

fn persist_task_parse() {
    let snap = TASK_PARSE.lock().clone();
    common::persist_task_map(TASK_PARSE_PATH, &snap);
}

/// ParseAll starter registered for resume/maintenance.
pub struct Starter;

#[async_trait]
impl ParseAllStarter for Starter {
    fn tracker_name(&self) -> &'static str {
        TRACKER_NAME
    }
    async fn parse_all_task(&self) -> String {
        parse_all_task(None, 0)
    }
}

/// Hourly parse: page `page` of every quick forum (or the `cat` filter).
pub async fn parse(page: i32, cat: Option<String>, max_topics: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = format!("{}/forum/viewforum.php", conf().Rutracker.rq_host());

        let cats: Vec<&'static str> = match cat.as_deref().filter(|c| !c.trim().is_empty()) {
            None => categories::quick_parse_ids(),
            Some(c) => {
                let filter: Vec<String> = common::split_csv(c).into_iter().map(|x| x.to_lowercase()).collect();
                let quick: Vec<&'static str> = categories::quick_parse_ids().into_iter().filter(|id| filter.iter().any(|f| f == id)).collect();
                if quick.is_empty() {
                    // Smoke runs may target any known forum id, not only quick ones.
                    categories::ids().into_iter().filter(|id| filter.iter().any(|f| f == id)).collect()
                } else {
                    quick
                }
            }
        };

        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, cats={}, maxTopics={max_topics}, base: {base_url}", cats.len()));
        let never = CancellationToken::new();
        let mut failed: Option<String> = None;
        for c in cats {
            let page_url = if page == 0 { format!("{base_url}?f={c}") } else { format!("{base_url}?f={c}&start={}", page * 50) };
            parser_log::write(TRACKER_NAME, format!("Category {c}: {page_url}"));
            match parse_page(c, page, &never, max_topics).await {
                Ok(result) => log.push_str(&format!("{c} - {page} - {}\n", common::bool_text(result))),
                Err(e) => {
                    failed = Some(e.to_string());
                    break;
                }
            }
        }
        match failed {
            None => parser_log::write(TRACKER_NAME, format!("Parse completed successfully (took {:.1}s)", sw.elapsed().as_secs_f64())),
            Some(e) => parser_log::write(TRACKER_NAME, format!("Error: {e}")),
        }

        if log.trim().is_empty() {
            "ok".into()
        } else {
            log
        }
    })
    .await
}

fn resolve_cat_filter(cat: Option<&str>) -> Option<Vec<String>> {
    let cat = cat.filter(|c| !c.trim().is_empty())?;
    let filter: Vec<String> = common::split_csv(cat).into_iter().map(|x| x.to_lowercase()).collect();
    let cats: Vec<String> = categories::ids().into_iter().filter(|id| filter.iter().any(|f| f == id)).map(|s| s.to_string()).collect();
    if cats.is_empty() {
        None
    } else {
        Some(cats)
    }
}

/// Rebuild the page-slot map from live forum page counts (background).
pub fn update_tasks_parse(cat: Option<String>) -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, false, move |ct| async move {
        let cat_filter = resolve_cat_filter(cat.as_deref());
        let cats: Vec<String> = cat_filter.clone().unwrap_or_else(|| categories::ids().into_iter().map(|s| s.to_string()).collect());
        parser_log::write(TRACKER_NAME, format!("UpdateTasksParse start cats={}", cats.len()));

        for c in &cats {
            trackers::check(&ct)?;
            let url = format!("{}/forum/viewforum.php?f={c}", conf().Rutracker.rq_host());
            let Some(html) = net::get(&url, &Req::new().useproxy(conf().Rutracker.useproxy).cancel(&ct)).await else { continue };

            let page_count = parser::effective_page_count(&html);
            if page_count < 1 {
                continue;
            }

            let mut map = TASK_PARSE.lock();
            let val = map.entry(c.clone()).or_default();
            common::ensure_pages(val, page_count);
            let pruned = parser::prune_pages_beyond_page_count(Some(val), page_count);
            if pruned > 0 {
                parser_log::write(TRACKER_NAME, format!("UpdateTasksParse cat={c}: pageCount={page_count}, pruned={pruned}, total={}", val.len()));
            }
        }

        if cat_filter.is_none() {
            let ids = categories::ids();
            TASK_PARSE.lock().retain(|k, _| ids.contains(&k.as_str()));
        }

        persist_task_parse();
        parser_log::write(TRACKER_NAME, format!("UpdateTasksParse done cats={}", cats.len()));
        Ok(())
    })
}

/// Full (or filtered) crawl of every slot in the page map (background, resumable cycle).
pub fn parse_all_task(cat: Option<String>, max_pages: i32) -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, false, move |ct| async move {
        let res = parse_all_inner(cat, max_pages, &ct).await;
        persist_task_parse();
        res
    })
}

async fn parse_all_inner(cat: Option<String>, max_pages: i32, ct: &CancellationToken) -> anyhow::Result<()> {
    let cat_filter = resolve_cat_filter(cat.as_deref());
    let full_run = cat_filter.is_none() && max_pages <= 0;

    let cycle_state = if full_run {
        let mut map = TASK_PARSE.lock();
        let (c, map_count, pending_count) = cycle::begin_flat_full_run(TRACKER_NAME, &mut map);
        parser_log::write(TRACKER_NAME, format!("ParseAllTask start {} maxPages={max_pages}", cycle::format_start_log(&c, pending_count, map_count)));
        Some(c)
    } else {
        parser_log::write(TRACKER_NAME, format!("ParseAllTask start partial cat={} maxPages={max_pages}", cat.as_deref().unwrap_or("*")));
        None
    };

    let mut pending: Vec<(String, i32)> = {
        let map = TASK_PARSE.lock();
        let today = time::today_local();
        map.iter()
            .filter(|(k, _)| cat_filter.as_ref().map(|f| f.contains(k)).unwrap_or(true))
            .flat_map(|(k, v)| {
                v.iter()
                    .filter(|p| match &cycle_state {
                        Some(c) => cycle::is_pending_in_cycle(p, c),
                        None => p.updateTime != today,
                    })
                    .map(|p| (k.clone(), p.page))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    if max_pages > 0 && pending.len() > max_pages as usize {
        pending.truncate(max_pages as usize);
    }

    let total = pending.len() as i64;
    let mut done: i64 = 0;
    trackers::report_progress(TRACKER_NAME, "ParseAllTask", 0, total, None, None);

    for (c, page) in pending {
        trackers::check(ct)?;
        trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, conf().Rutracker.parse_delay(), ct).await?;

        let res = parse_page(&c, page, ct, 0).await?;
        trackers::note_request(TRACKER_NAME);
        {
            let mut map = TASK_PARSE.lock();
            if let Some(slot) = map.get_mut(&c).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
                match &cycle_state {
                    Some(cy) => {
                        cycle::note_attempt(TRACKER_NAME, slot, cy, res);
                    }
                    None if res => slot.updateTime = time::today_local(),
                    None => {}
                }
            }
            done += 1;
            trackers::report_progress(TRACKER_NAME, "ParseAllTask", done, total, Some(&c), Some(page));
            if let Some(cy) = &cycle_state {
                cycle::persist_after_page_if_needed(&cycle_path(), Some(cy), TASK_PARSE_PATH, &*map, done, total);
            }
        }
    }

    parser_log::write(TRACKER_NAME, format!("ParseAllTask done {done}/{total}"));
    Ok(())
}

/// First `pages` slots of every forum in the page map.
pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        parser_log::write(TRACKER_NAME, format!("Starting ParseLatest pages={pages}"));

        let (cycle_state, work) = {
            let mut map = TASK_PARSE.lock();
            let c = cycle::load_flat_active_cycle(TRACKER_NAME, &mut map);
            let work: Vec<(String, Vec<i32>)> = map
                .iter()
                .map(|(k, v)| {
                    let mut ps: Vec<i32> = v.iter().map(|p| p.page).collect();
                    ps.sort();
                    ps.truncate(pages.max(0) as usize);
                    (k.clone(), ps)
                })
                .collect();
            (c, work)
        };

        let never = CancellationToken::new();
        let mut error: Option<String> = None;
        'outer: for (c, ps) in work {
            for page in ps {
                if let Err(e) = latest_step(&c, page, &never, &cycle_state, &mut log).await {
                    error = Some(e.to_string());
                    break 'outer;
                }
            }
        }

        match error {
            None => {
                persist_task_parse();
                cycle::save_state(&cycle_path(), &cycle_state);
                parser_log::write(TRACKER_NAME, format!("ParseLatest completed successfully (took {:.1}s)", sw.elapsed().as_secs_f64()));
            }
            Some(e) => parser_log::write(TRACKER_NAME, format!("ParseLatest Error: {e}")),
        }
        log
    })
    .await
}

async fn latest_step(c: &str, page: i32, ct: &CancellationToken, cy: &cycle::ParseAllCycleState, log: &mut String) -> Result<(), Cancelled> {
    trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, conf().Rutracker.parse_delay(), ct).await?;
    let res = parse_page(c, page, ct, 0).await?;
    trackers::note_request(TRACKER_NAME);
    if res {
        let mut map = TASK_PARSE.lock();
        if let Some(slot) = map.get_mut(c).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
            cycle::mark_done_in_cycle(slot, cy);
        }
        log.push_str(&format!("{c} - {page}\n"));
    }
    Ok(())
}

/// One forum listing page + topic pages for magnets. `Ok(false)` when the listing fetch failed.
async fn parse_page(cat: &str, page: i32, ct: &CancellationToken, max_topics: i32) -> Result<bool, Cancelled> {
    let c = conf();
    let start = if page == 0 { String::new() } else { format!("&start={}", page * 50) };
    let url = format!("{}/forum/viewforum.php?f={cat}{start}", c.Rutracker.rq_host());
    let req = Req::new().useproxy(c.Rutracker.useproxy).cancel(ct);
    let html = net::get(&url, &req).await;
    trackers::check(ct)?;
    let Some(html) = html.filter(|h| parser::looks_like_forum_listing(h)) else { return Ok(false) };

    let mut torrents = parser::parse_torrents_from_page(&html, cat);
    if max_topics > 0 && torrents.len() > max_topics as usize {
        torrents.truncate(max_topics as usize);
    }

    let topics_done = AtomicI32::new(0);
    let delay_ms = c.Rutracker.parse_delay();
    let topic_attempts = c.Rutracker.topicFetchAttempts.max(1);
    let td = &topics_done;
    let req = &req;

    common::add_or_update_async(torrents, common::by_url, |mut t, cached| async move {
        if max_topics > 0 && td.load(Ordering::SeqCst) >= max_topics {
            return Some(t);
        }
        if parser::should_skip_topic_fetch(cached.as_ref(), &t) {
            return Some(t);
        }
        for attempt in 1..=topic_attempts {
            if delay_ms > 0 && trackers::sleep(delay_ms as u64, ct).await.is_err() {
                return None;
            }
            let full_news = net::get(&conf().Rutracker.rq_host_uri(&t.url), req).await;
            if parser::apply_topic_page_details(&mut t, full_news.as_deref()) {
                td.fetch_add(1, Ordering::SeqCst);
                return Some(t);
            }
            if attempt < topic_attempts {
                let wait = if delay_ms > 0 { delay_ms as u64 } else { 1500 };
                if trackers::sleep(wait, ct).await.is_err() {
                    return None;
                }
            }
        }
        None
    })
    .await;

    trackers::check(ct)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    page: Option<String>,
    cat: Option<String>,
    maxtopics: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CatQ {
    cat: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseAllQ {
    cat: Option<String>,
    maxpages: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LatestQ {
    pages: Option<String>,
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(common::q_int(&q.page, 0), common::q_str(&q.cat), common::q_int(&q.maxtopics, 0)).await
}

async fn h_update_tasks_parse(Query(q): Query<CatQ>) -> String {
    update_tasks_parse(common::q_str(&q.cat))
}

async fn h_parse_all_task(Query(q): Query<ParseAllQ>) -> String {
    parse_all_task(common::q_str(&q.cat), common::q_int(&q.maxpages, 0))
}

async fn h_parse_latest(Query(q): Query<LatestQ>) -> String {
    parse_latest(common::q_int(&q.pages, 5)).await
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/rutracker/parse", get(h_parse).post(h_parse))
        .route("/cron/rutracker/updatetasksparse", get(h_update_tasks_parse).post(h_update_tasks_parse))
        .route("/cron/rutracker/parsealltask", get(h_parse_all_task).post(h_parse_all_task))
        .route("/cron/rutracker/parselatest", get(h_parse_latest).post(h_parse_latest))
}
