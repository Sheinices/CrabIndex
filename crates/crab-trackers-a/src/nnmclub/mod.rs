//! nnmclub: `/forum/portal.php?c={cat}&start={page*25}` portal listings (cp1251) with magnets.

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

use crab_core::models::{TaskMap, TaskParse};
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::rx;
use crab_core::trackers::{self, Cancelled, LatestLock, ParseLock, WorkFlag};
use crab_core::{conf, fdb};

use crate::common::{self, q_i32};
use pagination::PageParseStatus;

pub const TRACKER_NAME: &str = "nnmclub";
const TASK_PARSE_PATH: &str = "Data/temp/nnmclub_taskParse.json";

/// Portal page size; the URL uses `start={page * PAGE_SIZE}`.
pub const PAGE_SIZE: i32 = 25;

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

fn persist_task_parse() {
    common::persist(TASK_PARSE_PATH, &*TASK_PARSE.lock());
}

async fn parse_page(cat: &str, page: i32, ct: &CancellationToken) -> Result<PageParseStatus, Cancelled> {
    let c = conf();
    let url = format!("{}/forum/portal.php?c={cat}&start={}", c.NNMClub.rq_host(), page * PAGE_SIZE);
    let html = net::get(&url, &Req::new().cp1251().useproxy(c.NNMClub.useproxy).cancel(ct)).await;
    trackers::check(ct)?;
    let Some(html) = html.filter(|h| h.contains("NNM-Club</title>")) else {
        return Ok(PageParseStatus::TransientError);
    };

    if pagination::is_portal_limit_faq(&html) {
        parser_log::write(TRACKER_NAME, format!("{cat}/{page} portal limit FAQ"));
        return Ok(PageParseStatus::PortalLimitFaq);
    }

    let torrents = parser::parse_torrents_from_page(&html, cat);
    fdb::add_or_update(&torrents);
    Ok(pagination::classify_page(Some(&html), torrents.len()))
}

async fn parse_page_settled(cat: String, page: i32, ct: CancellationToken) -> Result<bool, Cancelled> {
    Ok(pagination::should_settle_task(parse_page(&cat, page, &ct).await?))
}

pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = format!("{}/forum/portal.php", conf().NNMClub.rq_host());
        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, base: {base_url}"));
        let ct = CancellationToken::new();
        for cat in categories::ids() {
            parser_log::write(TRACKER_NAME, format!("Category {cat}: {base_url}?c={cat}&start={}", page * PAGE_SIZE));
            let _ = parse_page(cat, page, &ct).await;
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

/// Build the task map from each category pager, capped at the portal page window.
pub fn update_tasks_parse() -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, false, |ct| async move {
        for cat in categories::ids() {
            trackers::check(&ct)?;
            let c = conf();
            let url = format!("{}/forum/portal.php?c={cat}", c.NNMClub.rq_host());
            let html = net::get(&url, &Req::new().cp1251().timeout(10).useproxy(c.NNMClub.useproxy).cancel(&ct)).await;
            trackers::check(&ct)?;
            let Some(html) = html.filter(|h| h.contains("NNM-Club</title>")) else { continue };

            let maxpages = common::parse_int(&rx::group(
                &html,
                "<a href=\"[^\"]+\">([0-9]+)</a>[^<\n\r]+<a href=\"[^\"]+\">След.</a>",
                1,
            ));
            let task_count = pagination::clamp_task_page_count(maxpages);

            let mut g = TASK_PARSE.lock();
            let val = g.entry(cat.to_string()).or_default();
            let mut added = 0;
            for page in 0..task_count {
                if !val.iter().any(|i| i.page == page) {
                    val.push(TaskParse::new(page));
                    added += 1;
                }
            }
            let pruned = pagination::prune_tasks_beyond_portal_limit(val);
            parser_log::write(
                TRACKER_NAME,
                format!("UpdateTasksParse cat={cat}: pagerMax={maxpages}, taskCount={task_count}, added={added}, pruned={pruned}, total={}", val.len()),
            );
        }
        persist_task_parse();
        Ok(())
    })
}

pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, false, |ct| async move {
        common::flat_parse_all(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || conf().NNMClub.parse_delay(), ct, parse_page_settled).await
    })
}

pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, false, || {
        common::flat_parse_latest(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || conf().NNMClub.parse_delay(), pages, parse_page_settled)
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
        .route("/cron/nnmclub/parse", any(|Query(q): Query<HashMap<String, String>>| async move { parse(q_i32(&q, "page", 0)).await }))
        .route("/cron/nnmclub/updatetasksparse", any(|| async { update_tasks_parse() }))
        .route("/cron/nnmclub/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/nnmclub/parselatest", any(|Query(q): Query<HashMap<String, String>>| async move { parse_latest(q_i32(&q, "pages", 5)).await }))
}
