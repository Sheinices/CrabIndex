//! viruseproject sync - category listings → release pages → one row per quality (.torrent → magnet).
//!
//! Route: `GET /cron/viruseproject/parse?limit_page=N` (N ≤ 0 → detect from pagination-end).

pub mod categories;
pub mod parser;

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::time::{Duration, Instant};

use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock};
use crab_core::{conf, fdb, util};

use crate::common::{self, cached_row, group_by_key, log_kv, q_int, same_trimmed, secs_f1, Counts};
use parser::{ViruseprojectDetails, TRACKER_NAME as TRACKER};

static PARSE_LOCK: ParseLock = ParseLock::new();

fn useproxy() -> bool {
    conf().Viruseproject.useproxy
}

fn get_req() -> Req {
    Req::new().encoding(encoding_rs::UTF_8).useproxy(useproxy())
}

/// Parse category listings. `limit_page > 0` limits pages per category (capped at the
/// detected last page); otherwise every page is crawled.
pub async fn parse(limit_page: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let host = common::trim_host(&conf().Viruseproject.rq_host());
        if util::is_blank(&host) {
            parser_log::write(TRACKER, "Config missing - add Viruseproject.host");
            return "config missing".to_string();
        }

        let sw = Instant::now();
        let mut total = Counts::default();

        log_kv(TRACKER, "Starting parse", &[("limitPage", limit_page.to_string()), ("host", host.clone())]);

        for cat in categories::MAP {
            let step = parser::get_page_step(cat.slug);
            let first_url = format!("{host}/releases/{}?start=0", cat.slug);
            let Some(first_body) = net::get(&first_url, &get_req()).await.filter(|b| !b.is_empty()) else {
                continue;
            };

            let last_page = parser::detect_last_page(&first_body, step);
            let mut total_pages = limit_page;
            if total_pages <= 0 || total_pages > last_page {
                total_pages = last_page;
            }

            for page in 1..=total_pages {
                let (body, page_url) = if page == 1 {
                    (first_body.clone(), first_url.clone())
                } else {
                    let delay = conf().Viruseproject.parse_delay();
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay as u64)).await;
                    }
                    let page_url = format!("{host}/releases/{}?start={}", cat.slug, (page - 1) * step);
                    match net::get(&page_url, &get_req()).await.filter(|b| !b.is_empty()) {
                        Some(b) => (b, page_url),
                        None => continue,
                    }
                };

                let c = parse_listing(&body, &page_url, &host, cat.types).await;
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

async fn parse_listing(body: &str, page_url: &str, host: &str, types: &[&str]) -> Counts {
    let post_urls = parser::extract_post_urls(body, host);
    if post_urls.is_empty() {
        return Counts::default();
    }
    let mut torrents: Vec<ViruseprojectDetails> = Vec::new();
    for post_url in &post_urls {
        let detail = net::get(post_url, &get_req().referer(page_url)).await;
        let Some(detail) = detail.filter(|h| !h.is_empty()) else { continue };
        torrents.extend(parser::parse_detail_html(&detail, post_url, host, types));
    }
    save_torrents(torrents, host).await
}

async fn save_torrents(torrents: Vec<ViruseprojectDetails>, host: &str) -> Counts {
    let mut c = Counts { fetched: torrents.len() as i32, ..Default::default() };
    if torrents.is_empty() {
        return Counts::default();
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

            if !util::is_blank(&t.download_uri) {
                let file = net::download(&t.download_uri, &Req::new().referer(host).useproxy(useproxy()).timeout(30)).await;
                if let Some(file) = file.filter(|f| !f.is_empty()) {
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
                match &cached {
                    Some(cached) if !util::is_blank(&cached.magnet) => t.t.magnet = cached.magnet.clone(),
                    _ => {
                        c.failed += 1;
                        parser_log::write_failed(TRACKER, &t.t, Some("could not get magnet"));
                        continue;
                    }
                }
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
    Router::new().route("/cron/viruseproject/parse", get(parse_h))
}
