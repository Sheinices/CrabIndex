//! korsars sync - login required (bb_data cookie). Listing pages carry inline magnets.
//! Requests use the alias host when set; FileDB urls stay on the canonical host.
//!
//! Routes: `/cron/korsars/{parse,updatetasksparse,parsealltask,parselatest}`.

pub mod categories;
pub mod parser;

use async_trait::async_trait;
use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crab_core::models::{TaskMap, TorrentDetails};
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, cycle, Cancelled, LatestLock, ParseAllStarter, ParseLock, WorkFlag};
use crab_core::{conf, fdb, util};

use crate::common::{self, bool_str, cached_row, group_by_key, log_kv, q_int, same_trimmed, secs_f1, Counts};
use parser::TRACKER_NAME as TRACKER;

const TASK_PARSE_PATH: &str = "Data/temp/korsars_taskParse.json";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const COOKIE_TTL: Duration = Duration::from_secs(24 * 3600);
const CONFIG_MISSING: &str = "Config missing - add Korsars.host";

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

/// Session cookie obtained by login (value, obtained at).
static DYN_COOKIE: Lazy<Mutex<Option<(String, Instant)>>> = Lazy::new(|| Mutex::new(None));

static PARSE_LOCK: ParseLock = ParseLock::new();
static PARSE_ALL_TASK_WORK: WorkFlag = WorkFlag::new();
static UPDATE_TASKS_WORK: WorkFlag = WorkFlag::new();
static PARSE_LATEST_LOCK: LatestLock = LatestLock::new();

fn cycle_path() -> String {
    cycle::cycle_path_for_tracker(TRACKER)
}

fn persist_task_parse() {
    let snap = TASK_PARSE.lock().clone();
    common::persist_task_map(TASK_PARSE_PATH, &snap);
}

/// Canonical host stored in FileDB urls.
fn canonical_host() -> String {
    common::trim_host(&conf().Korsars.host)
}

/// Request host - alias when set.
fn request_host() -> String {
    common::trim_host(&conf().Korsars.rq_host())
}

fn useproxy() -> bool {
    conf().Korsars.useproxy
}

fn parse_delay() -> i32 {
    conf().Korsars.parse_delay()
}

fn cookie_header() -> Option<String> {
    {
        let mut g = DYN_COOKIE.lock();
        match g.as_ref() {
            Some((c, at)) if at.elapsed() < COOKIE_TTL && !util::is_blank(c) => return Some(c.clone()),
            Some(_) => *g = None,
            None => {}
        }
    }
    conf().Korsars.cookie.as_deref().filter(|c| !util::is_blank(c)).map(|c| c.trim().to_string())
}

fn invalidate_cookie() {
    *DYN_COOKIE.lock() = None;
}

fn get_req(ct: &CancellationToken) -> Req {
    Req::new().encoding(encoding_rs::UTF_8).cookie_opt(cookie_header()).useproxy(useproxy()).cancel(ct)
}

async fn ensure_login(ct: &CancellationToken) -> Result<bool, Cancelled> {
    if cookie_header().is_some() {
        return Ok(true);
    }
    take_login(ct).await
}

async fn take_login(ct: &CancellationToken) -> Result<bool, Cancelled> {
    let host = request_host();
    let c = conf();
    let user = c.Korsars.login.u.as_deref().map(|u| u.trim().to_string()).unwrap_or_default();
    let pass = c.Korsars.login.p.clone().unwrap_or_default();
    if util::is_blank(&host) || util::is_blank(&user) {
        let cfg = crab_core::config::get_config_source_info();
        let reason = if util::is_blank(&host) && util::is_blank(&user) {
            "host empty and login.u empty"
        } else if util::is_blank(&host) {
            "host empty"
        } else {
            "login.u empty"
        };
        log_kv(
            TRACKER,
            &format!("Login skipped - {reason}"),
            &[
                ("configPath", cfg.path.unwrap_or_else(|| "(none)".into())),
                ("cwd", std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()),
                ("rawHost", c.Korsars.host.clone()),
                ("hasLoginU", bool_str(!util::is_blank(&user))),
            ],
        );
        return Ok(false);
    }

    log_kv(TRACKER, "Attempting login", &[("host", host.clone()), ("user", user.clone())]);

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            parser_log::write(TRACKER, format!("Login error: {e}"));
            return Ok(false);
        }
    };

    let form = [
        ("login_username", user.as_str()),
        ("login_password", pass.as_str()),
        ("autologin", "1"),
        ("login", "Вход"),
    ];
    let send = client
        .post(format!("{host}/login.php"))
        .header("User-Agent", USER_AGENT)
        .header("Referer", format!("{host}/"))
        .form(&form)
        .send();
    let resp = tokio::select! {
        _ = ct.cancelled() => return Err(Cancelled),
        r = send => r,
    };
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            parser_log::write(TRACKER, format!("Login error: {e}"));
            return Ok(false);
        }
    };
    parser_log::write(TRACKER, format!("Login response status={}", resp.status().as_u16()));

    let cookies: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|c| c.split(';').next().unwrap_or("").to_string())
        .collect();
    if cookies.is_empty() {
        parser_log::write(TRACKER, "Login FAILED - no Set-Cookie");
        return Ok(false);
    }
    let cookie_str = cookies.join("; ");
    if !cookie_str.contains("bb_data") {
        parser_log::write(TRACKER, "Login FAILED - no bb_data in cookies");
        return Ok(false);
    }
    *DYN_COOKIE.lock() = Some((cookie_str, Instant::now()));
    parser_log::write(TRACKER, "Login OK, got bb_data");
    Ok(true)
}

/// Parse one zero-based listing page of every category.
pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, true, || async move {
        let rq_host = request_host();
        let canon_host = canonical_host();
        if util::is_blank(&rq_host) || util::is_blank(&canon_host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return "config missing".to_string();
        }
        let ct = CancellationToken::new();
        if !ensure_login(&ct).await.unwrap_or(false) {
            return "login failed".to_string();
        }

        let mut log = String::new();
        let sw = Instant::now();
        let mut total = Counts::default();

        log_kv(
            TRACKER,
            "Starting parse",
            &[("page", page.to_string()), ("host", rq_host.clone()), ("categories", categories::count().to_string())],
        );

        for cat in categories::ids() {
            let delay = parse_delay();
            if delay > 0 && trackers::sleep(delay as u64, &ct).await.is_err() {
                return log;
            }
            let c = match parse_category_page(&rq_host, &canon_host, cat, page, &ct).await {
                Ok(c) => c,
                Err(_) => return log,
            };
            total.add(&c);
            log.push_str(&format!("{cat} - {page}\n"));
            log_kv(
                TRACKER,
                "Category page done",
                &[
                    ("f", cat.to_string()),
                    ("page", page.to_string()),
                    ("fetched", c.fetched.to_string()),
                    ("added", c.added.to_string()),
                    ("skipped", c.skipped.to_string()),
                    ("failed", c.failed.to_string()),
                ],
            );
        }

        log_kv(
            TRACKER,
            &format!("Parse completed successfully (took {}s)", secs_f1(sw)),
            &[
                ("fetched", total.fetched.to_string()),
                ("added", total.added.to_string()),
                ("updated", total.updated.to_string()),
                ("skipped", total.skipped.to_string()),
                ("failed", total.failed.to_string()),
            ],
        );

        if log.is_empty() {
            "ok".to_string()
        } else {
            log
        }
    })
    .await
}

/// Refresh the forum → pages map from each forum's first listing page (background).
pub fn update_tasks_parse() -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER, &UPDATE_TASKS_WORK, true, |ct| async move {
        let rq_host = request_host();
        if util::is_blank(&rq_host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return Ok(());
        }
        if !ensure_login(&ct).await? {
            parser_log::write(TRACKER, "UpdateTasksParse: login failed");
            return Ok(());
        }
        for cat in categories::ids() {
            trackers::check(&ct)?;
            let html = net::get(&parser::forum_url(&rq_host, cat, 0), &get_req(&ct)).await;
            let Some(html) = html.filter(|h| !h.is_empty()) else {
                parser_log::write(TRACKER, format!("UpdateTasksParse f={cat}: empty response"));
                continue;
            };
            if parser::looks_like_login_form(&html) {
                parser_log::write(TRACKER, format!("UpdateTasksParse f={cat}: login form - invalidating session"));
                invalidate_cookie();
                continue;
            }
            let max_page = parser::last_page_from_html(&html);
            let (pruned, total) = {
                let mut g = TASK_PARSE.lock();
                let pruned = common::merge_forum_pages(&mut g, cat, max_page);
                (pruned, g.get(cat).map(|v| v.len()).unwrap_or(0))
            };
            let tail = if pruned > 0 { format!(", pruned={pruned}") } else { String::new() };
            parser_log::write(TRACKER, format!("UpdateTasksParse f={cat}: maxPage={max_page}, total={total}{tail}"));
        }
        persist_task_parse();
        Ok(())
    })
}

/// Resumable full crawl of every mapped page (background).
pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER, &PARSE_ALL_TASK_WORK, true, |ct| async move {
        let rq_host = request_host();
        let canon_host = canonical_host();
        if util::is_blank(&rq_host) || util::is_blank(&canon_host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return Ok(());
        }
        if !ensure_login(&ct).await? {
            parser_log::write(TRACKER, "ParseAllTask: login failed");
            return Ok(());
        }
        if TASK_PARSE.lock().is_empty() {
            rebuild_tasks(&rq_host, &ct).await?;
        }
        let res = run_parse_all(&rq_host, &canon_host, &ct).await;
        persist_task_parse();
        res.map_err(Into::into)
    })
}

async fn run_parse_all(rq_host: &str, canon_host: &str, ct: &CancellationToken) -> Result<(), Cancelled> {
    let (cycle, pending) = {
        let mut g = TASK_PARSE.lock();
        let (cycle, map_count, pending_count) = cycle::begin_flat_full_run(TRACKER, &mut g);
        parser_log::write(TRACKER, format!("ParseAllTask start {}", cycle::format_start_log(&cycle, pending_count, map_count)));
        let pending: Vec<(String, i32)> = g
            .iter()
            .flat_map(|(k, v)| v.iter().filter(|p| cycle::is_pending_in_cycle(p, &cycle)).map(move |p| (k.clone(), p.page)))
            .collect();
        (cycle, pending)
    };
    let total = pending.len() as i64;
    let mut done: i64 = 0;
    trackers::report_progress(TRACKER, "ParseAllTask", 0, total, None, None);

    for (cat, page) in pending {
        trackers::check(ct)?;
        trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER, parse_delay(), ct).await?;

        parse_category_page(rq_host, canon_host, &cat, page, ct).await?;
        // Empty listings still count as done.
        {
            let mut g = TASK_PARSE.lock();
            if let Some(slot) = g.get_mut(&cat).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
                cycle::note_attempt(TRACKER, slot, &cycle, true);
            }
        }

        trackers::note_request(TRACKER);
        done += 1;
        trackers::report_progress(TRACKER, "ParseAllTask", done, total, Some(&cat), Some(page));
        if trackers::should_persist_checkpoint(done, total) {
            let snap = TASK_PARSE.lock().clone();
            cycle::persist_after_page(&cycle_path(), Some(&cycle), TASK_PARSE_PATH, &snap);
        }
    }
    Ok(())
}

/// Cheap daily pass: first `pages` pages of every category (from the task map).
pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER, &PARSE_LATEST_LOCK, true, || async move {
        let rq_host = request_host();
        let canon_host = canonical_host();
        if util::is_blank(&rq_host) || util::is_blank(&canon_host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return "config missing".to_string();
        }
        let ct = CancellationToken::new();
        if !ensure_login(&ct).await.unwrap_or(false) {
            return "login failed".to_string();
        }
        let pages = if pages <= 0 { 5 } else { pages };

        if TASK_PARSE.lock().is_empty() && rebuild_tasks(&rq_host, &ct).await.is_err() {
            return "ok".to_string();
        }

        let mut log = String::new();
        let sw = Instant::now();
        parser_log::write(TRACKER, format!("Starting ParseLatest pages={pages}"));

        let (cycle, work) = {
            let mut g = TASK_PARSE.lock();
            let cycle = cycle::load_flat_active_cycle(TRACKER, &mut g);
            let work: Vec<(String, Vec<i32>)> = g
                .iter()
                .map(|(k, v)| {
                    let mut p: Vec<i32> = v.iter().map(|x| x.page).collect();
                    p.sort();
                    p.truncate(pages as usize);
                    (k.clone(), p)
                })
                .collect();
            (cycle, work)
        };

        for (cat, list) in work {
            for page in list {
                if trackers::yield_to_hourly_parse_and_throttle(&PARSE_LOCK, TRACKER, parse_delay(), &ct).await.is_err() {
                    break;
                }
                match parse_category_page(&rq_host, &canon_host, &cat, page, &ct).await {
                    Ok(_) => {
                        {
                            let mut g = TASK_PARSE.lock();
                            if let Some(slot) = g.get_mut(&cat).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
                                cycle::mark_done_in_cycle(slot, &cycle);
                            }
                        }
                        log.push_str(&format!("{cat} - {page}\n"));
                    }
                    Err(e) => parser_log::write(TRACKER, format!("ParseLatest f={cat} page={page} error: {e}")),
                }
                trackers::note_request(TRACKER);
            }
        }

        persist_task_parse();
        cycle::save_state(&cycle_path(), &cycle);
        parser_log::write(TRACKER, format!("ParseLatest completed successfully (took {}s)", secs_f1(sw)));

        if log.is_empty() {
            "ok".to_string()
        } else {
            log
        }
    })
    .await
}

async fn rebuild_tasks(rq_host: &str, ct: &CancellationToken) -> Result<(), Cancelled> {
    if !ensure_login(ct).await? {
        return Ok(());
    }
    for cat in categories::ids() {
        trackers::check(ct)?;
        let html = net::get(&parser::forum_url(rq_host, cat, 0), &get_req(ct)).await;
        let Some(html) = html.filter(|h| !h.is_empty() && !parser::looks_like_login_form(h)) else { continue };
        let max_page = parser::last_page_from_html(&html);
        common::merge_forum_pages(&mut TASK_PARSE.lock(), cat, max_page);
    }
    persist_task_parse();
    Ok(())
}

async fn parse_category_page(rq_host: &str, canon_host: &str, cat: &str, page: i32, ct: &CancellationToken) -> Result<Counts, Cancelled> {
    let list_url = parser::forum_url(rq_host, cat, page);
    let list_html = net::get(&list_url, &get_req(ct)).await;
    trackers::check(ct)?;
    let Some(list_html) = list_html.filter(|h| !h.is_empty()) else {
        log_kv(TRACKER, "Listing fetch failed", &[("f", cat.to_string()), ("page", page.to_string()), ("url", list_url)]);
        return Ok(Counts::default());
    };
    if parser::looks_like_login_form(&list_html) {
        parser_log::write(TRACKER, format!("cat={cat} page={page} returned login form - invalidating session"));
        invalidate_cookie();
        return Ok(Counts::default());
    }
    let torrents = parser::parse_listing_html(&list_html, cat, canon_host);
    save_torrents(torrents, ct)
}

fn save_torrents(torrents: Vec<TorrentDetails>, ct: &CancellationToken) -> Result<Counts, Cancelled> {
    let mut c = Counts::default();
    if torrents.is_empty() {
        return Ok(c);
    }
    let torrents: Vec<TorrentDetails> =
        torrents.into_iter().filter(|t| !util::is_blank(&t.name) && !util::is_blank(&t.magnet)).collect();
    c.fetched = torrents.len() as i32;
    if c.fetched == 0 {
        return Ok(Counts::default());
    }

    for (key, list) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for t in list {
            trackers::check(ct)?;
            if util::is_blank(&t.magnet) {
                c.failed += 1;
                parser_log::write_failed(TRACKER, &t, Some("empty magnet"));
                continue;
            }
            if util::magnet_infohash(&t.magnet).is_none() {
                c.failed += 1;
                parser_log::write_failed(TRACKER, &t, Some("invalid magnet infohash"));
                continue;
            }
            let cached = cached_row(&w, &t.url);
            if let Some(cached) = &cached {
                if same_trimmed(&cached.title, &t.title) && !util::is_blank(&cached.magnet) {
                    c.skipped += 1;
                    parser_log::write_skipped(TRACKER, cached, Some("no changes"));
                    continue;
                }
            }
            if cached.is_some() {
                c.updated += 1;
                parser_log::write_updated(TRACKER, &t, Some("magnet/title updated"));
            } else {
                c.added += 1;
                parser_log::write_added(TRACKER, &t);
            }
            w.add_or_update(&t);
        }
    }
    Ok(c)
}

// ---------------------------------------------------------------------------
// ParseAll starter + routes
// ---------------------------------------------------------------------------

pub struct Starter;

#[async_trait]
impl ParseAllStarter for Starter {
    fn tracker_name(&self) -> &'static str {
        TRACKER
    }
    async fn parse_all_task(&self) -> String {
        parse_all_task()
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PageQ {
    page: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PagesQ {
    pages: Option<String>,
}

async fn parse_h(Query(q): Query<PageQ>) -> String {
    parse(q_int(&q.page, 0)).await
}

async fn update_tasks_parse_h() -> String {
    update_tasks_parse()
}

async fn parse_all_task_h() -> String {
    parse_all_task()
}

async fn parse_latest_h(Query(q): Query<PagesQ>) -> String {
    parse_latest(q_int(&q.pages, 5)).await
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/korsars/parse", get(parse_h).post(parse_h))
        .route("/cron/korsars/updatetasksparse", get(update_tasks_parse_h).post(update_tasks_parse_h))
        .route("/cron/korsars/parsealltask", get(parse_all_task_h).post(parse_all_task_h))
        .route("/cron/korsars/parselatest", get(parse_latest_h).post(parse_latest_h))
}
