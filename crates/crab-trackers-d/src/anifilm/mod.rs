//! anifilm sync - category listings, detail page → .torrent → magnet.
//! Optional login (CSRF form) yields a session cookie; a configured cookie wins.
//!
//! Route: `GET /cron/anifilm/parse?fullparse=true|false`.

pub mod categories;
pub mod parser;

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::time::{Duration, Instant};

use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock};
use crab_core::{conf, fdb, rx, time, util};

use crate::common::{self, bool_str, cached_row, group_by_key, log_kv, q_bool, secs_f1, Counts};
use parser::{AnifilmDetails, TRACKER_NAME as TRACKER};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const CSRF_INPUT_RE: &str = r#"(?i)<input[^>]+name="([^"]*CSRF[^"]*)"[^>]+value="([^"]+)""#;
const CSRF_INPUT_RE2: &str = r#"(?i)<input[^>]+value="([^"]+)"[^>]+name="([^"]*CSRF[^"]*)""#;

static PARSE_LOCK: ParseLock = ParseLock::new();

struct CookieState {
    dyn_cookie: Option<String>,
    last_login_attempt: DateTime<Utc>,
}

static COOKIE: Lazy<Mutex<CookieState>> =
    Lazy::new(|| Mutex::new(CookieState { dyn_cookie: None, last_login_attempt: time::min() }));

fn host() -> String {
    common::trim_host(&conf().Anifilm.rq_host())
}

fn useproxy() -> bool {
    conf().Anifilm.useproxy
}

/// Parse category listings. `fullparse` uses the larger per-category page limits.
pub async fn parse(fullparse: bool) -> String {
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let host = host();
        if util::is_blank(&host) {
            parser_log::write(TRACKER, "Config missing - add Anifilm.host");
            return "config missing".to_string();
        }

        let sw = Instant::now();
        let mut total = Counts::default();

        ensure_login().await;

        log_kv(TRACKER, "Starting parse", &[("fullparse", bool_str(fullparse)), ("host", host.clone())]);

        for cat in categories::MAP {
            let max_page = categories::max_pages(cat, fullparse);
            for page in 1..=max_page {
                let delay = conf().Anifilm.parse_delay();
                if page > 1 && delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay as u64)).await;
                }

                let mut create_time = time::now();
                if fullparse {
                    create_time = time::now() - chrono::Duration::days(2 * page as i64);
                }

                let page_url = format!("{host}/releases/page/{page}?category={}", cat.slug);
                let body = net::get(
                    &page_url,
                    &Req::new()
                        .encoding(encoding_rs::UTF_8)
                        .cookie_opt(cookie_header())
                        .referer(format!("{host}/"))
                        .useproxy(useproxy()),
                )
                .await;

                let Some(body) = body.filter(|b| !b.is_empty()) else {
                    continue;
                };
                if looks_like_login_form(&body) {
                    invalidate_cookie();
                    continue;
                }

                let items = parser::parse_listing_html(&body, &host, cat.types, create_time);
                total.fetched += items.len() as i32;
                if items.is_empty() {
                    continue;
                }
                let fetched = items.len();
                let c = save_torrents(items, &host).await;
                total.added += c.added;
                total.updated += c.updated;
                total.skipped += c.skipped;
                total.failed += c.failed;

                log_kv(
                    TRACKER,
                    "Category page done",
                    &[
                        ("cat", cat.slug.to_string()),
                        ("page", page.to_string()),
                        ("maxPage", max_page.to_string()),
                        ("fetched", fetched.to_string()),
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

async fn save_torrents(torrents: Vec<AnifilmDetails>, host: &str) -> Counts {
    let mut c = Counts::default();
    if torrents.is_empty() {
        return c;
    }
    for (key, list) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in list {
            let cached = cached_row(&w, &t.t.url);
            let mut need_magnet = match &cached {
                None => true,
                Some(c) => util::is_blank(&c.magnet),
            };
            if !need_magnet {
                if let Some(cached) = &cached {
                    let exist_title = cached.title.replace(" [1080p]", "");
                    if exist_title != t.t.title {
                        need_magnet = true;
                    }
                }
            }
            if !need_magnet {
                c.skipped += 1;
                if let Some(cached) = &cached {
                    parser_log::write_skipped(TRACKER, cached, Some("no changes"));
                }
                continue;
            }

            let detail_html = net::get(
                &t.t.url,
                &Req::new()
                    .encoding(encoding_rs::UTF_8)
                    .cookie_opt(cookie_header())
                    .referer(format!("{host}/"))
                    .useproxy(useproxy()),
            )
            .await;
            let detail_html = match detail_html.filter(|h| !h.is_empty()) {
                Some(h) if !looks_like_login_form(&h) => h,
                other => {
                    if other.is_some() {
                        invalidate_cookie();
                    }
                    c.failed += 1;
                    parser_log::write_failed(TRACKER, &t.t, Some("detail page empty or login form"));
                    continue;
                }
            };

            let (tid, is1080p) = parser::extract_torrent_download_path(&detail_html);
            let Some(tid) = tid.filter(|x| !util::is_blank(x)) else {
                c.failed += 1;
                parser_log::write_failed(TRACKER, &t.t, Some("tid not found"));
                continue;
            };

            if is1080p && !t.t.title.contains(" [1080p]") {
                t.t.title.push_str(" [1080p]");
            }

            t.download_id = tid.clone();
            let down_url = format!("{host}/{}", tid.trim_start_matches('/'));
            let file = net::download(
                &down_url,
                &Req::new().cookie_opt(cookie_header()).referer(t.t.url.clone()).useproxy(useproxy()).timeout(30),
            )
            .await;
            if let Some(file) = file.filter(|f| !f.is_empty()) {
                if let Some(magnet) = bencode::magnet(&file).filter(|m| !util::is_blank(m)) {
                    t.t.magnet = magnet;
                    if let Some(sn) = bencode::size_name(&file).filter(|s| !util::is_blank(s)) {
                        t.t.sizeName = sn;
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
    c
}

fn cookie_header() -> Option<String> {
    if let Some(c) = COOKIE.lock().dyn_cookie.clone().filter(|c| !util::is_blank(c)) {
        return Some(c);
    }
    conf().Anifilm.cookie.as_deref().filter(|c| !util::is_blank(c)).map(|c| c.trim().to_string())
}

async fn ensure_login() {
    if cookie_header().is_some() {
        return;
    }
    if util::is_blank(conf().Anifilm.login_u()) {
        return;
    }
    take_login().await;
}

fn invalidate_cookie() {
    let mut g = COOKIE.lock();
    g.dyn_cookie = None;
    g.last_login_attempt = time::min();
}

async fn take_login() {
    {
        let mut g = COOKIE.lock();
        if time::now() - g.last_login_attempt < chrono::Duration::minutes(2) {
            return;
        }
        g.last_login_attempt = time::now();
    }

    let host = host();
    let user = conf().Anifilm.login_u().trim().to_string();
    let pass = conf().Anifilm.login_p().to_string();
    if util::is_blank(&host) || util::is_blank(&user) {
        return;
    }

    log_kv(TRACKER, "Attempting login", &[("host", host.clone()), ("user", user.clone())]);

    if let Err((message, kind)) = login_flow(&host, &user, &pass).await {
        log_kv(TRACKER, "Login error", &[("message", message), ("type", kind.to_string())]);
    }
}

/// Returns Err((message, kind)) on transport errors; logical failures are logged here.
async fn login_flow(host: &str, user: &str, pass: &str) -> Result<(), (String, &'static str)> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .no_proxy()
        .build()
        .map_err(|e| (e.to_string(), "RequestError"))?;

    let login_url = format!("{host}/account/login");
    let get_resp = client.get(&login_url).header("User-Agent", USER_AGENT).send().await.map_err(req_err)?;
    let get_headers = get_resp.headers().clone();
    let page_html = get_resp.text().await.map_err(req_err)?;

    let all_cookies = merge_set_cookie("", &get_headers);
    let (mut csrf_name, mut csrf_token) = (String::new(), String::new());
    let m1 = rx::groups(&page_html, CSRF_INPUT_RE);
    if !m1[0].is_empty() {
        csrf_name = m1[1].clone();
        csrf_token = util::html_decode(&m1[2]);
    } else {
        let m2 = rx::groups(&page_html, CSRF_INPUT_RE2);
        if !m2[0].is_empty() {
            csrf_token = util::html_decode(&m2[1]);
            csrf_name = m2[2].clone();
        }
    }
    if util::is_blank(&csrf_token) || util::is_blank(&csrf_name) {
        parser_log::write(TRACKER, "Login failed - CSRF token not found");
        return Ok(());
    }

    let mut form: IndexMap<String, String> = IndexMap::new();
    form.insert(csrf_name, csrf_token);
    form.insert("LoginForm[username]".into(), user.to_string());
    form.insert("LoginForm[password]".into(), pass.to_string());
    form.insert("LoginForm[pass]".into(), String::new());
    let pairs: Vec<(String, String)> = form.into_iter().collect();

    let mut post = client.post(&login_url).header("User-Agent", USER_AGENT).header("Referer", &login_url).form(&pairs);
    if !util::is_blank(&all_cookies) {
        post = post.header("Cookie", &all_cookies);
    }
    let post_resp = post.send().await.map_err(req_err)?;
    let final_cookies = merge_set_cookie(&all_cookies, post_resp.headers());
    let status = post_resp.status().as_u16();
    if status != 302 && !(200..300).contains(&status) {
        log_kv(TRACKER, "Login failed", &[("status", status.to_string())]);
        return Ok(());
    }
    if util::is_blank(&final_cookies) {
        parser_log::write(TRACKER, "Login failed - no cookies in response");
        return Ok(());
    }
    COOKIE.lock().dyn_cookie = Some(final_cookies);
    parser_log::write(TRACKER, "Login OK");
    Ok(())
}

fn req_err(e: reqwest::Error) -> (String, &'static str) {
    let kind = if e.is_timeout() { "TimeoutError" } else { "RequestError" };
    (e.to_string(), kind)
}

/// True when the page is the login form (session missing/expired).
pub fn looks_like_login_form(body: &str) -> bool {
    if body.is_empty() {
        return false;
    }
    let lower = body.to_lowercase();
    lower.contains("action=\"/account/login\"") || lower.contains("action='/account/login'")
}

/// Merge `Set-Cookie` name=value pairs from `headers` into `existing` ("a=1; b=2").
pub fn merge_set_cookie(existing: &str, headers: &HeaderMap) -> String {
    let mut result = existing.to_string();
    for line in headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()) {
        if util::is_blank(line) {
            continue;
        }
        let part = line.split(';').next().unwrap_or("").trim();
        if part.is_empty() {
            continue;
        }
        result = merge_cookie_strings(&result, part);
    }
    result
}

/// Merge two cookie strings; later names win (case-insensitive), first-seen order kept.
pub fn merge_cookie_strings(a: &str, b: &str) -> String {
    let mut map: IndexMap<String, (String, String)> = IndexMap::new();
    let mut add = |s: &str| {
        for piece in s.split(';') {
            let p = piece.trim();
            if p.is_empty() {
                continue;
            }
            let Some(eq) = p.find('=') else { continue };
            if eq == 0 {
                continue;
            }
            let name = p[..eq].trim().to_string();
            let val = p[eq + 1..].trim().to_string();
            match map.get_mut(&name.to_lowercase()) {
                Some(slot) => slot.1 = val,
                None => {
                    map.insert(name.to_lowercase(), (name, val));
                }
            }
        }
    };
    add(a);
    add(b);
    map.values().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    fullparse: Option<String>,
}

async fn parse_h(Query(q): Query<ParseQ>) -> String {
    parse(q_bool(&q.fullparse, false)).await
}

pub fn router() -> Router {
    Router::new().route("/cron/anifilm/parse", get(parse_h))
}
