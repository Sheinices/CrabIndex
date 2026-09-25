//! BitRu via the official API (`api.php?get=torrents`). Rate limit is 5 requests/s per IP,
//! so every API call and download waits 250 ms. Live `after_date` means older-than.

pub mod backfill;
pub mod categories;
pub mod models;
pub mod pagination;
pub mod parser;

use async_trait::async_trait;
use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use chrono::NaiveDate;
use crab_core::models::TorrentDetails;
use crab_core::net::{self, PostBody, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, Cancelled, ParseLock};
use crab_core::{conf, rx, util};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Once;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::common;
use backfill::{BackfillSource, BitruBackfillPage, BitruBackfillProgress};
use models::BitruApiResponse;

pub const TRACKER_NAME: &str = "bitru";
const API_GET_TORRENTS: &str = "torrents";
const API_DELAY_MS: u64 = 250;
const LAST_NEW_TOR_PATH: &str = "Data/temp/bitru_lastnewtor.txt";
const BACKFILL_CURSOR_PATH: &str = "Data/temp/bitru_backfill_cursor.txt";
const LEGACY_LAST_NEW_TOR_PATH: &str = "Data/temp/bitruapi_lastnewtor.txt";

static PARSE_LOCK: ParseLock = ParseLock::new();
static MIGRATE: Once = Once::new();

fn host_url() -> String {
    let h = conf().Bitru.host.trim_end_matches('/').to_string();
    if h.is_empty() {
        "https://bitru.org".into()
    } else {
        h
    }
}

fn api_url() -> String {
    format!("{}/api.php", host_url())
}

/// One-time move of the legacy `bitruapi_lastnewtor.txt` state file.
fn migrate_legacy_last_new_tor_file() {
    MIGRATE.call_once(|| {
        let exists = |p: &str| std::path::Path::new(p).exists();
        if exists(LAST_NEW_TOR_PATH) || !exists(LEGACY_LAST_NEW_TOR_PATH) {
            return;
        }
        if let Err(e) = std::fs::rename(LEGACY_LAST_NEW_TOR_PATH, LAST_NEW_TOR_PATH) {
            parser_log::write(TRACKER_NAME, format!("Legacy lastnewtor migration failed: {e}"));
        }
    });
}

/// Newest page only (regular cron).
pub async fn parse(limit: i32, ct: CancellationToken) -> String {
    migrate_legacy_last_new_tor_file();
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let lim = pagination::clamp_limit(limit);
        parser_log::write(TRACKER_NAME, format!("Parse start, limit={lim}, maxPages=1, api={}", api_url()));

        let res: anyhow::Result<String> = async {
            let page = fetch_one_page(lim, None, None, &ct).await?;
            if !page.stop && !page.torrents.is_empty() {
                save_torrents_and_magnets(&page.torrents, &ct).await?;
                write_last_new_tor(&page.torrents);
                Ok(format!("saved {}", page.torrents.len()))
            } else {
                Ok("no items".to_string())
            }
        }
        .await;

        let log = match res {
            Ok(log) => {
                parser_log::write(TRACKER_NAME, format!("Parse completed in {:.1}s, {log}", sw.elapsed().as_secs_f64()));
                log
            }
            Err(e) => {
                parser_log::write(TRACKER_NAME, format!("Error: {e}"));
                format!("error: {e}")
            }
        };
        if log.trim().is_empty() {
            "ok".into()
        } else {
            log
        }
    })
    .await
}

/// Walk older archive pages from the persisted cursor (`Data/temp/bitru_backfill_cursor.txt`:
/// unix seconds, or `finished` after the last page - the next kick then starts a new pass
/// from the newest page).
pub async fn backfill(pages: i32, limit: i32, ct: CancellationToken) -> String {
    migrate_legacy_last_new_tor_file();
    let max_pages = pagination::clamp_pages(pages);
    let lim = pagination::clamp_limit(limit);

    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let new_cycle = backfill::is_finished(BACKFILL_CURSOR_PATH);
        let start_cursor = backfill::read_start_cursor(BACKFILL_CURSOR_PATH);

        let sw = Instant::now();
        let cursor_label = start_cursor.map(|c| c.to_string()).unwrap_or_else(|| "none".into());
        let api = api_url();
        parser_log::write(
            TRACKER_NAME,
            if new_cycle {
                format!("Backfill start new cycle, pages={max_pages}, limit={lim}, cursor={cursor_label}, api={api}")
            } else {
                format!("Backfill start, pages={max_pages}, limit={lim}, cursor={cursor_label}, api={api}")
            },
        );

        let (log, completed) = crawl_older_pages("Backfill", max_pages, lim, start_cursor, &ct).await;
        if completed {
            parser_log::write(TRACKER_NAME, format!("Backfill completed in {:.1}s, {log}", sw.elapsed().as_secs_f64()));
        }
        if log.trim().is_empty() {
            "ok".into()
        } else {
            log
        }
    })
    .await
}

/// Torrents older than the given calendar day (`dd.MM.yyyy`); continues via the backfill cursor.
pub async fn parse_from_date(lastnewtor: Option<String>, limit: i32, pages: i32, ct: CancellationToken) -> String {
    migrate_legacy_last_new_tor_file();
    let Some(lastnewtor) = lastnewtor.filter(|s| !util::is_blank(s)) else {
        return "bad lastnewtor (use dd.MM.yyyy)".into();
    };
    let trimmed = lastnewtor.trim();
    let from_date = if rx::is_match(trimmed, r"^\d{2}\.\d{2}\.\d{4}$") { NaiveDate::parse_from_str(trimmed, "%d.%m.%Y").ok() } else { None };
    let Some(from_date) = from_date else {
        return "bad date format (use dd.MM.yyyy)".into();
    };

    let max_pages = pagination::clamp_pages(pages);
    let lim = pagination::clamp_limit(limit);
    let unix_from = parser::unix_from_date(from_date);

    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        parser_log::write(
            TRACKER_NAME,
            format!("ParseFromDate lastnewtor={lastnewtor} (olderThan unix={unix_from}), pages={max_pages}, limit={lim}"),
        );
        let (log, completed) = crawl_older_pages("ParseFromDate", max_pages, lim, Some(unix_from), &ct).await;
        if completed {
            parser_log::write(TRACKER_NAME, format!("ParseFromDate completed in {:.1}s, {log}", sw.elapsed().as_secs_f64()));
        }
        if log.trim().is_empty() {
            "ok".into()
        } else {
            log
        }
    })
    .await
}

struct LiveSource {
    limit: i32,
    previous_ids: Option<HashSet<i64>>,
}

#[async_trait]
impl BackfillSource for LiveSource {
    async fn fetch_page(&mut self, cursor: Option<i64>, ct: &CancellationToken) -> anyhow::Result<BitruBackfillPage> {
        let page = fetch_one_page(self.limit, cursor, self.previous_ids.as_ref(), ct).await?;
        if !page.stop {
            self.previous_ids = page.ids.clone();
        }
        Ok(page)
    }

    async fn save_page(&mut self, torrents: &[TorrentDetails], ct: &CancellationToken) -> anyhow::Result<()> {
        save_torrents_and_magnets(torrents, ct).await
    }

    fn commit_cursor(&mut self, unix: i64) {
        if let Err(e) = backfill::write_cursor_atomic(BACKFILL_CURSOR_PATH, unix) {
            parser_log::write(TRACKER_NAME, format!("Write backfill cursor failed: {e}"));
        }
    }

    fn commit_finished(&mut self) {
        if let Err(e) = backfill::write_finished_atomic(BACKFILL_CURSOR_PATH) {
            parser_log::write(TRACKER_NAME, format!("Write backfill finished failed: {e}"));
        }
    }
}

async fn crawl_older_pages(job_label: &str, max_pages: i32, limit: i32, start_cursor: Option<i64>, ct: &CancellationToken) -> (String, bool) {
    let mut progress = BitruBackfillProgress { last_committed_cursor: start_cursor, ..Default::default() };
    let mut source = LiveSource { limit, previous_ids: None };

    match backfill::run(max_pages, start_cursor, &mut source, &mut progress, ct).await {
        Ok(()) => (progress.format_log(), true),
        Err(e) if backfill::is_cancelled(&e) || ct.is_cancelled() => {
            let log = progress.format_canceled_log();
            parser_log::write(TRACKER_NAME, format!("{job_label} canceled, {log}"));
            (log, false)
        }
        Err(e) => {
            parser_log::write(TRACKER_NAME, format!("{job_label} error: {e}"));
            (format!("error: {e}"), false)
        }
    }
}

async fn api_request(params: &serde_json::Map<String, serde_json::Value>, ct: &CancellationToken) -> anyhow::Result<Option<BitruApiResponse>> {
    let json = serde_json::to_string(params)?;
    let post_data = format!("get={API_GET_TORRENTS}&json={}", util::url_encode(&json));
    trackers::check(ct)?;
    let req = Req::new().timeout(15).useproxy(conf().Bitru.useproxy);
    let response = net::post(&api_url(), &PostBody::Form(post_data), &req).await;
    let Some(response) = response.filter(|r| !util::is_blank(r)) else { return Ok(None) };
    Ok(Some(serde_json::from_str::<BitruApiResponse>(&response)?))
}

async fn fetch_one_page(limit: i32, older_than_unix: Option<i64>, previous_ids: Option<&HashSet<i64>>, ct: &CancellationToken) -> anyhow::Result<BitruBackfillPage> {
    trackers::sleep(API_DELAY_MS, ct).await?;

    let params = pagination::build_request_params(limit, older_than_unix);
    let resp = api_request(&params, ct).await?;
    let Some(resp) = resp else { return Ok(BitruBackfillPage::halt()) };
    let items_len = resp.result.as_ref().and_then(|r| r.items.as_ref()).map(|i| i.len());
    if resp.has_error() || items_len.is_none() {
        if resp.has_error() {
            if let Some(msg) = resp.error_message().filter(|m| !m.is_empty()) {
                parser_log::write(TRACKER_NAME, format!("API error: {msg}"));
            }
        }
        return Ok(BitruBackfillPage::halt());
    }
    if items_len == Some(0) {
        return Ok(BitruBackfillPage::halt());
    }

    let page_torrents = parser::parse_torrents_from_response(Some(&resp), &host_url());
    let page_ids = pagination::collect_torrent_ids(page_torrents.iter().map(|t| t.url.as_str()));

    if pagination::is_duplicate_page(previous_ids, Some(&page_ids)) {
        parser_log::write(TRACKER_NAME, "Stop: page fully overlaps previous page");
        return Ok(BitruBackfillPage::halt());
    }

    let next_cursor = pagination::try_get_next_older_page_cursor(resp.result.as_ref(), older_than_unix);
    Ok(BitruBackfillPage::ok(page_torrents, next_cursor, Some(page_ids)))
}

async fn save_torrents_and_magnets(torrents: &[TorrentDetails], ct: &CancellationToken) -> anyhow::Result<()> {
    let host = host_url();
    let host = &host;
    common::add_or_update_async(torrents.to_vec(), common::by_url, |mut t, cached| async move {
        if cached.map(|c| c.title == t.title).unwrap_or(false) {
            return Some(t);
        }

        let mut download_url = t._sn.clone();
        if util::is_blank(&download_url) || !download_url.to_lowercase().starts_with("http") {
            let id = rx::group(&t.url, r"\?id=(\d+)", 1);
            download_url = if id.is_empty() { String::new() } else { format!("{host}/api.php?download={id}") };
        }
        if util::is_blank(&download_url) {
            return None;
        }

        trackers::sleep(API_DELAY_MS, ct).await.ok()?;

        let req = Req::new().referer(format!("{host}/")).timeout(15).useproxy(conf().Bitru.useproxy).cancel(ct);
        let data = net::download(&download_url, &req).await;
        let magnet = data.as_deref().and_then(bencode::magnet).filter(|m| !util::is_blank(m))?;
        t.magnet = magnet;
        t._sn = String::new();
        Some(t)
    })
    .await;

    if ct.is_cancelled() {
        return Err(Cancelled.into());
    }
    Ok(())
}

fn write_last_new_tor(torrents: &[TorrentDetails]) {
    // First of the newest createTime values wins (stable order).
    let mut best: Option<&TorrentDetails> = None;
    for t in torrents {
        if best.map(|b| t.createTime > b.createTime).unwrap_or(true) {
            best = Some(t);
        }
    }
    if let Some(t) = best {
        let _ = std::fs::write(LAST_NEW_TOR_PATH, t.createTime.format("%d.%m.%Y").to_string());
    }
}

// ---------------------------------------------------------------------------
// HTTP (`/cron/bitru/...`)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    limit: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BackfillQ {
    pages: Option<String>,
    limit: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FromDateQ {
    lastnewtor: Option<String>,
    limit: Option<String>,
    pages: Option<String>,
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(common::q_int(&q.limit, 100), CancellationToken::new()).await
}

async fn h_backfill(Query(q): Query<BackfillQ>) -> String {
    backfill(common::q_int(&q.pages, 20), common::q_int(&q.limit, 100), CancellationToken::new()).await
}

async fn h_parse_from_date(Query(q): Query<FromDateQ>) -> String {
    parse_from_date(q.lastnewtor.clone(), common::q_int(&q.limit, 100), common::q_int(&q.pages, 20), CancellationToken::new()).await
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/bitru/parse", get(h_parse).post(h_parse))
        .route("/cron/bitru/backfill", get(h_backfill).post(h_backfill))
        .route("/cron/bitru/parsefromdate", get(h_parse_from_date).post(h_parse_from_date))
}
