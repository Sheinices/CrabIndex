//! ultradox.vip sync: hourly parse, UpdateTasksParse, resumable ParseAllTask and ParseLatest.
//!
//! Nginx returns 503 unless the Referer looks like a search engine. Listing magnets are
//! empty, so every row needs a detail fetch for the real btih variants.

pub mod categories;
pub mod parser;

use std::sync::Arc;
use std::time::Instant;

use axum::extract::Query;
use axum::routing::any;
use axum::Router;
use crab_core::conf;
use crab_core::fdb;
use crab_core::models::{TaskMap, TaskParse, TorrentDetails};
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log as plog;
use crab_core::trackers::{self, cycle, Cancelled, LatestLock, ParseLock, WorkFlag};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::common::{self, kv, QueryMap};

pub const TRACKER_NAME: &str = parser::TRACKER_NAME;
const TASK_PARSE_PATH: &str = "Data/temp/ultradox_taskParse.json";

fn cycle_path() -> String {
    cycle::cycle_path_for_tracker(TRACKER_NAME)
}

fn browser_headers() -> Vec<(String, String)> {
    [
        ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
        ("Accept-Language", "ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7"),
        ("Cache-Control", "no-cache"),
        ("Pragma", "no-cache"),
        ("Sec-Fetch-Dest", "document"),
        ("Sec-Fetch-Mode", "navigate"),
        ("Sec-Fetch-Site", "cross-site"),
        ("Sec-Fetch-User", "?1"),
        ("Upgrade-Insecure-Requests", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

const CONFIG_MISSING: &str = "Config missing - add Ultradox.host";

fn persist_task_parse() {
    let snap = TASK_PARSE.lock().clone();
    let _ = cycle::write_json_atomic(TASK_PARSE_PATH, &snap);
}

fn host() -> String {
    conf().Ultradox.rq_host().trim_end_matches('/').to_string()
}

fn parse_delay() -> i32 {
    conf().Ultradox.parse_delay()
}

/// GET with a search-engine Referer and browser navigate headers (redirects are followed).
async fn fetch_page(url: &str, ct: &CancellationToken) -> Option<String> {
    let req = Req::new()
        .encoding(encoding_rs::UTF_8)
        .referer(parser::SEARCH_ENGINE_REFERER)
        .headers(browser_headers())
        .useproxy(conf().Ultradox.useproxy)
        .cancel(ct);
    net::get(url, &req).await
}

#[derive(Default, Clone, Copy)]
struct PageStats {
    fetched: i32,
    added: i32,
    updated: i32,
    skipped: i32,
    failed: i32,
    listing_ok: bool,
}

/// Parse one listing page of every section (page ≤ 0 = section root).
pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, true, || async move {
        let ct = trackers::app_stopping();
        let host = host();
        if host.trim().is_empty() {
            plog::write(TRACKER_NAME, CONFIG_MISSING);
            return "config missing".to_string();
        }

        let mut log = String::new();
        let sw = Instant::now();
        let (mut t_fetched, mut t_added, mut t_updated, mut t_skipped, mut t_failed) = (0, 0, 0, 0, 0);
        plog::write_kv(TRACKER_NAME, "Starting parse", &kv![("page", page), ("host", &host)]);

        let res: Result<(), Cancelled> = async {
            for (section, types) in categories::MAP.iter() {
                trackers::check(&ct)?;
                if parse_delay() > 0 {
                    trackers::sleep(parse_delay() as u64, &ct).await?;
                }
                let st = parse_section_page(&host, section, types, page, &ct).await?;
                t_fetched += st.fetched;
                t_added += st.added;
                t_updated += st.updated;
                t_skipped += st.skipped;
                t_failed += st.failed;
                log.push_str(&format!("{section} - {page}\n"));
                plog::write_kv(
                    TRACKER_NAME,
                    "Section page done",
                    &kv![
                        ("section", section),
                        ("page", page),
                        ("fetched", st.fetched),
                        ("added", st.added),
                        ("skipped", st.skipped),
                        ("failed", st.failed)
                    ],
                );
            }
            Ok(())
        }
        .await;

        if res.is_ok() {
            plog::write_kv(
                TRACKER_NAME,
                &format!("Parse completed successfully (took {}s)", common::secs(sw)),
                &kv![("fetched", t_fetched), ("added", t_added), ("updated", t_updated), ("skipped", t_skipped), ("failed", t_failed)],
            );
        }

        if log.is_empty() {
            "ok".to_string()
        } else {
            log
        }
    })
    .await
}

/// Refresh the page map from each section's pager (background, returns immediately).
pub fn update_tasks_parse() -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, true, |ct| async move {
        let host = host();
        if host.trim().is_empty() {
            plog::write(TRACKER_NAME, CONFIG_MISSING);
            return Ok(());
        }
        for section in categories::ids() {
            trackers::check(&ct)?;
            let html = fetch_page(&parser::listing_url(&host, section, 0), &ct).await;
            let Some(html) = html.filter(|h| !h.is_empty()) else {
                trackers::check(&ct)?;
                plog::write(TRACKER_NAME, format!("UpdateTasksParse {section}: empty response"));
                continue;
            };
            let max_page = parser::last_page_from_html(&html, Some(section));
            let pruned = merge_section_pages(section, max_page);
            let total = TASK_PARSE.lock().get(section).map(|v| v.len()).unwrap_or(0);
            let tail = if pruned > 0 { format!(", pruned={pruned}") } else { String::new() };
            plog::write(TRACKER_NAME, format!("UpdateTasksParse {section}: maxPage={max_page}, total={total}{tail}"));
        }
        persist_task_parse();
        Ok(())
    })
}

/// Resumable full crawl of every mapped page (background, returns immediately).
pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, true, |ct| async move {
        let host = host();
        if host.trim().is_empty() {
            plog::write(TRACKER_NAME, CONFIG_MISSING);
            return Ok(());
        }
        if TASK_PARSE.lock().is_empty() {
            rebuild_tasks(&host, &ct).await?;
        }
        let res = parse_all_inner(&host, &ct).await;
        persist_task_parse();
        res.map_err(Into::into)
    })
}

async fn parse_all_inner(host: &str, ct: &CancellationToken) -> Result<(), Cancelled> {
    let (cycle, pending) = {
        let mut map = TASK_PARSE.lock();
        let (cycle, map_count, pending_count) = cycle::begin_flat_full_run(TRACKER_NAME, &mut map);
        plog::write(TRACKER_NAME, format!("ParseAllTask start {}", cycle::format_start_log(&cycle, pending_count, map_count)));
        let pending: Vec<(String, i32)> = map
            .iter()
            .flat_map(|(cat, pages)| {
                pages.iter().filter(|p| cycle::is_pending_in_cycle(p, &cycle)).map(move |p| (cat.clone(), p.page))
            })
            .collect();
        (cycle, pending)
    };

    let total = pending.len() as i64;
    let mut done: i64 = 0;
    trackers::report_progress(TRACKER_NAME, "ParseAllTask", 0, total, None, None);

    for (cat, page) in pending {
        trackers::check(ct)?;
        trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, parse_delay(), ct).await?;

        let Some(types) = categories::types(&cat) else { continue };
        let st = parse_section_page(host, &cat, types, page, ct).await?;
        {
            let mut map = TASK_PARSE.lock();
            if let Some(slot) = map.get_mut(&cat).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
                cycle::note_attempt(TRACKER_NAME, slot, &cycle, st.listing_ok);
            }
        }

        trackers::note_request(TRACKER_NAME);
        done += 1;
        trackers::report_progress(TRACKER_NAME, "ParseAllTask", done, total, Some(&cat), Some(page));
        let snap = TASK_PARSE.lock().clone();
        cycle::persist_after_page_if_needed(&cycle_path(), Some(&cycle), TASK_PARSE_PATH, &snap, done, total);
    }
    Ok(())
}

/// Cheap daily pass: first `pages` pages of every section from the task map.
pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, true, || async move {
        let ct = trackers::app_stopping();
        let host = host();
        if host.trim().is_empty() {
            plog::write(TRACKER_NAME, CONFIG_MISSING);
            return "config missing".to_string();
        }
        let pages = if pages <= 0 { 5 } else { pages };

        if TASK_PARSE.lock().is_empty() && rebuild_tasks(&host, &ct).await.is_err() {
            return "ok".to_string();
        }

        let mut log = String::new();
        let sw = Instant::now();
        plog::write(TRACKER_NAME, format!("Starting ParseLatest pages={pages}"));

        let (cycle, work) = {
            let mut map = TASK_PARSE.lock();
            let cycle = cycle::load_flat_active_cycle(TRACKER_NAME, &mut map);
            let work: Vec<(String, Vec<i32>)> = map
                .iter()
                .map(|(cat, list)| {
                    let mut ps: Vec<i32> = list.iter().map(|p| p.page).collect();
                    ps.sort();
                    ps.truncate(pages as usize);
                    (cat.clone(), ps)
                })
                .collect();
            (cycle, work)
        };

        let res: Result<(), Cancelled> = async {
            for (cat, page_list) in &work {
                let Some(types) = categories::types(cat) else { continue };
                for &page in page_list {
                    trackers::check(&ct)?;
                    trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, parse_delay(), &ct).await?;
                    let st = parse_section_page(&host, cat, types, page, &ct).await?;
                    if st.listing_ok {
                        let mut map = TASK_PARSE.lock();
                        if let Some(slot) = map.get_mut(cat).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
                            cycle::mark_done_in_cycle(slot, &cycle);
                        }
                        log.push_str(&format!("{cat} - {page}\n"));
                    }
                    trackers::note_request(TRACKER_NAME);
                }
            }
            Ok(())
        }
        .await;

        match res {
            Ok(()) => {
                persist_task_parse();
                cycle::save_state(&cycle_path(), &cycle);
                plog::write(TRACKER_NAME, format!("ParseLatest completed successfully (took {}s)", common::secs(sw)));
            }
            Err(e) => plog::write(TRACKER_NAME, format!("ParseLatest Error: {e}")),
        }

        if log.is_empty() {
            "ok".to_string()
        } else {
            log
        }
    })
    .await
}

async fn rebuild_tasks(host: &str, ct: &CancellationToken) -> Result<(), Cancelled> {
    for section in categories::ids() {
        trackers::check(ct)?;
        let Some(html) = fetch_page(&parser::listing_url(host, section, 0), ct).await.filter(|h| !h.is_empty()) else {
            continue;
        };
        let max_page = parser::last_page_from_html(&html, Some(section));
        merge_section_pages(section, max_page);
    }
    persist_task_parse();
    Ok(())
}

/// Ensure slots 1..=max_page exist for `section`, prune the ghost tail, keep pages sorted.
fn merge_section_pages(section: &str, max_page: i32) -> i32 {
    let max_page = max_page.max(1);
    let mut map = TASK_PARSE.lock();
    let val = map.entry(section.to_string()).or_default();
    for page in 1..=max_page {
        if !val.iter().any(|p| p.page == page) {
            val.push(TaskParse::new(page));
        }
    }
    let pruned = parser::prune_pages_beyond_max(Some(val), max_page);
    val.sort_by_key(|p| p.page);
    pruned
}

async fn parse_section_page(host: &str, section: &str, types: &[&str], page: i32, ct: &CancellationToken) -> Result<PageStats, Cancelled> {
    let list_url = parser::listing_url(host, section, page);
    let list_html = fetch_page(&list_url, ct).await;
    trackers::check(ct)?;
    let Some(list_html) = list_html.filter(|h| !h.is_empty()) else {
        plog::write_kv(TRACKER_NAME, "Listing fetch failed", &kv![("section", section), ("page", page), ("url", &list_url)]);
        return Ok(PageStats::default());
    };

    let items = parser::parse_listing_html(&list_html);
    if items.is_empty() {
        return Ok(PageStats { listing_ok: true, ..Default::default() });
    }

    let mut torrents: Vec<TorrentDetails> = Vec::new();
    for item in &items {
        trackers::check(ct)?;
        let mut detail_url = item.detail_url.clone();
        if !detail_url.to_ascii_lowercase().starts_with("http") {
            detail_url = format!("{host}/{}", detail_url.trim_start_matches('/'));
        }
        if parse_delay() > 0 {
            trackers::sleep(parse_delay() as u64, ct).await?;
        }
        let detail_html = fetch_page(&detail_url, ct).await;
        trackers::check(ct)?;
        let Some((variants, info)) = detail_html.as_deref().filter(|h| !h.is_empty()).and_then(parser::try_parse_detail_html) else {
            continue;
        };
        for v in &variants {
            if let Some(rec) = parser::build_torrent(host, section, types, item, v, Some(&info)) {
                torrents.push(rec);
            }
        }
    }

    let mut st = save_torrents(torrents);
    st.listing_ok = true;
    Ok(st)
}

fn save_torrents(torrents: Vec<TorrentDetails>) -> PageStats {
    let mut st = PageStats::default();
    if torrents.is_empty() {
        return st;
    }
    let torrents: Vec<TorrentDetails> =
        torrents.into_iter().filter(|t| !t.name.trim().is_empty() && !t.magnet.trim().is_empty()).collect();
    st.fetched = torrents.len() as i32;
    if st.fetched == 0 {
        return st;
    }

    for (key, list) in common::group_by_bucket(torrents) {
        let w = fdb::open_write(&key);
        for t in list {
            let cached = common::cached_row(&w, &t.url);
            let need_write = match &cached {
                None => true,
                Some(c) => c.title.trim() != t.title.trim() || c.magnet.trim().is_empty() || !c.magnet.eq_ignore_ascii_case(&t.magnet),
            };
            if !need_write {
                st.skipped += 1;
                if let Some(c) = &cached {
                    plog::write_skipped(TRACKER_NAME, c, Some("no changes"));
                }
                continue;
            }
            if t.magnet.trim().is_empty() {
                st.failed += 1;
                plog::write_failed(TRACKER_NAME, &t, Some("empty magnet"));
                continue;
            }
            if cached.is_some() {
                st.updated += 1;
                plog::write_updated(TRACKER_NAME, &t, Some("magnet/title updated"));
            } else {
                st.added += 1;
                plog::write_added(TRACKER_NAME, &t);
            }
            w.add_or_update(&t);
        }
    }
    st
}

/// Registers `ultradox` as a resumable ParseAllTask tracker.
pub struct UltradoxParseAllStarter;

#[async_trait::async_trait]
impl trackers::ParseAllStarter for UltradoxParseAllStarter {
    fn tracker_name(&self) -> &'static str {
        TRACKER_NAME
    }
    async fn parse_all_task(&self) -> String {
        parse_all_task()
    }
}

pub fn starter() -> Arc<dyn trackers::ParseAllStarter> {
    Arc::new(UltradoxParseAllStarter)
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/ultradox/parse", any(|Query(q): Query<QueryMap>| async move { parse(common::q_i32(&q, "page", 0)).await }))
        .route("/cron/ultradox/updatetasksparse", any(|| async { update_tasks_parse() }))
        .route("/cron/ultradox/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/ultradox/parselatest", any(|Query(q): Query<QueryMap>| async move { parse_latest(common::q_i32(&q, "pages", 5)).await }))
}
