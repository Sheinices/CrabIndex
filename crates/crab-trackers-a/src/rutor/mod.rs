//! rutor: browse pages `/browse/{page}/{cat}/0/0` with magnets in the listing.

pub mod categories;
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
use crab_core::trackers::{self, Cancelled, LatestLock, ParseLock, WorkFlag};
use crab_core::{conf, fdb};

use crate::common::{self, q_i32};

pub const TRACKER_NAME: &str = "rutor";
const TASK_PARSE_PATH: &str = "Data/temp/rutor_taskParse.json";

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

fn persist_task_parse() {
    common::persist(TASK_PARSE_PATH, &*TASK_PARSE.lock());
}

fn browse_url(cat: &str, page: i32) -> String {
    format!("{}/browse/{page}/{cat}/0/0", conf().Rutor.rq_host())
}

async fn parse_page(cat: String, page: i32, ct: CancellationToken) -> Result<bool, Cancelled> {
    let html = net::get(&browse_url(&cat, page), &Req::new().useproxy(conf().Rutor.useproxy).cancel(&ct)).await;
    trackers::check(&ct)?;
    let Some(html) = html.filter(|h| parser::looks_like_browse_listing(h)) else { return Ok(false) };
    let torrents = parser::parse_torrents_from_page(&html, &cat);
    fdb::add_or_update(&torrents);
    Ok(true)
}

pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = format!("{}/browse", conf().Rutor.rq_host());
        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, base: {base_url}"));
        let ct = CancellationToken::new();
        for cat in categories::ids() {
            parser_log::write(TRACKER_NAME, format!("Category {cat}: {base_url}/{page}/{cat}/0/0"));
            let res = parse_page(cat.to_string(), page, ct.clone()).await.unwrap_or(false);
            log.push_str(&format!("{cat} - {page} / {}\n", if res { "True" } else { "False" }));
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

pub fn update_tasks_parse() -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, false, |ct| async move {
        for cat in categories::ids() {
            trackers::check(&ct)?;
            let url = format!("{}/browse/0/{cat}/0/0", conf().Rutor.rq_host());
            let Some(html) = net::get(&url, &Req::new().useproxy(conf().Rutor.useproxy).cancel(&ct)).await else {
                trackers::check(&ct)?;
                parser_log::write(TRACKER_NAME, format!("UpdateTasksParse cat={cat}: empty response"));
                continue;
            };
            let maxpages = parser::last_page_from_html(&html);

            let mut g = TASK_PARSE.lock();
            let val = g.entry(cat.to_string()).or_default();
            for page in 0..=maxpages {
                if !val.iter().any(|i| i.page == page) {
                    val.push(TaskParse::new(page));
                }
            }
            let pruned = parser::prune_pages_beyond_max(val, maxpages);
            parser_log::write(TRACKER_NAME, format!("UpdateTasksParse cat={cat}: maxPage={maxpages}, pruned={pruned}, total={}", val.len()));
        }
        persist_task_parse();
        Ok(())
    })
}

pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, false, |ct| async move {
        common::flat_parse_all(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || conf().Rutor.parse_delay(), ct, parse_page).await
    })
}

pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, false, || {
        common::flat_parse_latest(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || conf().Rutor.parse_delay(), pages, parse_page)
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
        .route("/cron/rutor/parse", any(|Query(q): Query<HashMap<String, String>>| async move { parse(q_i32(&q, "page", 0)).await }))
        .route("/cron/rutor/updatetasksparse", any(|| async { update_tasks_parse() }))
        .route("/cron/rutor/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/rutor/parselatest", any(|Query(q): Query<HashMap<String, String>>| async move { parse_latest(q_i32(&q, "pages", 5)).await }))
}
