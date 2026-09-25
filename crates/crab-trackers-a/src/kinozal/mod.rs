//! kinozal: `browse.php?c={cat}&page=N[&d={year}&t=1]` listings (cp1251, login cookie),
//! info hash from `get_srv_details.php`. Task map is nested: category → year arg → pages.

pub mod categories;
pub mod parser;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::any;
use axum::Router;
use chrono::Datelike;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crab_core::conf;
use crab_core::models::{NestedTaskMap, TaskParse};
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::rx;
use crab_core::trackers::{self, cycle, Cancelled, LatestLock, ParseLock, WorkFlag};

use crate::common::{self, q_i32};

pub const TRACKER_NAME: &str = "kinozal";
const TASK_PARSE_PATH: &str = "Data/temp/kinozal_taskParse.json";

const STALE_RECYCLE_AFTER: i32 = 3;
const STALE_RETRY_DELAY_MS: u64 = 3000;
const STALE_MAX_RETRIES: i32 = 3;
const UPDATE_TASKS_MAX_DURATION: Duration = Duration::from_secs(2 * 60 * 60);

static TASK_PARSE: Lazy<Mutex<NestedTaskMap>> =
    Lazy::new(|| Mutex::new(trackers::read_json_file::<NestedTaskMap>(TASK_PARSE_PATH).unwrap_or_default()));

static COOKIE: Mutex<Option<String>> = parking_lot::const_mutex(None);
static LAST_LOGIN_ERROR: Mutex<Option<String>> = parking_lot::const_mutex(None);
static LOGIN_SEMAPHORE: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));
static CONSECUTIVE_STALES: AtomicI32 = AtomicI32::new(0);

static PARSE_LOCK: ParseLock = ParseLock::new();
/// Shared by ParseAllTask and UpdateTasksParse (they never run together).
static CLUSTER: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

/// Url → torrent id for FileDB matching (`details.php?id=`, never `userdetails.php`).
pub fn url_id(url: &str) -> i32 {
    parser::try_get_details_id(url).unwrap_or(0)
}

fn persist_task_parse() {
    common::persist(TASK_PARSE_PATH, &*TASK_PARSE.lock());
}

fn tracker_host() -> String {
    url::Url::parse(&conf().Kinozal.host).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_else(|| "kinozal.guru".into())
}

fn note_valid_browse() {
    CONSECUTIVE_STALES.store(0, Ordering::SeqCst);
}

async fn note_stale_browse() {
    let n = CONSECUTIVE_STALES.fetch_add(1, Ordering::SeqCst) + 1;
    if n < STALE_RECYCLE_AFTER {
        return;
    }
    CONSECUTIVE_STALES.store(0, Ordering::SeqCst);
    parser_log::write(TRACKER_NAME, "recycle FlareSolverr session after consecutive stale shells");
    common::recycle_session(&tracker_host()).await;
}

fn cookie_header() -> Option<String> {
    if let Some(c) = conf().Kinozal.cookie_opt() {
        return Some(c.to_string());
    }
    COOKIE.lock().clone()
}

fn has_cookie() -> bool {
    cookie_header().map(|c| !c.trim().is_empty()).unwrap_or(false)
}

fn set_login_error(e: Option<String>) {
    *LAST_LOGIN_ERROR.lock() = e;
}

/// Value of cookie `name` from raw Set-Cookie lines (first line containing `name=`).
pub fn extract_cookie_value(lines: &[String], name: &str) -> Option<String> {
    let key = format!("{name}=");
    for line in lines {
        if line.trim().is_empty() || !line.contains(&key) {
            continue;
        }
        let start = line.find(&key)? + key.len();
        let v = rx::group(&line[start..], "([^;]+)(;|$)", 1);
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

/// `uid=…; pass=…;` from Set-Cookie lines: exact cookie names first, substring match as fallback.
pub fn cookie_from_set_cookies(lines: &[String]) -> Option<String> {
    let mut uid = None;
    let mut pass = None;
    for line in lines {
        let pair = line.split(';').next().unwrap_or("").trim();
        if let Some((k, v)) = pair.split_once('=') {
            match k.trim() {
                "uid" => uid = Some(v.trim().to_string()),
                "pass" => pass = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    let ok = |v: &Option<String>| v.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !ok(&uid) || !ok(&pass) {
        uid = extract_cookie_value(lines, "uid");
        pass = extract_cookie_value(lines, "pass");
    }
    if !ok(&uid) || !ok(&pass) {
        return None;
    }
    Some(format!("uid={}; pass={};", uid.unwrap_or_default(), pass.unwrap_or_default()))
}

async fn take_login() -> bool {
    if has_cookie() {
        return true;
    }
    let permit = match tokio::time::timeout(Duration::from_secs(15), LOGIN_SEMAPHORE.acquire()).await {
        Ok(Ok(p)) => p,
        _ => {
            set_login_error(Some("login wait timeout".into()));
            parser_log::write(TRACKER_NAME, "TakeLogin skipped: login semaphore timeout (15s)");
            return false;
        }
    };
    let _permit = permit;
    if has_cookie() {
        return true;
    }

    let c = conf();
    if c.Kinozal.login_u().trim().is_empty() || c.Kinozal.login_p().trim().is_empty() {
        let e = "credentials not configured (set Kinozal.login.u/p or Kinozal.cookie)".to_string();
        parser_log::write(TRACKER_NAME, format!("TakeLogin failed: {e}"));
        set_login_error(Some(e));
        return false;
    }
    let host = c.Kinozal.host.trim_end_matches('/').to_string();
    if host.trim().is_empty() {
        let e = "host is not configured".to_string();
        parser_log::write(TRACKER_NAME, format!("TakeLogin failed: {e}"));
        set_login_error(Some(e));
        return false;
    }

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(cl) => cl,
        Err(e) => {
            set_login_error(Some(e.to_string()));
            parser_log::write(TRACKER_NAME, format!("TakeLogin error: {e}"));
            return false;
        }
    };
    let form = [("username", c.Kinozal.login_u()), ("password", c.Kinozal.login_p()), ("returnto", "")];
    let resp = client
        .post(format!("{host}/takelogin.php"))
        .header("user-agent", net::http::USER_AGENT)
        .header("cache-control", "no-cache")
        .header("dnt", "1")
        .header("origin", host.as_str())
        .header("pragma", "no-cache")
        .header("referer", format!("{host}/"))
        .header("upgrade-insecure-requests", "1")
        .form(&form)
        .send()
        .await;
    match resp {
        Ok(resp) => {
            let lines: Vec<String> =
                resp.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(|s| s.to_string())).collect();
            if let Some(cookie) = cookie_from_set_cookies(&lines) {
                *COOKIE.lock() = Some(cookie);
                set_login_error(None);
                parser_log::write(TRACKER_NAME, "TakeLogin OK");
                return true;
            }
            let e = format!("no uid/pass cookies in response, status={}", resp.status().as_u16());
            parser_log::write(TRACKER_NAME, format!("TakeLogin failed: {e}"));
            set_login_error(Some(e));
        }
        Err(e) => {
            set_login_error(Some(e.to_string()));
            parser_log::write(TRACKER_NAME, format!("TakeLogin error: {e}"));
        }
    }
    false
}

async fn ensure_logged_in() -> bool {
    if has_cookie() {
        return true;
    }
    take_login().await
}

async fn get_browse_html(browse_url: &str, ct: &CancellationToken) -> Option<String> {
    let c = conf();
    let req = Req::new()
        .cp1251()
        .cookie_opt(cookie_header())
        .referer(format!("{}/", c.Kinozal.host))
        .useproxy(c.Kinozal.useproxy)
        .cancel(ct);
    net::get(browse_url, &req).await
}

async fn get_browse_html_retrying_transient(browse_url: &str, ct: &CancellationToken) -> Result<Option<String>, Cancelled> {
    let html = get_browse_html(browse_url, ct).await;
    trackers::check(ct)?;
    if !parser::is_transient_browse_failure(html.as_deref()) {
        return Ok(html);
    }
    trackers::sleep(1500, ct).await?;
    Ok(get_browse_html(browse_url, ct).await)
}

async fn parse_page(cat: &str, page: i32, arg: Option<&str>, ct: &CancellationToken) -> Result<bool, Cancelled> {
    if !ensure_logged_in().await {
        return Ok(false);
    }

    let browse_url = format!("{}/browse.php?c={cat}&page={page}{}", conf().Kinozal.host, arg.unwrap_or(""));
    let mut html = get_browse_html_retrying_transient(&browse_url, ct).await?;
    let diag = |h: &Option<String>| parser::format_browse_diag(h.as_deref());

    if parser::is_transient_browse_failure(html.as_deref()) {
        parser_log::write(TRACKER_NAME, format!("browse transient failure, skip login: {browse_url}"));
        return Ok(false);
    }

    let empty_here = |h: &Option<String>| parser::is_empty_search_result(h.as_deref()) && !parser::browse_filters_mismatch(h.as_deref(), cat, arg);
    if empty_here(&html) {
        // page=0 can be a leftover empty-search tab; year tails are really empty.
        if page == 0 {
            let mut retry = 0;
            while empty_here(&html) && retry < 2 {
                trackers::sleep(STALE_RETRY_DELAY_MS, ct).await?;
                html = get_browse_html(&browse_url, ct).await;
                retry += 1;
            }
        }
        if empty_here(&html) {
            parser_log::write(TRACKER_NAME, format!("browse empty search: {browse_url} {}", diag(&html)));
            note_valid_browse();
            return Ok(true);
        }
    }

    let mut retry = 0;
    while (parser::is_stale_listing_html(html.as_deref()) || parser::browse_filters_mismatch(html.as_deref(), cat, arg)) && retry < STALE_MAX_RETRIES {
        trackers::sleep(STALE_RETRY_DELAY_MS, ct).await?;
        html = get_browse_html(&browse_url, ct).await;
        retry += 1;
    }

    if parser::is_stale_listing_html(html.as_deref()) || parser::is_transient_browse_failure(html.as_deref()) {
        parser_log::write(TRACKER_NAME, format!("browse stale/empty shell: {browse_url} {}", diag(&html)));
        note_stale_browse().await;
        return Ok(false);
    }

    if parser::browse_filters_mismatch(html.as_deref(), cat, arg) {
        parser_log::write(TRACKER_NAME, format!("browse filter mismatch: {browse_url} {}", diag(&html)));
        note_stale_browse().await;
        return Ok(false);
    }

    if parser::is_empty_search_result(html.as_deref()) {
        parser_log::write(TRACKER_NAME, format!("browse empty search: {browse_url} {}", diag(&html)));
        note_valid_browse();
        return Ok(true);
    }

    note_valid_browse();

    let logged_in = html.as_deref().map(parser::is_logged_in).unwrap_or(false);
    if parser::is_login_wall(html.as_deref()) || (parser::is_valid_browse_page(html.as_deref()) && !logged_in) {
        *COOKIE.lock() = None;
        if !take_login().await {
            return Ok(false);
        }

        html = get_browse_html_retrying_transient(&browse_url, ct).await?;
        if parser::browse_filters_mismatch(html.as_deref(), cat, arg) {
            parser_log::write(TRACKER_NAME, format!("browse filter mismatch: {browse_url} {}", diag(&html)));
            note_stale_browse().await;
            return Ok(false);
        }
        if parser::is_empty_search_result(html.as_deref()) {
            parser_log::write(TRACKER_NAME, format!("browse empty search: {browse_url} {}", diag(&html)));
            note_valid_browse();
            return Ok(true);
        }
        let stale = parser::is_stale_listing_html(html.as_deref());
        if parser::is_transient_browse_failure(html.as_deref()) || stale || !parser::is_valid_browse_page(html.as_deref()) {
            if stale {
                note_stale_browse().await;
            }
            return Ok(false);
        }
    } else if !parser::is_valid_browse_page(html.as_deref()) {
        return Ok(false);
    }

    let html = html.unwrap_or_default();
    let torrents = parser::parse_torrents_from_page(&html, cat);
    let listing_href_count = parser::count_torrent_listing_links(Some(&html));
    if listing_href_count > 0 && torrents.is_empty() {
        parser_log::write(TRACKER_NAME, format!("parse yielded 0 from {listing_href_count} listing hrefs: {browse_url}"));
        return Ok(false);
    }

    let parsed_count = torrents.len();
    let resolved = Arc::new(AtomicUsize::new(0));
    common::add_or_update_async(torrents, |mut t, cached| {
        let resolved = resolved.clone();
        let ct = ct.clone();
        async move {
            if let Some(c) = cached {
                if parser::should_skip_hash_fetch(&c, &t) {
                    resolved.fetch_add(1, Ordering::SeqCst);
                    return Some(t);
                }
            }
            let id = parser::try_get_details_id(&t.url)?;
            let c = conf();
            let req = Req::new().cp1251().cookie_opt(cookie_header()).useproxy(c.Kinozal.useproxy).cancel(&ct);
            let srv_details = net::get(&format!("{}/get_srv_details.php?id={id}&action=2", c.Kinozal.host), &req).await;
            let hash = parser::parse_info_hash(srv_details.as_deref())?;
            if hash.trim().is_empty() {
                return None;
            }
            t.magnet = format!("magnet:?xt=urn:btih:{}", hash.to_uppercase());
            resolved.fetch_add(1, Ordering::SeqCst);
            Some(t)
        }
    })
    .await;
    trackers::check(ct)?;

    Ok(parser::should_mark_page_done(parsed_count, resolved.load(Ordering::SeqCst), listing_href_count))
}

pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let mut log = String::new();
        let sw = Instant::now();
        let base_url = format!("{}/browse.php", conf().Kinozal.host);
        parser_log::write(TRACKER_NAME, format!("Starting parse page={page}, base: {base_url}"));
        let ct = CancellationToken::new();
        for cat in categories::ids() {
            parser_log::write(TRACKER_NAME, format!("Category {cat}: {base_url}?c={cat}&page={page}"));
            let _ = parse_page(cat, page, None, &ct).await;
            trackers::note_request(TRACKER_NAME);
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

/// Build the nested task map: every category × year (current year down to 1990), pages
/// from the year listing pager. Runs in the background with a 2h wall clock.
pub async fn update_tasks_parse() -> String {
    if CLUSTER.is_busy() {
        trackers::log_parse_skipped(TRACKER_NAME, trackers::WORK_RESULT);
        parser_log::write(TRACKER_NAME, "UpdateTasksParse skipped: sibling job running");
        return trackers::WORK_RESULT.into();
    }

    if !ensure_logged_in().await {
        return match LAST_LOGIN_ERROR.lock().clone().filter(|e| !e.trim().is_empty()) {
            Some(e) => format!("login failed: {e}"),
            None => "login failed".into(),
        };
    }

    trackers::run_in_background(
        TRACKER_NAME,
        "UpdateTasksParse",
        &CLUSTER,
        false,
        |ct| async move {
            let delay_ms = parser::update_tasks_parse_delay_ms(conf().Kinozal.parse_delay());
            let mut pruned = 0;
            let this_year = chrono::Local::now().year();
            for cat in categories::ids() {
                for year in (1990..=this_year).rev() {
                    trackers::check(&ct)?;
                    let html = get_browse_html(&format!("{}/browse.php?c={cat}&d={year}&t=1", conf().Kinozal.host), &ct).await;
                    trackers::check(&ct)?;
                    if delay_ms > 0 {
                        trackers::sleep(delay_ms as u64, &ct).await?;
                    }

                    let arg = format!("&d={year}&t=1");
                    let mismatch = parser::browse_filters_mismatch(html.as_deref(), cat, Some(&arg));
                    if parser::is_stale_listing_html(html.as_deref()) || mismatch {
                        note_stale_browse().await;
                    } else if parser::is_valid_browse_page(html.as_deref()) {
                        note_valid_browse();
                    }
                    if mismatch || !parser::is_valid_browse_page(html.as_deref()) {
                        continue;
                    }

                    let page_count = parser::year_task_page_count(html.as_deref().unwrap_or(""));
                    let mut g = TASK_PARSE.lock();
                    let val = g.entry(cat.to_string()).or_default().entry(arg).or_default();
                    for page in 0..page_count {
                        if !val.iter().any(|i| i.page == page) {
                            val.push(TaskParse::new(page));
                        }
                    }
                    pruned += parser::prune_pages_beyond_year_count(val, page_count);
                }
            }
            persist_task_parse();
            if pruned > 0 {
                parser_log::write(TRACKER_NAME, format!("UpdateTasksParse pruned {pruned} empty year-tail pages"));
            }
            Ok(())
        },
        Some(UPDATE_TASKS_MAX_DURATION),
    )
}

fn find_slot<'a>(map: &'a mut NestedTaskMap, cat: &str, arg: &str, page: i32) -> Option<&'a mut TaskParse> {
    map.get_mut(cat)?.get_mut(arg)?.iter_mut().find(|x| x.page == page)
}

pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &CLUSTER, false, |ct| async move {
        let cycle_path = cycle::cycle_path_for_tracker(TRACKER_NAME);
        let run = async {
            let (cyc, pending) = {
                let mut g = TASK_PARSE.lock();
                let (cyc, map_count, pending_count) = cycle::begin_nested_full_run(TRACKER_NAME, &mut g);
                parser_log::write(TRACKER_NAME, format!("ParseAllTask start {}", cycle::format_start_log(&cyc, pending_count, map_count)));
                let mut pending: Vec<(String, String, i32)> = Vec::new();
                for (cat, args) in g.iter() {
                    for (arg, pages) in args {
                        for p in pages.iter().filter(|p| cycle::is_pending_in_cycle(p, &cyc)) {
                            pending.push((cat.clone(), arg.clone(), p.page));
                        }
                    }
                }
                (cyc, pending)
            };
            let total = pending.len() as i64;
            let mut attempted = 0i64;
            let mut succeeded = 0i64;
            trackers::report_progress(TRACKER_NAME, "ParseAllTask", 0, total, None, None);

            for (cat, arg, page) in pending {
                trackers::check(&ct)?;
                trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, conf().Kinozal.parse_delay(), &ct).await?;

                let res = parse_page(&cat, page, Some(&arg), &ct).await?;
                trackers::note_request(TRACKER_NAME);

                let mut g = TASK_PARSE.lock();
                if let Some(slot) = find_slot(&mut g, &cat, &arg, page) {
                    if cycle::note_attempt(TRACKER_NAME, slot, &cyc, res) {
                        succeeded += 1;
                    }
                }
                attempted += 1;
                trackers::report_progress(TRACKER_NAME, "ParseAllTask", attempted, total, Some(&cat), Some(page));
                cycle::persist_after_page_if_needed(&cycle_path, Some(&cyc), TASK_PARSE_PATH, &*g, attempted, total);
                if attempted == total || attempted % trackers::PERSIST_EVERY_PAGES == 0 {
                    parser_log::write(TRACKER_NAME, format!("ParseAllTask ok={succeeded}/{attempted}"));
                }
            }
            Ok::<(), anyhow::Error>(())
        };
        let r = run.await;
        persist_task_parse();
        r
    })
}

pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER_NAME, &PARSE_LATEST_LOCK, false, || async move {
        let mut log = String::new();
        let ct = CancellationToken::new();
        let run = async {
            let sw = Instant::now();
            parser_log::write(TRACKER_NAME, format!("Starting ParseLatest pages={pages}"));

            let (cyc, plan) = {
                let mut g = TASK_PARSE.lock();
                let cyc = cycle::load_nested_active_cycle(TRACKER_NAME, &mut g);
                let mut plan: Vec<(String, String, Vec<i32>)> = Vec::new();
                for (cat, args) in g.iter() {
                    for (arg, v) in args {
                        let mut ps: Vec<i32> = v.iter().map(|x| x.page).collect();
                        ps.sort();
                        ps.truncate(pages.max(0) as usize);
                        plan.push((cat.clone(), arg.clone(), ps));
                    }
                }
                (cyc, plan)
            };

            for (cat, arg, ps) in plan {
                for page in ps {
                    trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER_NAME, conf().Kinozal.parse_delay(), &ct).await?;
                    let res = parse_page(&cat, page, Some(&arg), &ct).await?;
                    trackers::note_request(TRACKER_NAME);
                    if res {
                        if let Some(slot) = find_slot(&mut TASK_PARSE.lock(), &cat, &arg, page) {
                            cycle::mark_done_in_cycle(slot, &cyc);
                        }
                        log.push_str(&format!("{cat} - {arg} - {page}\n"));
                    }
                }
            }

            persist_task_parse();
            cycle::save_state(&cycle::cycle_path_for_tracker(TRACKER_NAME), &cyc);
            parser_log::write(TRACKER_NAME, format!("ParseLatest completed successfully (took {}s)", common::took(sw)));
            Ok::<(), Cancelled>(())
        };
        if let Err(e) = run.await {
            parser_log::write(TRACKER_NAME, format!("ParseLatest Error: {e}"));
        }
        log
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
        .route("/cron/kinozal/parse", any(|Query(q): Query<HashMap<String, String>>| async move { parse(q_i32(&q, "page", 0)).await }))
        .route("/cron/kinozal/updatetasksparse", any(|| async { update_tasks_parse().await }))
        .route("/cron/kinozal/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/kinozal/parselatest", any(|Query(q): Query<HashMap<String, String>>| async move { parse_latest(q_i32(&q, "pages", 5)).await }))
}
