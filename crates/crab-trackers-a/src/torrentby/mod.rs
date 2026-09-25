//! torrentby: `/{section}/?page=N` listings with magnets.

pub mod categories;
pub mod pagination;
pub mod parser;

use std::collections::HashMap;
use std::time::Instant;

use axum::extract::Query;
use axum::routing::any;
use axum::Router;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crab_core::conf;
use crab_core::models::{TaskMap, TaskParse};
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, Cancelled, LatestLock, ParseLock, WorkFlag};

use crate::common::{self, q_i32};

pub const TRACKER_NAME: &str = "torrentby";
const TASK_PARSE_PATH: &str = "Data/temp/torrentby_taskParse.json";

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

fn persist_task_parse() {
    common::persist(TASK_PARSE_PATH, &*TASK_PARSE.lock());
}

pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = conf().TorrentBy.rq_host();
        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, base: {base_url}"));
        let ct = CancellationToken::new();
        for cat in categories::ids() {
            parser_log::write(TRACKER_NAME, format!("Category {cat}: {base_url}/{cat}/?page={page}"));
            let _ = parser::parse_page(cat.to_string(), page, ct.clone()).await;
            log.push_str(&format!("{cat} - {page}\n"));
        }
        parser_log::write(TRACKER_NAME, format!("Parse completed successfully (took {}s)", common::took(sw)));
        if log.trim().is_empty() {
            "ok".into()
        } else {
            log
        }
    })
    .await
}

fn listing_url(host: &str, cat: &str, page: i32) -> String {
    if page <= 0 {
        format!("{host}/{cat}/")
    } else {
        format!("{host}/{cat}/?page={page}")
    }
}

/// Follow the pager (including `...` jumps) to find the last page, then binary-search
/// the empty tail. Returns `(last, claimed, ok)`.
async fn discover_last_page(host: &str, cat: &str, ct: &CancellationToken) -> Result<(i32, i32, bool), Cancelled> {
    let mut last = 0;
    let mut ok = false;
    let mut page = 0;
    for hop in 0..=pagination::MAX_ELLIPSIS_HOPS {
        trackers::check(ct)?;
        let delay = conf().TorrentBy.parse_delay();
        if hop > 0 && delay > 0 {
            trackers::sleep(delay as u64, ct).await?;
        }
        let req = Req::new().timeout(10).useproxy(conf().TorrentBy.useproxy).cancel(ct);
        let Some(html) = net::get(&listing_url(host, cat, page), &req).await else {
            trackers::check(ct)?;
            break;
        };
        ok = true;
        let pager = pagination::parse_pager(&html);
        if pager.max_page_index > last {
            last = pager.max_page_index;
        }
        let (true, Some(jump)) = (pager.has_trailing_ellipsis, pager.ellipsis_jump_page) else { break };
        if jump <= page {
            break;
        }
        page = jump;
    }

    let claimed = last;
    if ok && last > 0 {
        last = pagination::shrink_to_last_non_empty(
            last,
            |p| {
                let url = listing_url(host, cat, p);
                let req = Req::new().timeout(10).useproxy(conf().TorrentBy.useproxy).cancel(ct);
                async move {
                    let html = net::get(&url, &req).await?;
                    if !parser::is_listing_page(&html) {
                        return None;
                    }
                    Some(parser::has_listing_rows(&html))
                }
            },
            ct,
        )
        .await?;
    }
    Ok((last, claimed, ok))
}

pub fn update_tasks_parse() -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, false, |ct| async move {
        let host = conf().TorrentBy.rq_host().trim_end_matches('/').to_string();
        for cat in categories::ids() {
            trackers::check(&ct)?;
            let (last, claimed, ok) = discover_last_page(&host, cat, &ct).await?;
            if !ok {
                parser_log::write(TRACKER_NAME, format!("UpdateTasksParse cat={cat}: empty response"));
                continue;
            }

            let mut g = TASK_PARSE.lock();
            let val = g.entry(cat.to_string()).or_default();
            for page in 0..=last {
                if !val.iter().any(|i| i.page == page) {
                    val.push(TaskParse::new(page));
                }
            }
            let pruned = pagination::prune_pages_beyond_max(val, last);
            val.sort_by_key(|x| x.page);
            let mut msg = format!("UpdateTasksParse cat={cat}: maxPage={last}, total={}", val.len());
            if pruned > 0 {
                msg.push_str(&format!(", pruned={pruned}"));
            }
            if claimed > last {
                msg.push_str(&format!(", pagerWas={claimed}"));
            }
            parser_log::write(TRACKER_NAME, msg);
        }
        persist_task_parse();
        Ok(())
    })
}

pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, false, |ct| async move {
        common::flat_parse_all(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || conf().TorrentBy.parse_delay(), ct, parser::parse_page).await
    })
}

pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, false, || {
        common::flat_parse_latest(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || conf().TorrentBy.parse_delay(), pages, parser::parse_page)
    })
    .await
}

pub struct Starter;

#[async_trait::async_trait]
impl trackers::ParseAllStarter for Starter {
    fn tracker_name(&self) -> &'static str {
        TRACKER_NAME
    }
    async fn parse_all_task(&self) -> String {
        parse_all_task()
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/torrentby/parse", any(|Query(q): Query<HashMap<String, String>>| async move { parse(q_i32(&q, "page", 0)).await }))
        .route("/cron/torrentby/updatetasksparse", any(|| async { update_tasks_parse() }))
        .route("/cron/torrentby/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/torrentby/parselatest", any(|Query(q): Query<HashMap<String, String>>| async move { parse_latest(q_i32(&q, "pages", 5)).await }))
}
