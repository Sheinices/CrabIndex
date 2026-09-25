//! Anistar: category listings (anime / hentai / dorams) → post pages → `engine/gettorrent.php`.
//! Requests go to `alias` (request host) while FileDB urls stay on `host`.
//!
//! Route: `/cron/anistar/parse?limit_page`.

pub mod categories;
pub mod parser;

use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crab_core::net::{self, cf, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock};
use crab_core::util::is_blank;
use crab_core::{conf, fdb};

use crate::common::{b, cached, group_by_key, int_param, kv, secs};
use parser::TRACKER;

static PARSE_LOCK: ParseLock = ParseLock::new();

fn canonical_host() -> String {
    conf().Anistar.host.trim_end_matches('/').to_string()
}

fn request_host() -> String {
    conf().Anistar.rq_host().trim_end_matches('/').to_string()
}

fn fetch_url(canon: &str) -> String {
    conf().Anistar.rq_host_uri(canon)
}

fn cookie_or_none() -> Option<String> {
    conf().Anistar.cookie.clone().filter(|c| !is_blank(c))
}

fn useproxy() -> bool {
    conf().Anistar.useproxy
}

fn log_kv(msg: &str, data: Vec<(String, String)>) {
    parser_log::write_kv(TRACKER, msg, &data);
}

fn page_req(cookie: &Option<String>) -> Req {
    Req::new().cp1251().cookie_opt(cookie.clone()).useproxy(useproxy())
}

pub async fn parse(limit_page: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let rq_host = request_host();
        let canon_host = canonical_host();
        if is_blank(&rq_host) || is_blank(&canon_host) {
            log_kv("Config missing", kv!("reason" => if is_blank(&canon_host) { "empty host" } else { "empty request host" }));
            return "config missing".to_string();
        }
        let cookie = cookie_or_none();
        let cookie_set = cookie.is_some();
        log_kv(
            "Starting parse",
            kv!("limitPage" => limit_page, "host" => canon_host, "rqHost" => rq_host, "cookieSet" => b(cookie_set)),
        );

        let (mut fetched, mut added, mut updated, mut skipped, mut failed, mut empty_pages) = (0, 0, 0, 0, 0, 0);

        for cat in categories::MAP {
            let cat_path = cat.id;
            let mut last_page = limit_page;
            if last_page <= 0 {
                let first = net::get(&format!("{rq_host}/{cat_path}/"), &page_req(&cookie)).await.unwrap_or_default();
                last_page = parser::detect_last_page(&first, Some(cat_path));
            }
            for page in 1..=last_page {
                let list_url = if page <= 1 { format!("{rq_host}/{cat_path}/") } else { format!("{rq_host}/{cat_path}/page/{page}/") };
                log_kv("Parsing list page", kv!("category" => cat_path, "page" => page, "url" => list_url));

                let list_html = net::get(&list_url, &page_req(&cookie).referer(format!("{rq_host}/"))).await;
                let Some(post_urls) = try_use_list_html(list_html.as_deref(), &list_url, cat_path, page, &canon_host, cookie_set) else {
                    empty_pages += 1;
                    continue;
                };
                fetched += post_urls.len() as i32;

                for canon_post in &post_urls {
                    let r = parse_detail_and_save(canon_post, &list_url, &rq_host, cat.types, &cookie, cookie_set).await;
                    added += r.0;
                    updated += r.1;
                    skipped += r.2;
                    failed += r.3;
                    let delay = conf().Anistar.parse_delay();
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay as u64)).await;
                    }
                }
            }
        }

        let no_results = fetched == 0;
        let msg = if no_results {
            format!("Parse completed with no results (took {}s)", secs(sw))
        } else {
            format!("Parse completed successfully (took {}s)", secs(sw))
        };
        log_kv(
            &msg,
            kv!("fetched" => fetched, "added" => added, "updated" => updated, "skipped" => skipped, "failed" => failed, "emptyPages" => empty_pages),
        );
        if no_results { "empty" } else { "ok" }.to_string()
    })
    .await
}

fn try_use_list_html(list_html: Option<&str>, list_url: &str, cat_path: &str, page: i32, canon_host: &str, cookie_set: bool) -> Option<Vec<String>> {
    let html = list_html.unwrap_or("");
    if html.is_empty() {
        log_kv(
            "Page fetch failed",
            kv!("category" => cat_path, "page" => page, "url" => list_url, "htmlLength" => 0, "cookieSet" => b(cookie_set), "reason" => "null response"),
        );
        return None;
    }
    let len = html.encode_utf16().count();
    if cf::is_challenge_body(html) {
        log_kv(
            "Page fetch failed",
            kv!("category" => cat_path, "page" => page, "url" => list_url, "htmlLength" => len, "cookieSet" => b(cookie_set), "reason" => "cloudflare challenge"),
        );
        return None;
    }
    let urls = parser::extract_post_urls(html, canon_host);
    if urls.is_empty() {
        log_kv(
            "No posts extracted",
            kv!(
                "category" => cat_path,
                "page" => page,
                "url" => list_url,
                "htmlLength" => len,
                "cookieSet" => b(cookie_set),
                "hasDleContent" => b(html.to_lowercase().contains("dle-content"))
            ),
        );
        return None;
    }
    Some(urls)
}

async fn parse_detail_and_save(
    canon_post_url: &str,
    referer: &str,
    rq_host: &str,
    types: &[&str],
    cookie: &Option<String>,
    cookie_set: bool,
) -> (i32, i32, i32, i32) {
    let fetch_post_url = fetch_url(canon_post_url);
    let post_html = net::get(&fetch_post_url, &page_req(cookie).referer(referer)).await.unwrap_or_default();
    if post_html.is_empty() || cf::is_challenge_body(&post_html) {
        log_kv(
            "Detail fetch failed",
            kv!(
                "url" => fetch_post_url,
                "canonUrl" => canon_post_url,
                "htmlLength" => post_html.encode_utf16().count(),
                "cookieSet" => b(cookie_set),
                "reason" => if post_html.is_empty() { "null response" } else { "cloudflare challenge" }
            ),
        );
        return (0, 0, 0, 1);
    }

    let torrents = parser::parse_detail_torrents(&post_html, canon_post_url, types);
    if torrents.is_empty() {
        log_kv(
            "No torrents extracted",
            kv!("url" => fetch_post_url, "canonUrl" => canon_post_url, "htmlLength" => post_html.encode_utf16().count()),
        );
        return (0, 0, 0, 0);
    }

    let (mut added, mut updated, mut skipped, mut failed) = (0, 0, 0, 0);
    for (key, group) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in group {
            let existing = cached(&w, &t.t.url);
            let need_magnet = match existing.as_ref() {
                None => true,
                Some(c) => c.title.trim() != t.t.title.trim() || is_blank(&c.magnet),
            };
            if !need_magnet {
                skipped += 1;
                if let Some(c) = existing.as_ref() {
                    parser_log::write_skipped(TRACKER, c, Some("no changes"));
                }
                continue;
            }

            if !is_blank(&t.download_id) {
                let down_url = format!("{rq_host}/engine/gettorrent.php?id={}", t.download_id);
                let r = Req::new().cookie_opt(cookie.clone()).referer(rq_host).useproxy(useproxy()).timeout(30);
                if let Some(file) = net::download(&down_url, &r).await.filter(|d| !d.is_empty()) {
                    if let Some(magnet) = bencode::magnet(&file).filter(|m| !is_blank(m)) {
                        t.t.magnet = magnet;
                        if let Some(s) = bencode::size_name(&file).filter(|s| !is_blank(s)) {
                            t.t.sizeName = s;
                        }
                    }
                }
            }

            if is_blank(&t.t.magnet) {
                failed += 1;
                parser_log::write_failed(TRACKER, &t.t, Some("could not get magnet"));
                continue;
            }
            if existing.is_some() {
                updated += 1;
                parser_log::write_updated(TRACKER, &t.t, Some("magnet"));
            } else {
                added += 1;
                parser_log::write_added(TRACKER, &t.t);
            }
            w.add_or_update(&t.t);
        }
    }
    (added, updated, skipped, failed)
}

// ---------------------------------------------------------------------------
// routes
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    limit_page: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/cron/anistar/parse", get(h_parse).post(h_parse))
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(int_param(&q.limit_page, 0)).await
}
