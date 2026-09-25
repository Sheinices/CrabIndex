//! le-production sync - category listings → post pages → per-quality rows with magnets.
//!
//! Route: `GET /cron/leproduction/parse?limit_page=N` (N ≤ 0 → detect last page).

pub mod categories;
pub mod parser;

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::time::{Duration, Instant};

use crab_core::models::TorrentDetails;
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, ParseLock};
use crab_core::{conf, fdb, util};

use crate::common::{self, cached_row, group_by_key, log_kv, q_int, same_trimmed, secs_f1, Counts};
use parser::TRACKER_NAME as TRACKER;

static PARSE_LOCK: ParseLock = ParseLock::new();

fn useproxy() -> bool {
    conf().Leproduction.useproxy
}

fn get_req() -> Req {
    Req::new().encoding(encoding_rs::UTF_8).useproxy(useproxy())
}

/// Parse category listings. `limit_page > 0` limits pages per category;
/// otherwise the last page is detected from the pagination block.
pub async fn parse(limit_page: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let host = common::trim_host(&conf().Leproduction.rq_host());
        if util::is_blank(&host) {
            parser_log::write(TRACKER, "Config missing - add Leproduction.host");
            return "config missing".to_string();
        }

        let sw = Instant::now();
        let mut total = Counts::default();

        log_kv(TRACKER, "Starting parse", &[("limitPage", limit_page.to_string()), ("host", host.clone())]);

        for cat in categories::MAP {
            let mut total_pages = limit_page;
            if total_pages <= 0 {
                total_pages = detect_last_page(&host, cat.slug).await;
            }

            for page in 1..=total_pages {
                if page > 1 {
                    let delay = conf().Leproduction.parse_delay();
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay as u64)).await;
                    }
                }
                let page_url = if page == 1 { format!("{host}/{}/", cat.slug) } else { format!("{host}/{}/page/{page}/", cat.slug) };

                let c = parse_page(&page_url, &host, cat.slug, cat.types).await;
                total.add(&c);

                log_kv(
                    TRACKER,
                    "Category page done",
                    &[
                        ("cat", cat.slug.to_string()),
                        ("page", page.to_string()),
                        ("totalPages", total_pages.to_string()),
                        ("fetched", c.fetched.to_string()),
                        ("added", c.added.to_string()),
                        ("skipped", c.skipped.to_string()),
                        ("failed", c.failed.to_string()),
                    ],
                );
            }
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
        "ok".to_string()
    })
    .await
}

async fn detect_last_page(host: &str, cat: &str) -> i32 {
    let html = net::get(&format!("{host}/{cat}/"), &get_req()).await;
    parser::detect_last_page(html.as_deref().unwrap_or(""), Some(cat))
}

async fn parse_page(page_url: &str, host: &str, cat: &str, types: &[&str]) -> Counts {
    let html = net::get(page_url, &get_req()).await;
    let Some(html) = html.filter(|h| !h.is_empty()) else {
        log_kv(
            TRACKER,
            "Page fetch failed",
            &[("cat", cat.to_string()), ("url", page_url.to_string()), ("reason", "null response".to_string())],
        );
        return Counts::default();
    };

    let post_urls = parser::extract_post_urls(&html, host);
    if post_urls.is_empty() {
        return Counts::default();
    }

    let mut torrents: Vec<TorrentDetails> = Vec::new();
    for post_url in &post_urls {
        let detail = net::get(post_url, &get_req().referer(page_url)).await;
        let Some(detail) = detail.filter(|h| !h.is_empty()) else { continue };
        torrents.extend(parser::parse_detail_html(&detail, post_url, types));
    }

    let mut c = Counts { fetched: torrents.len() as i32, ..Default::default() };
    if torrents.is_empty() {
        return Counts::default();
    }

    for (key, list) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in list {
            let cached = cached_row(&w, &t.url);
            let need_magnet = match &cached {
                None => true,
                Some(c) => !same_trimmed(&c.title, &t.title) || util::is_blank(&c.magnet),
            };

            if need_magnet && util::is_blank(&t.magnet) {
                if let Some(tid) = parser::extract_torrent_id(&t.url).filter(|x| !util::is_blank(x)) {
                    let down_url = format!("{host}/index.php?do=download&id={tid}");
                    let mag_html = net::get(&down_url, &get_req().referer(host)).await;
                    if let Some(mag_html) = mag_html.filter(|h| !h.is_empty()) {
                        t.magnet = parser::extract_magnet(&mag_html).unwrap_or_default();
                    }
                }
            }

            if need_magnet && util::is_blank(&t.magnet) {
                c.failed += 1;
                parser_log::write_failed(TRACKER, &t, Some("could not get magnet"));
                continue;
            }

            if let Some(cached) = &cached {
                let unchanged = !need_magnet
                    || (!util::is_blank(&cached.magnet)
                        && cached.magnet.to_lowercase() == t.magnet.to_lowercase()
                        && same_trimmed(&cached.title, &t.title));
                if unchanged {
                    c.skipped += 1;
                    parser_log::write_skipped(TRACKER, cached, Some("no changes"));
                    continue;
                }
                c.updated += 1;
                parser_log::write_updated(TRACKER, &t, Some("magnet/title updated"));
            } else {
                c.added += 1;
                parser_log::write_added(TRACKER, &t);
            }
            w.add_or_update(&t);
        }
    }
    c
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    limit_page: Option<String>,
}

async fn parse_h(Query(q): Query<ParseQ>) -> String {
    parse(q_int(&q.limit_page, 0)).await
}

pub fn router() -> Router {
    Router::new().route("/cron/leproduction/parse", get(parse_h))
}
