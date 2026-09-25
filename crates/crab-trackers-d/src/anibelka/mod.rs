//! anibelka sync - anonymous only. Never logs in: passkeys must not enter magnets.
//!
//! Routes: `/cron/anibelka/{parse,updatetasksparse,parsealltask,parselatest}`.

pub mod categories;
pub mod parser;

use async_trait::async_trait;
use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use crab_core::models::TaskMap;
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, cycle, Cancelled, LatestLock, ParseAllStarter, ParseLock, WorkFlag};
use crab_core::{conf, fdb, util};

use crate::common::{self, cached_row, group_by_key, log_kv, q_int, same_trimmed, secs_f1, Counts};
use parser::{AnibelkaDetails, TRACKER_NAME as TRACKER};

const TASK_PARSE_PATH: &str = "Data/temp/anibelka_taskParse.json";

static TASK_PARSE: Lazy<Mutex<TaskMap>> = Lazy::new(|| Mutex::new(common::load_task_map(TASK_PARSE_PATH)));

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

fn host() -> String {
    common::trim_host(&conf().Anibelka.rq_host())
}

fn useproxy() -> bool {
    conf().Anibelka.useproxy
}

fn parse_delay() -> i32 {
    conf().Anibelka.parse_delay()
}

fn get_req(ct: &CancellationToken) -> Req {
    Req::new().encoding(encoding_rs::UTF_8).useproxy(useproxy()).cancel(ct)
}

const CONFIG_MISSING: &str = "Config missing - add Anibelka.host";

/// Parse one zero-based listing page of every section.
pub async fn parse(page: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, true, || async move {
        let host = host();
        if util::is_blank(&host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return "config missing".to_string();
        }
        let ct = CancellationToken::new();
        let mut log = String::new();
        let sw = Instant::now();
        let mut total = Counts::default();

        log_kv(TRACKER, "Starting parse", &[("page", page.to_string()), ("host", host.clone())]);

        for cat in categories::MAP {
            let delay = parse_delay();
            if delay > 0 && trackers::sleep(delay as u64, &ct).await.is_err() {
                return log;
            }
            let c = match parse_section_page(&host, cat.id, page, &ct).await {
                Ok(c) => c,
                Err(_) => return log,
            };
            total.add(&c);
            log.push_str(&format!("{} - {page}\n", cat.id));
            log_kv(
                TRACKER,
                "Section page done",
                &[
                    ("f", cat.id.to_string()),
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

/// Refresh the section → pages map from each section's first listing page (background).
pub fn update_tasks_parse() -> String {
    trackers::run_update_tasks_parse_in_background(TRACKER, &UPDATE_TASKS_WORK, true, |ct| async move {
        let host = host();
        if util::is_blank(&host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return Ok(());
        }
        for cat in categories::MAP {
            trackers::check(&ct)?;
            let html = net::get(&parser::forum_url(&host, cat.id, 0), &get_req(&ct)).await;
            let Some(html) = html.filter(|h| !h.is_empty()) else {
                parser_log::write(TRACKER, format!("UpdateTasksParse f={}: empty response", cat.id));
                continue;
            };
            let max_page = parser::last_page_from_html(&html);
            let (pruned, total) = {
                let mut g = TASK_PARSE.lock();
                let pruned = common::merge_forum_pages(&mut g, cat.id, max_page);
                (pruned, g.get(cat.id).map(|v| v.len()).unwrap_or(0))
            };
            let tail = if pruned > 0 { format!(", pruned={pruned}") } else { String::new() };
            parser_log::write(TRACKER, format!("UpdateTasksParse f={}: maxPage={max_page}, total={total}{tail}", cat.id));
        }
        persist_task_parse();
        Ok(())
    })
}

/// Resumable full crawl of every mapped page (background).
pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER, &PARSE_ALL_TASK_WORK, true, |ct| async move {
        let host = host();
        if util::is_blank(&host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return Ok(());
        }
        if TASK_PARSE.lock().is_empty() {
            rebuild_tasks(&host, &ct).await?;
        }
        let res = run_parse_all(&host, &ct).await;
        persist_task_parse();
        res.map_err(Into::into)
    })
}

async fn run_parse_all(host: &str, ct: &CancellationToken) -> Result<(), Cancelled> {
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

        parse_section_page(host, &cat, page, ct).await?;
        // Empty listings still count as done.
        note_attempt(&cat, page, &cycle, true);

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

fn note_attempt(cat: &str, page: i32, cycle: &cycle::ParseAllCycleState, ok: bool) {
    let mut g = TASK_PARSE.lock();
    if let Some(slot) = g.get_mut(cat).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
        cycle::note_attempt(TRACKER, slot, cycle, ok);
    }
}

/// Cheap daily pass: first `pages` pages of every section (from the task map).
pub async fn parse_latest(pages: i32) -> String {
    trackers::run_parse_latest(TRACKER, &PARSE_LATEST_LOCK, true, || async move {
        let host = host();
        if util::is_blank(&host) {
            parser_log::write(TRACKER, CONFIG_MISSING);
            return "config missing".to_string();
        }
        let pages = if pages <= 0 { 5 } else { pages };
        let ct = CancellationToken::new();

        if TASK_PARSE.lock().is_empty() && rebuild_tasks(&host, &ct).await.is_err() {
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
                match parse_section_page(&host, &cat, page, &ct).await {
                    Ok(_) => {
                        let mut g = TASK_PARSE.lock();
                        if let Some(slot) = g.get_mut(&cat).and_then(|v| v.iter_mut().find(|p| p.page == page)) {
                            cycle::mark_done_in_cycle(slot, &cycle);
                        }
                        drop(g);
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

async fn rebuild_tasks(host: &str, ct: &CancellationToken) -> Result<(), Cancelled> {
    for cat in categories::MAP {
        trackers::check(ct)?;
        let html = net::get(&parser::forum_url(host, cat.id, 0), &get_req(ct)).await;
        let Some(html) = html.filter(|h| !h.is_empty()) else { continue };
        let max_page = parser::last_page_from_html(&html);
        common::merge_forum_pages(&mut TASK_PARSE.lock(), cat.id, max_page);
    }
    persist_task_parse();
    Ok(())
}

async fn parse_section_page(host: &str, section_id: &str, page: i32, ct: &CancellationToken) -> Result<Counts, Cancelled> {
    let list_url = parser::forum_url(host, section_id, page);
    let list_html = net::get(&list_url, &get_req(ct)).await;
    trackers::check(ct)?;
    let Some(list_html) = list_html.filter(|h| !h.is_empty()) else {
        log_kv(TRACKER, "Listing fetch failed", &[("f", section_id.to_string()), ("page", page.to_string()), ("url", list_url)]);
        return Ok(Counts::default());
    };

    let items = parser::parse_listing_html(&list_html);
    if items.is_empty() {
        return Ok(Counts::default());
    }

    let mut torrents: Vec<AnibelkaDetails> = Vec::new();
    for item in &items {
        trackers::check(ct)?;
        let topic_url = parser::topic_url(host, &item.topic_id);
        let delay = parse_delay();
        if delay > 0 {
            trackers::sleep(delay as u64, ct).await?;
        }
        let topic_html = net::get(&topic_url, &get_req(ct).referer(list_url.clone())).await;
        trackers::check(ct)?;
        let Some(info) = topic_html.as_deref().filter(|h| !h.is_empty()).and_then(parser::try_parse_topic_html) else {
            continue;
        };
        let (name, original, year) = parser::parse_title(&item.title);
        if util::is_blank(&name) {
            continue;
        }
        let now = crab_core::time::now();
        let mut t = crab_core::models::TorrentDetails::new(TRACKER, &["anime"], topic_url, item.title.clone());
        t.sid = info.sid;
        t.pir = info.pir;
        t.sizeName = info.size_name.clone();
        t.createTime = if crab_core::time::is_min(&info.create_time) { now } else { info.create_time };
        t.updateTime = now;
        t.name = name;
        t.originalname = original;
        t.relased = year;
        torrents.push(AnibelkaDetails { t, download_id: info.torrent_id });
    }

    save_torrents(torrents, host, ct).await
}

async fn save_torrents(torrents: Vec<AnibelkaDetails>, host: &str, ct: &CancellationToken) -> Result<Counts, Cancelled> {
    let mut c = Counts::default();
    if torrents.is_empty() {
        return Ok(c);
    }
    // Drop records with empty names before the DB merge.
    let torrents: Vec<AnibelkaDetails> =
        torrents.into_iter().filter(|t| !util::is_blank(&t.t.name) && !util::is_blank(&t.download_id)).collect();
    c.fetched = torrents.len() as i32;
    if c.fetched == 0 {
        return Ok(Counts::default());
    }

    for (key, list) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in list {
            let cached = cached_row(&w, &t.t.url);
            let need_magnet = match &cached {
                None => true,
                Some(c) => !same_trimmed(&c.title, &t.t.title) || util::is_blank(&c.magnet),
            };
            if !need_magnet {
                c.skipped += 1;
                if let Some(cached) = &cached {
                    parser_log::write_skipped(TRACKER, cached, Some("no changes"));
                }
                continue;
            }

            if !util::is_blank(&t.download_id) {
                let delay = parse_delay();
                if delay > 0 {
                    trackers::sleep(delay as u64, ct).await?;
                }
                // Anonymous download - never send cookies/login.
                let file = net::download(
                    &parser::torrent_download_url(host, &t.download_id),
                    &Req::new().referer(host).useproxy(useproxy()).timeout(30).cancel(ct),
                )
                .await;
                trackers::check(ct)?;
                if let Some(file) = file.filter(|f| !f.is_empty()) {
                    // Full magnet is fine: an anonymous .torrent has no personal passkey.
                    if let Some(magnet) = bencode::magnet(&file).filter(|m| !util::is_blank(m)) {
                        t.t.magnet = magnet;
                        if util::is_blank(&t.t.sizeName) {
                            if let Some(sn) = bencode::size_name(&file).filter(|s| !util::is_blank(s)) {
                                t.t.sizeName = sn;
                            }
                        }
                    }
                }
            }

            if util::is_blank(&t.t.magnet) {
                c.failed += 1;
                parser_log::write_failed(TRACKER, &t.t, Some("could not get magnet"));
                continue;
            }

            if cached.is_some() {
                c.updated += 1;
                parser_log::write_updated(TRACKER, &t.t, Some("magnet/title updated"));
            } else {
                c.added += 1;
                parser_log::write_added(TRACKER, &t.t);
            }
            w.add_or_update(&t.t);
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
        .route("/cron/anibelka/parse", get(parse_h).post(parse_h))
        .route("/cron/anibelka/updatetasksparse", get(update_tasks_parse_h).post(update_tasks_parse_h))
        .route("/cron/anibelka/parsealltask", get(parse_all_task_h).post(parse_all_task_h))
        .route("/cron/anibelka/parselatest", get(parse_latest_h).post(parse_latest_h))
}
