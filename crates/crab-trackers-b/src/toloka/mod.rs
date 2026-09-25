//! Toloka: login-cookie flow, hourly parse, page-map rebuild, resumable ParseAll and ParseLatest.
//! Magnets are built from the downloaded .torrent.

pub mod parser;

use async_trait::async_trait;
use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use crab_core::models::TaskMap;
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, cycle, Cancelled, LatestLock, ParseAllStarter, ParseLock, WorkFlag};
use crab_core::{conf, rx, util};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::common;

pub const TRACKER_NAME: &str = "toloka";
const TASK_PARSE_PATH: &str = "Data/temp/toloka_taskParse.json";
const COOKIE_KEY: &str = "cron:TolokaController:Cookie";
const AUTH_KEY: &str = "toloka:TakeLogin()";
const LOGIN_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/75.0.3770.100 Safari/537.36";

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
        parse_all_task()
    }
}

fn cookie() -> Option<String> {
    common::cache_get(COOKIE_KEY)
}

async fn take_login() -> bool {
    let c = conf();
    let Some(client) = common::login_client(10) else { return false };
    let form = [
        ("username", c.Toloka.login.u.clone().unwrap_or_default()),
        ("password", c.Toloka.login.p.clone().unwrap_or_default()),
        ("autologin", "on".into()),
        ("ssl", "on".into()),
        ("redirect", "index.php?".into()),
        ("login", "Вхід".into()),
    ];
    let Ok(resp) = client.post(format!("{}/login.php", c.Toloka.host)).header("user-agent", LOGIN_UA).form(&form).send().await else {
        return false;
    };
    let (mut sid, mut data) = (String::new(), String::new());
    for line in common::set_cookies(resp.headers()) {
        if util::is_blank(&line) {
            continue;
        }
        if line.contains("toloka_sid=") {
            sid = rx::group(&line, "toloka_sid=([^;]+)(;|$)", 1);
        }
        if line.contains("toloka_data=") {
            data = rx::group(&line, "toloka_data=([^;]+)(;|$)", 1);
        }
    }
    if !util::is_blank(&sid) && !util::is_blank(&data) {
        common::cache_set(COOKIE_KEY, format!("toloka_sid={sid}; toloka_ssl=1; toloka_data={data};"), Duration::from_secs(3600));
        return true;
    }
    false
}

/// Ensure a login cookie exists; false when login failed or is backing off.
async fn ensure_login() -> bool {
    if cookie().is_some() {
        return true;
    }
    if common::cache_has(AUTH_KEY) {
        return false;
    }
    if !take_login().await {
        common::cache_set(AUTH_KEY, "0", Duration::from_secs(5 * 60));
        return false;
    }
    true
}

pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = conf().Toloka.host.clone();
        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, base: {base_url}"));
        let never = CancellationToken::new();
        for cat in ["16", "96", "19", "139", "32", "173", "174", "44"] {
            let page_url = if page == 0 { format!("{base_url}/f{cat}") } else { format!("{base_url}/f{cat}-{}?sort=8", page * 45) };
            parser_log::write(TRACKER_NAME, format!("Category {cat}: {page_url}"));
            let _ = parse_page(cat, page, &never).await;
            log.push_str(&format!("{cat} - {page}\n"));
        }
        parser_log::write(TRACKER_NAME, format!("Parse completed successfully (took {:.1}s)", sw.elapsed().as_secs_f64()));
        if log.trim().is_empty() {
            "ok".into()
        } else {
            log
        }
    })
    .await
}

const UPDATE_CATS: [&str; 22] = [
    // Українське озвучення
    "16", "32", "19", "44", "127",
    // Українське кіно
    "84", "42", "124", "125",
    // HD українською
    "96", "173", "139", "174", "140",
    // Документальні фільми українською
    "12", "131", "230", "226", "227", "228", "229",
    // Телевізійні шоу та програми
    "132",
];

pub async fn update_tasks_parse() -> String {
    if !ensure_login().await {
        return "TakeLogin == null".into();
    }

    trackers::run_update_tasks_parse_in_background(TRACKER_NAME, &UPDATE_TASKS_WORK, false, |ct| async move {
        for cat in UPDATE_CATS {
            trackers::check(&ct)?;
            let req = Req::new().timeout(10).cookie_opt(cookie()).cancel(&ct);
            let Some(html) = net::get(&format!("{}/f{cat}", conf().Toloka.host), &req).await else { continue };

            let page_count = parser::last_page_from_html(&html).max(1);
            let mut map = TASK_PARSE.lock();
            let val = map.entry(cat.to_string()).or_default();
            common::ensure_pages(val, page_count);
            let pruned = parser::prune_pages_beyond_page_count(Some(val), page_count);
            if pruned > 0 {
                parser_log::write(TRACKER_NAME, format!("UpdateTasksParse cat={cat}: pageCount={page_count}, pruned={pruned}, total={}", val.len()));
            }
        }
        persist_task_parse();
        Ok(())
    })
}

pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, false, |ct| async move {
        let res = parse_all_inner(&ct).await;
        persist_task_parse();
        res
    })
}

async fn parse_all_inner(ct: &CancellationToken) -> anyhow::Result<()> {
    let (cy, pending) = {
        let mut map = TASK_PARSE.lock();
        let (cy, map_count, pending_count) = cycle::begin_flat_full_run(TRACKER_NAME, &mut map);
        parser_log::write(TRACKER_NAME, format!("ParseAllTask start {}", cycle::format_start_log(&cy, pending_count, map_count)));
        let pending: Vec<(String, i32)> = map
            .iter()
            .flat_map(|(k, v)| v.iter().filter(|p| cycle::is_pending_in_cycle(p, &cy)).map(|p| (k.clone(), p.page)).collect::<Vec<_>>())
            .collect();
        (cy, pending)
    };

    let total = pending.len() as i64;
    let mut done: i64 = 0;
    trackers::report_progress(TRACKER_NAME, "ParseAllTask", 0, total, None, None);

    for (c, page) in pending {
        trackers::check(ct)?;
        trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, conf().Toloka.parse_delay(), ct).await?;
        let res = parse_page(&c, page, ct).await?;
        trackers::note_request(TRACKER_NAME);

        let mut map = TASK_PARSE.lock();
        if let Some(slot) = map.get_mut(&c).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
            cycle::note_attempt(TRACKER_NAME, slot, &cy, res);
        }
        done += 1;
        trackers::report_progress(TRACKER_NAME, "ParseAllTask", done, total, Some(&c), Some(page));
        cycle::persist_after_page_if_needed(&cycle_path(), Some(&cy), TASK_PARSE_PATH, &*map, done, total);
    }
    Ok(())
}

pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        parser_log::write(TRACKER_NAME, format!("Starting ParseLatest pages={pages}"));

        let (cy, work) = {
            let mut map = TASK_PARSE.lock();
            let cy = cycle::load_flat_active_cycle(TRACKER_NAME, &mut map);
            let work: Vec<(String, Vec<i32>)> = map
                .iter()
                .map(|(k, v)| {
                    let mut ps: Vec<i32> = v.iter().map(|p| p.page).collect();
                    ps.sort();
                    ps.truncate(pages.max(0) as usize);
                    (k.clone(), ps)
                })
                .collect();
            (cy, work)
        };

        let never = CancellationToken::new();
        let mut error: Option<String> = None;
        'outer: for (c, ps) in work {
            for page in ps {
                if let Err(e) = latest_step(&c, page, &never, &cy, &mut log).await {
                    error = Some(e.to_string());
                    break 'outer;
                }
            }
        }
        match error {
            None => {
                persist_task_parse();
                cycle::save_state(&cycle_path(), &cy);
                parser_log::write(TRACKER_NAME, format!("ParseLatest completed successfully (took {:.1}s)", sw.elapsed().as_secs_f64()));
            }
            Some(e) => parser_log::write(TRACKER_NAME, format!("ParseLatest Error: {e}")),
        }
        log
    })
    .await
}

async fn latest_step(c: &str, page: i32, ct: &CancellationToken, cy: &cycle::ParseAllCycleState, log: &mut String) -> Result<(), Cancelled> {
    trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, conf().Toloka.parse_delay(), ct).await?;
    let res = parse_page(c, page, ct).await?;
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

async fn parse_page(cat: &str, page: i32, ct: &CancellationToken) -> Result<bool, Cancelled> {
    if !ensure_login().await {
        return Ok(false);
    }

    let host = conf().Toloka.host.clone();
    let suffix = if page == 0 { String::new() } else { format!("-{}", page * 45) };
    let html = net::get(&format!("{host}/f{cat}{suffix}?sort=8"), &Req::new().cookie_opt(cookie()).cancel(ct)).await;
    trackers::check(ct)?;
    let Some(html) = html.filter(|h| parser::looks_like_forum_listing(h)) else { return Ok(false) };

    let torrents = parser::parse_torrents_from_page(&html, cat);
    let host = &host;

    common::add_or_update_async(torrents, common::by_url, |mut t, cached| async move {
        if cached.map(|c| c.title == t.t.title).unwrap_or(false) {
            return Some(t);
        }
        let req = Req::new().timeout(30).cookie_opt(cookie()).referer(host.clone()).cancel(ct);
        let torrent = net::download(&format!("{host}/download.php?id={}", t.download_id), &req).await;
        let magnet = torrent.as_deref().and_then(bencode::magnet);
        match magnet {
            Some(m) => {
                t.t.magnet = m;
                Some(t)
            }
            None => None,
        }
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
struct PageQ {
    page: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LatestQ {
    pages: Option<String>,
}

async fn h_parse(Query(q): Query<PageQ>) -> String {
    parse(common::q_int(&q.page, 0)).await
}

async fn h_update_tasks_parse() -> String {
    update_tasks_parse().await
}

async fn h_parse_all_task() -> String {
    parse_all_task()
}

async fn h_parse_latest(Query(q): Query<LatestQ>) -> String {
    parse_latest(common::q_int(&q.pages, 5)).await
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/toloka/parse", get(h_parse).post(h_parse))
        .route("/cron/toloka/updatetasksparse", get(h_update_tasks_parse).post(h_update_tasks_parse))
        .route("/cron/toloka/parsealltask", get(h_parse_all_task).post(h_parse_all_task))
        .route("/cron/toloka/parselatest", get(h_parse_latest).post(h_parse_latest))
}
