//! megapeer: `browse.php?cat=&page=` listings (cp1251), magnets from .torrent downloads.

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
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, LatestLock, ParseLock, WorkFlag};

use crate::common::{self, q_i32};

pub const TRACKER_NAME: &str = "megapeer";
const TASK_PARSE_PATH: &str = "Data/temp/megapeer_taskParse.json";

pub const CATEGORIES: [&str; 7] = ["80", "79", "6", "5", "55", "57", "76"];

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

fn persist_task_parse() {
    common::persist(TASK_PARSE_PATH, &*TASK_PARSE.lock());
}

pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, true, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = format!("{}/browse.php", conf().Megapeer.rq_host());
        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, base: {base_url}"));
        let ct = CancellationToken::new();
        for cat in CATEGORIES {
            parser_log::write(TRACKER_NAME, format!("Category {cat}: {base_url}?cat={cat}&page={page}"));
            let res = parser::parse_page(cat.to_string(), page, ct.clone()).await.unwrap_or(false);
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
    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, true, |ct| async move {
        for cat in CATEGORIES {
            trackers::check(&ct)?;
            let url = format!("{}/browse.php?cat={cat}", conf().Megapeer.rq_host());
            let Some(html) = parser::get_megapeer_browse_page(&url, cat, &ct).await? else { continue };
            let maxpages = parser::last_page_from_html(&html);

            let mut g = TASK_PARSE.lock();
            let val = g.entry(cat.to_string()).or_default();
            for page in 0..=maxpages {
                if !val.iter().any(|i| i.page == page) {
                    val.push(TaskParse::new(page));
                }
            }
            let pruned = parser::prune_pages_beyond_max(val, maxpages);
            if pruned > 0 {
                parser_log::write(TRACKER_NAME, format!("UpdateTasksParse cat={cat}: maxPage={maxpages}, pruned={pruned}, total={}", val.len()));
            }
        }
        persist_task_parse();
        Ok(())
    })
}

pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, true, |ct| async move {
        common::flat_parse_all(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || 0, ct, parser::parse_page).await
    })
}

pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, true, || {
        common::flat_parse_latest(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK, || 0, pages, parser::parse_page)
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
        .route("/cron/megapeer/parse", any(|Query(q): Query<HashMap<String, String>>| async move { parse(q_i32(&q, "page", 0)).await }))
        .route("/cron/megapeer/updatetasksparse", any(|| async { update_tasks_parse() }))
        .route("/cron/megapeer/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/megapeer/parselatest", any(|Query(q): Query<HashMap<String, String>>| async move { parse_latest(q_i32(&q, "pages", 5)).await }))
}
