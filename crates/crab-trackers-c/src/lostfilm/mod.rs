//! LostFilm: `/new/` feed, per-episode V-page expansion to 1080p/2160p rows, movies and season packs.
//!
//! Routes (`/cron/lostfilm/*`): `parse`, `parsepages?pagefrom&pageto`, `parseseasonpacks?series`,
//! `verifypage?series`, `stats`.

pub mod parser;

use std::time::Instant;

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Utc};
use indexmap::IndexSet;
use serde::Deserialize;
use serde_json::{json, Value};

use crab_core::models::TorrentDetails;
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log, tparse};
use crab_core::trackers::{self, ParseLock};
use crab_core::util::{html_decode, is_blank};
use crab_core::{conf, fdb, rx, time};

use crate::common::{cached, group_by_key, int_param, secs, take_chars};
use parser::TRACKER;

static PARSE_LOCK: ParseLock = ParseLock::new();

type Magnet = (String, String, String); // (magnet, quality, sizeName)

fn host() -> String {
    let h = conf().Lostfilm.host.clone();
    if h.is_empty() {
        "https://www.lostfilm.tv".into()
    } else {
        h
    }
}

fn cookie() -> Option<String> {
    conf().Lostfilm.cookie.clone()
}

fn useproxy() -> bool {
    conf().Lostfilm.useproxy
}

fn log(msg: impl AsRef<str>) {
    parser_log::write(TRACKER, msg);
}

fn req(cookie: &Option<String>) -> Req {
    Req::new().cookie_opt(cookie.clone()).useproxy(useproxy())
}

fn ensure_auth() -> bool {
    if parser::has_auth_cookie(cookie().as_deref()) {
        return true;
    }
    log("No cookie or login credentials available - lostfilm requires lf_loyal_person/lf_session/lf_udv/PHPSESSID (see docs/trackers/lostfilm.mdx)");
    false
}

// ---------------------------------------------------------------------------
// public operations
// ---------------------------------------------------------------------------

/// First `/new/` page only (latest releases).
pub async fn parse() -> String {
    if !ensure_auth() {
        return "auth".into();
    }
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async {
        let sw = Instant::now();
        let host = host();
        let cookie = cookie();
        log("Parse (page /new/) start");
        parse_page(&host, &cookie, 1, None, None, None).await;
        log(format!("Parse done in {}s", secs(sw)));
        "ok".to_string()
    })
    .await
}

/// `/new/` pages in `[page_from, page_to]` (clamped to the real page count).
pub async fn parse_pages(page_from: i32, page_to: i32) -> String {
    if !ensure_auth() {
        return "auth".into();
    }
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let host = host();
        let cookie = cookie();
        let delay = conf().Lostfilm.parse_delay();
        let page_from = page_from.max(1);
        let mut page_to = if page_to < page_from { page_from } else { page_to };

        log(format!("ParsePages start pageFrom={page_from} pageTo={page_to} host={host}"));

        let first = net::get(&format!("{host}/new/"), &req(&cookie).http2()).await;
        let total = parser::extract_total_pages_from_new_page_html(first.as_deref().unwrap_or(""));
        if page_to > total {
            page_to = total;
        }
        log(format!("Pagination: totalPages={total} will parse pages {page_from}..{page_to}"));

        for page in page_from..=page_to {
            if page > 1 && delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
            }
            let pre = if page == 1 { first.clone() } else { None };
            parse_page(&host, &cookie, page, None, None, pre).await;
        }
        log(format!("ParsePages done in {}s", secs(sw)));
        "ok".to_string()
    })
    .await
}

/// `/series/{series}/seasons/` → full-season rows (`e=999`) for 1080p / 2160p.
pub async fn parse_season_packs(series: Option<String>) -> String {
    if !ensure_auth() {
        return "auth".into();
    }
    let Some(series) = series.filter(|s| !is_blank(s)) else {
        return "series required".into();
    };
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let host = host();
        let cookie = cookie();
        let series = series.trim().to_string();
        log(format!("ParseSeasonPacks start series={series}"));

        let seasons_url = format!("{host}/series/{series}/seasons/");
        let html = net::get(&seasons_url, &req(&cookie)).await.unwrap_or_default();
        if html.is_empty() || !html.contains("LostFilm.TV") {
            log(format!("ParseSeasonPacks: empty or invalid response {seasons_url}"));
            return "empty".to_string();
        }
        let (relased, russian_name) = parser::parse_relased_and_name_from_html(&html);
        if relased <= 0 {
            log(format!("ParseSeasonPacks: no relased in HTML for {series}"));
            return "no relased".to_string();
        }
        let originalname = series.replace('_', " ");
        let name = russian_name.filter(|n| !is_blank(n)).unwrap_or_else(|| originalname.clone());

        let mut seen: IndexSet<String> = IndexSet::new();
        let mut list: Vec<TorrentDetails> = Vec::new();
        for m in rx::all_groups_i(&html, r#"href="(/V/\?[^"]+)""#) {
            let v_path = &m[1];
            if !v_path.to_lowercase().contains("e=999") {
                continue;
            }
            let season_num: i32 = rx::group_i(v_path, r"[?&]s=(\d+)", 1).parse().unwrap_or(0);
            if season_num <= 0 {
                continue;
            }
            let v_full_url = absolutize(&host, v_path);
            if !seen.insert(v_full_url.to_lowercase()) {
                continue;
            }
            let magnets = get_magnets_from_v_page(&host, &cookie, &v_full_url).await;
            if magnets.is_empty() {
                log(format!("  no magnets: {series} s{season_num}"));
                continue;
            }
            let create_time = time::now();
            for (magnet, quality, size_name) in &magnets {
                list.push(TorrentDetails {
                    trackerName: TRACKER.into(),
                    types: vec!["serial".into()],
                    url: format!("{v_full_url}#{quality}"),
                    title: format!("{name} / {originalname} / {season_num} сезон (полный сезон) [{relased}, {quality}]"),
                    sid: 1,
                    createTime: create_time,
                    name: name.clone(),
                    originalname: originalname.clone(),
                    relased,
                    magnet: magnet.clone(),
                    sizeName: size_name.clone(),
                    ..Default::default()
                });
            }
            log(format!("  + {name} {season_num} сезон (полный): {} quality", magnets.len()));
        }

        if !list.is_empty() {
            fdb::add_or_update(&list);
            log(format!("ParseSeasonPacks: added {} torrents", list.len()));
        } else {
            log("ParseSeasonPacks: no season-pack links found");
        }
        log(format!("ParseSeasonPacks done in {}s", secs(sw)));
        "ok".to_string()
    })
    .await
}

/// Fetch `/new/` and report what the parser extracts (dates / years); optional series filter.
pub async fn verify_page(series: Option<String>) -> Value {
    let host = host();
    let cookie = cookie();
    let url = format!("{host}/new/");
    let html = net::get(&url, &req(&cookie).http2()).await.unwrap_or_default();
    if html.is_empty() || !html.contains("LostFilm.TV") {
        return json!({ "error": "empty", "url": url });
    }
    let mut items = parser::parse_new_page_dates(&html, &host);
    let mut series_filter: Option<String> = None;
    if let Some(s) = series.filter(|s| !is_blank(s)) {
        let f = s.trim().replace(' ', "_");
        let norm = f.replace('_', " ").to_lowercase();
        let norm_us = norm.replace(' ', "_");
        items.retain(|i| {
            let u = i.url.to_lowercase();
            let t = i.title.to_lowercase();
            u.contains(&norm_us) || u.contains(&norm) || t.contains(&norm)
        });
        series_filter = Some(f);
    }
    let mut obj = serde_json::Map::new();
    obj.insert("ok".into(), json!(true));
    obj.insert("url".into(), json!(url));
    if let Some(f) = series_filter {
        obj.insert("filteredBy".into(), json!(f));
    }
    obj.insert("count".into(), json!(items.len()));
    obj.insert("items".into(), serde_json::to_value(&items).unwrap_or(Value::Array(vec![])));
    Value::Object(obj)
}

/// Lostfilm rows in the database: totals, with/without magnet and bucket keys.
pub fn get_stats() -> Value {
    let mut keys: IndexSet<String> = IndexSet::new();
    let (mut total, mut with_magnet) = (0i64, 0i64);
    for (key, _) in fdb::master_db_snapshot() {
        for t in fdb::open_read(&key, false, false).values() {
            if t.trackerName != TRACKER {
                continue;
            }
            total += 1;
            if !t.magnet.is_empty() {
                with_magnet += 1;
            }
            keys.insert(key.clone());
        }
    }
    let mut keys: Vec<String> = keys.into_iter().collect();
    keys.sort();
    let count = keys.len();
    json!({
        "total": total,
        "withMagnet": with_magnet,
        "withoutMagnet": total - with_magnet,
        "keysCount": count,
        "keys": keys.iter().take(50).collect::<Vec<_>>(),
        "keysMore": if count > 50 { count - 50 } else { 0 },
    })
}

// ---------------------------------------------------------------------------
// page parsing
// ---------------------------------------------------------------------------

fn absolutize(host: &str, path: &str) -> String {
    if path.starts_with("http") {
        path.to_string()
    } else if path.starts_with('/') {
        format!("{}{path}", host.trim_end_matches('/'))
    } else {
        format!("{}/{path}", host.trim_end_matches('/'))
    }
}

/// Parse one `/new/` page. Returns true when `stop_before` was reached.
async fn parse_page(
    host: &str,
    cookie: &Option<String>,
    page: i32,
    stop_before: Option<DateTime<Utc>>,
    start_from: Option<DateTime<Utc>>,
    preloaded: Option<String>,
) -> bool {
    let url = if page > 1 { format!("{host}/new/page_{page}") } else { format!("{host}/new/") };
    let html = match preloaded.filter(|h| !h.is_empty()) {
        Some(h) => {
            log(format!("Page {page}: use preloaded"));
            h
        }
        None => {
            log(format!("Page {page}: GET {url}"));
            net::get(&url, &req(cookie).http2()).await.unwrap_or_default()
        }
    };
    if html.is_empty() {
        log(format!("Page {page}: empty response"));
        return false;
    }
    if !html.contains("LostFilm.TV") {
        log(format!("Page {page}: no 'LostFilm.TV' in response (cookies/redirect?)"));
        return false;
    }

    let normalized = tparse::replace_bad_names(&html);
    let mut list: Vec<TorrentDetails> = Vec::new();
    let map = parser::build_hor_breaker_name_map(&normalized);

    parser::collect_from_episode_links(&normalized, host, &mut list, page, Some(&map));
    let mut source = String::from("episode_links");
    if list.is_empty() {
        parser::collect_from_new_movie(&normalized, host, &mut list, page, Some(&map));
        source = "new-movie".into();
    }
    if list.is_empty() {
        parser::collect_from_hor_breaker(&normalized, host, &mut list, page);
        source = "hor-breaker".into();
    }
    let before_movies = list.len();
    collect_from_movies(&normalized, host, cookie, &mut list).await;

    parser::dedupe_list_by_url(&mut list);
    if list.len() > before_movies {
        source = format!("{source}+movies:{}", list.len() - before_movies);
    }

    let oldest = list.iter().map(|t| t.createTime).min();

    if let Some(sf) = start_from {
        if !list.is_empty() {
            let before = list.len();
            list.retain(|t| t.createTime <= sf);
            if list.len() < before {
                log(format!("Page {page}: filtered by startFromDate {before} -> {}", list.len()));
            }
        }
    }

    log(format!("Page {page}: collected {} items (source={source})", list.len()));

    if let (Some(sb), Some(o)) = (stop_before, oldest) {
        if o <= sb {
            return true;
        }
    }
    if list.is_empty() {
        return false;
    }

    let (mut list, expand_failed) = expand_serial_episodes_to_qualities(host, cookie, list).await;
    parser::dedupe_list_by_url(&mut list);

    let (mut added, mut from_cache, mut no_magnet) = (0, 0, 0);
    for (key, group) in group_by_key(list) {
        let w = fdb::open_write(&key);
        for mut t in group {
            if !t.magnet.is_empty() {
                w.add_or_update(&t);
                continue;
            }
            if let Some(c) = cached(&w, &t.url).filter(|c| !c.magnet.is_empty()) {
                from_cache += 1;
                parser::apply_magnet_cache(&mut t, &c);
                w.add_or_update(&t);
                continue;
            }

            let bare = parser::strip_url_fragment(&t.url).to_string();
            let mag = if t.has_type("movie") {
                get_magnet_for_movie(host, cookie, &bare).await
            } else {
                get_magnet_first_quality(host, cookie, &bare).await
            };
            let Some((magnet, quality, size_name)) = mag.filter(|m| !m.0.is_empty()) else {
                no_magnet += 1;
                log(format!("  no magnet: {}", t.url));
                continue;
            };
            t.magnet = magnet;
            t.sizeName = size_name;
            if !quality.is_empty() {
                let q = parser::normalize_quality(&quality);
                t.title = parser::append_quality_to_title(&t.title, &q);
                if !t.url.contains('#') {
                    t.url = format!("{}#{q}", parser::strip_url_fragment(&t.url));
                }
            }
            added += 1;
            log(format!("  + {}... [{quality}]", take_chars(&t.title, 60)));
            w.add_or_update(&t);
        }
    }
    log(format!("Page {page}: added={added} fromCache={from_cache} noMagnet={no_magnet} expandFailed={expand_failed}"));
    false
}

/// Resolve V-page magnets for each episode and emit one row per quality (`{url}#1080p`).
async fn expand_serial_episodes_to_qualities(host: &str, cookie: &Option<String>, list: Vec<TorrentDetails>) -> (Vec<TorrentDetails>, i32) {
    let mut result = Vec::with_capacity(list.len() * 2);
    let mut expand_failed = 0;
    let delay = conf().Lostfilm.parse_delay();

    for t in list {
        if !t.magnet.is_empty() || t.has_type("movie") || t.url.contains('#') || !parser::is_episode_path_url(&t.url) {
            result.push(t);
            continue;
        }
        let bare = parser::strip_url_fragment(&t.url).to_string();
        let magnets = get_magnets_for_episode(host, cookie, &bare).await;
        if magnets.is_empty() {
            expand_failed += 1;
            log(format!("  expand failed (no magnets): {bare}"));
            continue;
        }
        for (magnet, quality, size_name) in &magnets {
            result.push(parser::clone_with_quality(&t, magnet, quality, size_name));
        }
        log(format!("  expand {} → 1080p/2160p ({})", t.name, magnets.len()));
        if delay > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay.min(2000) as u64)).await;
        }
    }
    (result, expand_failed)
}

/// Movies from `/new/`: movie page → V-page → 1080p / 2160p rows.
async fn collect_from_movies(html: &str, host: &str, cookie: &Option<String>, list: &mut Vec<TorrentDetails>) {
    let mut seen: IndexSet<String> = IndexSet::new();
    for row in parser::movie_rows(html) {
        let movie_page_url = format!("{}/{}", host.trim_end_matches('/'), row.url.trim_start_matches('/'));
        if !seen.insert(movie_page_url.to_lowercase()) {
            continue;
        }
        let create_time = tparse::parse_create_time(&row.date_str, "dd.MM.yyyy").unwrap_or_else(time::now);
        let relased = create_time.year();

        let Some(v_page_url) = get_v_url_from_movie_page(host, cookie, &movie_page_url).await.filter(|u| !u.is_empty()) else {
            log(format!("  movie no V link: {}", row.name));
            continue;
        };
        let magnets = get_magnets_from_v_page(host, cookie, &v_page_url).await;
        if magnets.is_empty() {
            log(format!("  movie no magnets: {}", row.name));
            continue;
        }
        let name = html_decode(&row.name);
        let originalname = html_decode(&row.originalname);
        for (magnet, quality, size_name) in &magnets {
            let q = parser::normalize_quality(quality);
            list.push(TorrentDetails {
                trackerName: TRACKER.into(),
                types: vec!["movie".into()],
                url: format!("{movie_page_url}#{q}"),
                title: format!("{name} / {originalname} [Фильм, {relased}, {q}]"),
                sid: 1,
                createTime: create_time,
                name: name.clone(),
                originalname: originalname.clone(),
                relased,
                magnet: magnet.clone(),
                sizeName: size_name.clone(),
                ..Default::default()
            });
        }
        log(format!("  + movie {name} (1080p/2160p x{})", magnets.len()));
    }
}

const META_REDIRECT_RE: &str = r#"(?:content="[^"]*url\s*=\s*|location\.replace\s*\(\s*["'])([^"]+)"#;

/// Movie page `/movies/Slug` → V-page url (direct link or via `v_search.php`).
async fn get_v_url_from_movie_page(host: &str, cookie: &Option<String>, movie_page_url: &str) -> Option<String> {
    let html = net::get(movie_page_url, &req(cookie)).await.filter(|h| !h.is_empty())?;
    let v = rx::group_i(&html, r#"href="(/V/\?[^"]+)""#, 1);
    if !v.is_empty() {
        return Some(if v.starts_with("http") { v } else { format!("{}{v}", host.trim_end_matches('/')) });
    }
    let id = parser::try_extract_play_movie_or_episode_id(&html)?;
    let search = net::get(&format!("{host}/v_search.php?a={id}"), &req(cookie)).await.filter(|h| !h.is_empty())?;
    let meta = rx::groups(&search, META_REDIRECT_RE);
    if !meta[0].is_empty() {
        let u = meta[1].trim();
        return Some(if u.starts_with("http") { u.to_string() } else { format!("{}{u}", host.trim_end_matches('/')) });
    }
    let href = rx::group(&search, r#"href="(/V/\?[^"]+)""#, 1);
    if !href.is_empty() {
        return Some(format!("{}{href}", host.trim_end_matches('/')));
    }
    None
}

/// Movie fallback: first available quality.
async fn get_magnet_for_movie(host: &str, cookie: &Option<String>, movie_url: &str) -> Option<Magnet> {
    let v = get_v_url_from_movie_page(host, cookie, movie_url).await.filter(|u| !u.is_empty())?;
    get_magnets_from_v_page(host, cookie, &v).await.into_iter().next()
}

/// Download the 1080p / 2160p torrents listed on a V-page and convert them to magnets.
async fn parse_v_page_quality_links(host: &str, cookie: &Option<String>, search_html: &str) -> Vec<Magnet> {
    let links: Vec<(String, String)> =
        parser::parse_v_page_quality_link_urls(search_html).into_iter().filter(|x| parser::is_preferred_quality(&x.1)).collect();
    let mut results = Vec::new();
    let delay = conf().Lostfilm.parse_delay().min(2000);
    for (torrent_url, quality) in links {
        if delay > 0 && !results.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
        }
        let r = Req::new().cookie_opt(cookie.clone()).referer(format!("{host}/")).timeout(30);
        let Some(data) = net::download(&torrent_url, &r).await.filter(|d| !d.is_empty()) else { continue };
        let Some(magnet) = bencode::magnet(&data).filter(|m| !m.is_empty()) else { continue };
        let size_name = bencode::size_name(&data).unwrap_or_default();
        results.push((magnet, parser::normalize_quality(&quality), size_name));
    }
    results
}

/// V-page html (direct url or through `v_search.php?a=`), following meta/JS redirects once.
async fn fetch_v_page_html(host: &str, cookie: &Option<String>, v_page_url: Option<&str>, episode_id: Option<&str>) -> Option<String> {
    let h = host.trim_end_matches('/');
    let search_html = if let Some(id) = episode_id.filter(|s| !s.is_empty()) {
        net::get(&format!("{host}/v_search.php?a={id}"), &req(cookie)).await.filter(|s| !s.is_empty())?
    } else if let Some(v) = v_page_url.filter(|s| !s.is_empty()) {
        let url = absolutize(host, v);
        net::get(&url, &req(cookie).referer(format!("{host}/"))).await.filter(|s| !s.is_empty())?
    } else {
        return None;
    };

    if search_html.contains("inner-box--link") {
        return Some(search_html);
    }
    let mut v_url = String::new();
    let meta = rx::groups(&search_html, META_REDIRECT_RE);
    if !meta[0].is_empty() {
        v_url = meta[1].trim().to_string();
    }
    if v_url.is_empty() {
        v_url = rx::group(&search_html, r#"href="(/V/\?[^"]+)""#, 1).trim().to_string();
    }
    let has_cookie = cookie.as_deref().map(|c| !is_blank(c)).unwrap_or(false);
    if v_url.is_empty() {
        if has_cookie {
            log("auth/V-page: no inner-box--link and no redirect (cookie expired?)");
        }
        return Some(search_html);
    }
    if v_url.starts_with('/') {
        v_url = format!("{h}{v_url}");
    }
    let html = net::get(&v_url, &req(cookie).referer(format!("{host}/"))).await.unwrap_or_default();
    if has_cookie && !html.is_empty() && !html.contains("inner-box--link") {
        log("auth/V-page: final HTML missing inner-box--link (cookie expired?)");
    }
    Some(html)
}

async fn get_magnets_for_episode(host: &str, cookie: &Option<String>, episode_url: &str) -> Vec<Magnet> {
    let html = net::get(episode_url, &req(cookie)).await.unwrap_or_default();
    if html.is_empty() {
        log(format!("      GetMagnetsForEpisode: empty episode page {episode_url}"));
        return Vec::new();
    }
    let Some(episode_id) = parser::try_extract_play_episode_id(&html) else {
        log(format!("      GetMagnetsForEpisode: no PlayEpisode in {episode_url}"));
        return Vec::new();
    };
    log(format!("      GetMagnetsForEpisode: episodeId={episode_id}"));

    let search_html = fetch_v_page_html(host, cookie, None, Some(&episode_id)).await.unwrap_or_default();
    if search_html.is_empty() {
        log(format!("      GetMagnetsForEpisode: empty V-page response for {episode_url} (no authorization?)"));
        return Vec::new();
    }
    if !search_html.contains("inner-box--link") {
        if parser::has_auth_cookie(cookie.as_deref()) {
            log(format!("      GetMagnetsForEpisode: no inner-box--link (cookie expired?) for {episode_url}"));
        } else {
            log("      GetMagnetsForEpisode: no inner-box--link after V page (cookie is not configured - see docs/trackers/lostfilm.mdx)");
        }
        return Vec::new();
    }
    parse_v_page_quality_links(host, cookie, &search_html).await
}

async fn get_magnet_first_quality(host: &str, cookie: &Option<String>, episode_url: &str) -> Option<Magnet> {
    get_magnets_for_episode(host, cookie, episode_url).await.into_iter().next()
}

/// Direct V-page (e.g. `/V/?c=589&s=4&e=999`) → 1080p / 2160p magnets.
async fn get_magnets_from_v_page(host: &str, cookie: &Option<String>, v_page_url: &str) -> Vec<Magnet> {
    let html = fetch_v_page_html(host, cookie, Some(v_page_url), None).await.unwrap_or_default();
    if html.is_empty() || !html.contains("inner-box--link") {
        return Vec::new();
    }
    parse_v_page_quality_links(host, cookie, &html).await
}

// ---------------------------------------------------------------------------
// routes
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct PagesQ {
    pagefrom: Option<String>,
    pageto: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SeriesQ {
    series: Option<String>,
}

pub fn router() -> Router {
    Router::new()
        .route("/cron/lostfilm/parse", get(parse).post(parse))
        .route(
            "/cron/lostfilm/parsepages",
            get(h_parse_pages).post(h_parse_pages),
        )
        .route(
            "/cron/lostfilm/parseseasonpacks",
            get(h_season_packs).post(h_season_packs),
        )
        .route("/cron/lostfilm/verifypage", get(h_verify).post(h_verify))
        .route("/cron/lostfilm/stats", get(h_stats).post(h_stats))
}

async fn h_parse_pages(Query(q): Query<PagesQ>) -> String {
    parse_pages(int_param(&q.pagefrom, 1), int_param(&q.pageto, 1)).await
}

async fn h_season_packs(Query(q): Query<SeriesQ>) -> String {
    parse_season_packs(q.series).await
}

async fn h_verify(Query(q): Query<SeriesQ>) -> Json<Value> {
    Json(verify_page(q.series).await)
}

async fn h_stats() -> Json<Value> {
    Json(tokio::task::spawn_blocking(get_stats).await.unwrap_or_else(|_| json!({})))
}
