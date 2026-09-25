//! aniliberty API - anime torrents from aniliberty.top.
//!
//! Route: `GET /cron/aniliberty/parse?parsefrom=1&parseto=5`.

pub mod models;
pub mod parser;

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::time::{Duration, Instant};

use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, ParseLock};
use crab_core::{conf, fdb};

use crate::common::{cached_row, group_by_key, log_kv, q_int, secs_f1};
use models::AnilibertyApiResponse;
use parser::TRACKER_NAME as TRACKER;

static PARSE_LOCK: ParseLock = ParseLock::new();

struct PageResult {
    parsed: i32,
    added: i32,
    updated: i32,
    skipped: i32,
    failed: i32,
    last_page: i32,
}

/// Parse API pages `parse_from..=parse_to` (defaults to page 1), stopping at the API's last page.
pub async fn parse(parse_from: i32, parse_to: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let base_url = conf().Aniliberty.host.clone();

        let mut start_page = if parse_from > 0 { parse_from } else { 1 };
        let mut end_page = if parse_to > 0 {
            parse_to
        } else if parse_from > 0 {
            parse_from
        } else {
            1
        };
        if start_page > end_page {
            std::mem::swap(&mut start_page, &mut end_page);
        }

        log_kv(
            TRACKER,
            "Starting parse",
            &[
                ("parseFrom", parse_from.to_string()),
                ("parseTo", parse_to.to_string()),
                ("startPage", start_page.to_string()),
                ("endPage", end_page.to_string()),
                ("baseUrl", base_url.clone()),
            ],
        );

        let (mut t_parsed, mut t_added, mut t_updated, mut t_skipped, mut t_failed) = (0, 0, 0, 0, 0);
        let mut last_page = i32::MAX;

        let mut page = start_page;
        while page <= end_page && page <= last_page {
            if page > start_page {
                let delay = conf().Aniliberty.parse_delay();
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay as u64)).await;
                }
            }

            log_kv(
                TRACKER,
                "Parsing page",
                &[("page", page.to_string()), ("url", format!("{base_url}/api/v1/anime/torrents?page={page}&limit=50"))],
            );

            let r = parse_page(page).await;
            t_parsed += r.parsed;
            t_added += r.added;
            t_updated += r.updated;
            t_skipped += r.skipped;
            t_failed += r.failed;

            if r.last_page > 0 {
                last_page = r.last_page;
            }
            if page >= last_page {
                break;
            }
            page += 1;
        }

        log_kv(
            TRACKER,
            &format!("Parse completed successfully (took {}s)", secs_f1(sw)),
            &[
                ("parsed", t_parsed.to_string()),
                ("added", t_added.to_string()),
                ("updated", t_updated.to_string()),
                ("skipped", t_skipped.to_string()),
                ("failed", t_failed.to_string()),
            ],
        );
        "ok".to_string()
    })
    .await
}

async fn parse_page(page: i32) -> PageResult {
    let c = conf();
    let url = format!("{}/api/v1/anime/torrents?page={page}&limit=50", c.Aniliberty.host);
    let response: Option<AnilibertyApiResponse> =
        net::get_json(&url, &Req::new().encoding(encoding_rs::UTF_8).useproxy(c.Aniliberty.useproxy)).await;

    let empty = PageResult { parsed: 0, added: 0, updated: 0, skipped: 0, failed: 0, last_page: 0 };
    let reason = match &response {
        None => Some("null response"),
        Some(r) if r.data.as_ref().map(|d| d.is_empty()).unwrap_or(true) => Some("no data"),
        Some(_) => None,
    };
    if let Some(reason) = reason {
        log_kv(TRACKER, "Page parse failed", &[("page", page.to_string()), ("url", url), ("reason", reason.to_string())]);
        return empty;
    }
    let Some(response) = response else { return empty };

    let last_page = response.meta.as_ref().map(|m| m.last_page).unwrap_or(0);
    let torrents = parser::map_page_torrents(&response, &c.Aniliberty.host);

    let parsed = torrents.len() as i32;
    let (mut added, mut updated, mut skipped) = (0, 0, 0);

    for (key, list) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for t in list {
            let cached = cached_row(&w, &t.url);
            if let Some(cached) = &cached {
                if cached.magnet.trim().to_lowercase() == t.magnet.trim().to_lowercase() {
                    skipped += 1;
                    parser_log::write_skipped(TRACKER, cached, Some("no changes"));
                    continue;
                }
            }
            if cached.is_some() {
                updated += 1;
                parser_log::write_updated(TRACKER, &t, Some("magnet changed or updated"));
            } else {
                added += 1;
                parser_log::write_added(TRACKER, &t);
            }
            w.add_or_update(&t);
        }
    }

    if parsed > 0 {
        log_kv(
            TRACKER,
            &format!("Page {page} completed"),
            &[
                ("parsed", parsed.to_string()),
                ("added", added.to_string()),
                ("updated", updated.to_string()),
                ("skipped", skipped.to_string()),
                ("failed", "0".to_string()),
            ],
        );
    }

    PageResult { parsed, added, updated, skipped, failed: 0, last_page }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    parsefrom: Option<String>,
    parseto: Option<String>,
}

async fn parse_h(Query(q): Query<ParseQ>) -> String {
    parse(q_int(&q.parsefrom, 0), q_int(&q.parseto, 0)).await
}

pub fn router() -> Router {
    Router::new().route("/cron/aniliberty/parse", get(parse_h))
}
