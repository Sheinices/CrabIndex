//! Anidub: listing pages → magnet from the .torrent / detail page.
//!
//! Route: `/cron/anidub/parse?parsefrom&parseto`.

pub mod parser;

use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crab_core::models::TorrentDetails;
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock};
use crab_core::util::{html_decode, is_blank};
use crab_core::{conf, fdb, rx};

use crate::common::{cached, group_by_key, int_param, kv, secs};
use parser::{AnidubDetails, TRACKER};

static PARSE_LOCK: ParseLock = ParseLock::new();

type Counts = (i32, i32, i32, i32, i32);

fn host() -> String {
    conf().Anidub.host.clone()
}

fn useproxy() -> bool {
    conf().Anidub.useproxy
}

fn log_kv(msg: &str, data: Vec<(String, String)>) {
    parser_log::write_kv(TRACKER, msg, &data);
}

fn utf8_req() -> Req {
    Req::new().encoding(encoding_rs::UTF_8).useproxy(useproxy())
}

pub async fn parse(parse_from: i32, parse_to: i32) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let base_url = host();
        let mut start = if parse_from > 0 { parse_from } else { 1 };
        let mut end = if parse_to > 0 { parse_to } else if parse_from > 0 { parse_from } else { 1 };
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        log_kv(
            "Starting parse",
            kv!("parseFrom" => parse_from, "parseTo" => parse_to, "startPage" => start, "endPage" => end, "baseUrl" => base_url),
        );
        let mut tot: Counts = (0, 0, 0, 0, 0);
        for page in start..=end {
            if page > start {
                tokio::time::sleep(Duration::from_millis(conf().Anidub.parse_delay().max(0) as u64)).await;
            }
            if page > 1 {
                log_kv("Parsing page", kv!("page" => page, "url" => format!("{base_url}/page/{page}/")));
            }
            let r = parse_page(page).await;
            tot = (tot.0 + r.0, tot.1 + r.1, tot.2 + r.2, tot.3 + r.3, tot.4 + r.4);
        }
        log_kv(
            &format!("Parse completed successfully (took {}s)", secs(sw)),
            kv!("parsed" => tot.0, "added" => tot.1, "updated" => tot.2, "skipped" => tot.3, "failed" => tot.4),
        );
        "ok".to_string()
    })
    .await
}

fn size_from_detail(detail: &str) -> Option<String> {
    let mut g = rx::groups_i(detail, "Размер[^:]*:\\s*<span[^>]*>([^<]+)</span>");
    if g[0].is_empty() {
        g = rx::groups_i(detail, "Размер[^:]*:\\s*([^<]+)");
    }
    if g[0].is_empty() {
        None
    } else {
        Some(html_decode(&g[1]).trim().to_string())
    }
}

fn download_url_from_detail(detail: &str) -> Option<String> {
    let u = rx::group_i(detail, "href=\"([^\"]*engine/download\\.php\\?id=[0-9]+)\"", 1);
    if u.is_empty() {
        return None;
    }
    Some(if u.starts_with("http") { u } else { format!("{}/{}", host(), u.trim_start_matches('/')) })
}

fn magnet_from_detail(detail: &str) -> Option<String> {
    let g = rx::group_i(detail, "href=\"(magnet:\\?[^\"]+)\"", 1);
    if g.is_empty() {
        None
    } else {
        Some(g)
    }
}

async fn get_detail(url: &str) -> Option<String> {
    net::get(url, &utf8_req()).await
}

async fn download(url: &str, referer: &str, useproxy: bool) -> Option<Vec<u8>> {
    net::download(url, &Req::new().referer(referer).useproxy(useproxy).timeout(30)).await.filter(|d| !d.is_empty())
}

#[derive(Default)]
struct Stats {
    added: i32,
    updated: i32,
    skipped: i32,
    failed: i32,
}

/// Decide whether to store `t` (filling magnet/size/relased); mirrors the listing refresh rules.
async fn process(t: &mut AnidubDetails, cached_row: Option<TorrentDetails>, st: &mut Stats) -> bool {
    let exists = cached_row.is_some();
    let mut detail_html: Option<String> = None;

    if let Some(c) = cached_row.as_ref() {
        if c.title.trim().to_lowercase() == t.t.title.trim().to_lowercase() && !is_blank(&c.magnet) {
            detail_html = get_detail(&t.t.url).await;
            if let Some(detail) = detail_html.as_deref() {
                let relased = parser::extract_relased(detail);
                if relased > 0 {
                    t.t.relased = relased;
                }
                if let Some(current) = magnet_from_detail(detail) {
                    let should_skip = c.magnet.eq_ignore_ascii_case(&current);
                    if should_skip && c.relased == 0 && relased > 0 {
                        t.t.magnet = current;
                        t.t.relased = relased;
                        if let Some(s) = size_from_detail(detail) {
                            t.t.sizeName = s;
                        }
                        st.updated += 1;
                        parser_log::write_updated(TRACKER, &t.t, Some("relased updated"));
                        return true;
                    }
                    if should_skip {
                        st.skipped += 1;
                        parser_log::write_skipped(TRACKER, c, Some("no changes"));
                        return false;
                    }
                    t.t.magnet = current;
                    if let Some(s) = size_from_detail(detail) {
                        t.t.sizeName = s;
                    }
                    if let Some(dl) = download_url_from_detail(detail) {
                        if let Some(file) = download(&dl, &t.t.url, useproxy()).await {
                            if let Some(s) = bencode::size_name(&file).filter(|s| !is_blank(s)) {
                                t.t.sizeName = s;
                            }
                        }
                    }
                    st.updated += 1;
                    parser_log::write_updated(TRACKER, &t.t, Some("magnet changed"));
                    return true;
                }
            }
        }
    }

    if detail_html.is_none() {
        if let Some(torrent) = download(&t.download_uri, &host(), false).await {
            let magnet = bencode::magnet(&torrent).unwrap_or_default();
            let size_name = bencode::size_name(&torrent).unwrap_or_default();
            if !is_blank(&magnet) && !is_blank(&size_name) {
                t.t.magnet = magnet;
                t.t.sizeName = size_name;
                if exists {
                    st.updated += 1;
                    parser_log::write_updated(TRACKER, &t.t, Some("magnet from downloadUri"));
                } else {
                    st.added += 1;
                    parser_log::write_added(TRACKER, &t.t);
                }
                return true;
            }
        }
    }

    if detail_html.is_none() {
        detail_html = get_detail(&t.t.url).await;
    }

    if let Some(detail) = detail_html.as_deref() {
        let relased = parser::extract_relased(detail);
        if relased > 0 {
            t.t.relased = relased;
        }
        if let Some(mag) = magnet_from_detail(detail) {
            t.t.magnet = mag;
            if let Some(s) = size_from_detail(detail) {
                t.t.sizeName = s;
            }
            if exists {
                st.updated += 1;
                parser_log::write_updated(TRACKER, &t.t, Some("magnet from detail page"));
            } else {
                st.added += 1;
                parser_log::write_added(TRACKER, &t.t);
            }
            return true;
        }
        if let Some(dl) = download_url_from_detail(detail) {
            if let Some(file) = download(&dl, &t.t.url, useproxy()).await {
                let magnet = bencode::magnet(&file).unwrap_or_default();
                let size_name = bencode::size_name(&file).unwrap_or_default();
                if !is_blank(&magnet) && !is_blank(&size_name) {
                    t.t.magnet = magnet;
                    t.t.sizeName = size_name;
                    if t.t.relased == 0 && relased > 0 {
                        t.t.relased = relased;
                    }
                    if exists {
                        st.updated += 1;
                        parser_log::write_updated(TRACKER, &t.t, Some("magnet from torrent file"));
                    } else {
                        st.added += 1;
                        parser_log::write_added(TRACKER, &t.t);
                    }
                    return true;
                }
            }
            if let Some(s) = size_from_detail(detail) {
                t.t.sizeName = s;
            }
        }
    }

    st.failed += 1;
    parser_log::write_failed(TRACKER, &t.t, Some("could not get magnet or size"));
    false
}

async fn parse_page(page: i32) -> Counts {
    let h = host();
    let url = if page == 1 { h.clone() } else { format!("{h}/page/{page}/") };
    let html = match get_detail(&url).await {
        Some(x) if x.contains(parser::VALIDATION_DLE_CONTENT) => x,
        other => {
            let reason = if other.is_none() { "null response" } else { "invalid content" };
            log_kv("Page parse failed", kv!("page" => page, "url" => url, "reason" => reason));
            return (0, 0, 0, 0, 0);
        }
    };

    let torrents = parser::parse_torrent_list_from_html(&html, &h, page);
    let parsed = torrents.len() as i32;
    let mut st = Stats::default();

    for (key, group) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in group {
            let c = cached(&w, &t.t.url);
            if process(&mut t, c, &mut st).await {
                w.add_or_update(&t.t);
            }
        }
    }

    if parsed > 0 {
        log_kv(
            &format!("Page {page} completed"),
            kv!("parsed" => parsed, "added" => st.added, "updated" => st.updated, "skipped" => st.skipped, "failed" => st.failed),
        );
    }
    (parsed, st.added, st.updated, st.skipped, st.failed)
}

// ---------------------------------------------------------------------------
// routes
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    parsefrom: Option<String>,
    parseto: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/cron/anidub/parse", get(h_parse).post(h_parse))
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(int_param(&q.parsefrom, 0), int_param(&q.parseto, 0)).await
}
