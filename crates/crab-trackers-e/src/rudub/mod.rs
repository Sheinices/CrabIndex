//! RuDub sync - login/cookie required; browses HD 1080 + HD 2160 only (videoformat 4/5).
//! Torrent downloads are turned into tracker-less magnets so session passkeys never reach FileDB.

pub mod parser;

use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use crab_core::conf;
use crab_core::fdb;
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log as plog};
use crab_core::trackers::{self, ParseLock};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::common::{self, kv, QueryMap};

pub const TRACKER_NAME: &str = parser::TRACKER_NAME;
const COOKIE_PHPSESSID: &str = "PHPSESSID";
const COOKIE_PASS: &str = "pass";
const COOKIE_UID: &str = "uid";
const ENDPOINT_LOGIN: &str = "/takelogin.php";
const ENDPOINT_BROWSE: &str = "/browse.php";
const PARAM_USERNAME: &str = "username";
const PARAM_PASSWORD: &str = "password";
const LOGIN_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Safety cap for `limit_page` (each page × 2 videoformats + torrent downloads).
pub const MAX_LIMIT_PAGES: i32 = 100;

const COOKIE_CACHE_DURATION: Duration = Duration::from_secs(24 * 3600);

static PARSE_LOCK: ParseLock = ParseLock::new();
static LOGIN_SEMAPHORE: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
/// Session cookie obtained by login, with its expiry.
static COOKIE_CACHE: Lazy<Mutex<Option<(String, Instant)>>> = Lazy::new(|| Mutex::new(None));

fn host() -> String {
    conf().Rudub.rq_host().trim_end_matches('/').to_string()
}

fn cookie() -> Option<String> {
    let c = conf();
    if let Some(ck) = c.Rudub.cookie.as_deref().filter(|s| !s.trim().is_empty()) {
        return Some(ck.trim().to_string());
    }
    let mut g = COOKIE_CACHE.lock();
    match g.as_ref() {
        Some((ck, exp)) if Instant::now() < *exp => Some(ck.clone()),
        Some(_) => {
            *g = None;
            None
        }
        None => None,
    }
}

async fn check_login() -> bool {
    if cookie().is_some() {
        return true;
    }
    let c = conf();
    if !c.Rudub.login_u().trim().is_empty() && !c.Rudub.login_p().trim().is_empty() {
        return take_login().await;
    }
    plog::write(TRACKER_NAME, "No cookie or login credentials available");
    false
}

async fn take_login() -> bool {
    let Ok(_guard) = tokio::time::timeout(Duration::from_secs(15), LOGIN_SEMAPHORE.lock()).await else {
        plog::write(TRACKER_NAME, "TakeLogin skipped: login semaphore timeout (15s)");
        return false;
    };
    if cookie().is_some() {
        return true;
    }
    let c = conf();
    let login = c.Rudub.login_u().to_string();
    let pass = c.Rudub.login_p().to_string();
    let host = host();
    if host.is_empty() {
        return false;
    }

    let client = match reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(15)).build() {
        Ok(c) => c,
        Err(e) => {
            plog::write(TRACKER_NAME, format!("Login HTTP error: {e}"));
            return false;
        }
    };
    let form = [(PARAM_USERNAME, login.as_str()), (PARAM_PASSWORD, pass.as_str())];
    let resp = client.post(format!("{host}{ENDPOINT_LOGIN}")).header("user-agent", LOGIN_USER_AGENT).form(&form).send().await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            plog::write(TRACKER_NAME, format!("Login cancelled: {e}"));
            return false;
        }
        Err(e) => {
            plog::write(TRACKER_NAME, format!("Login HTTP error: {e}"));
            return false;
        }
    };

    let cook: Vec<String> = resp.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(|s| s.to_string())).collect();
    if cook.is_empty() {
        plog::write(TRACKER_NAME, "Login FAILED - no Set-Cookie");
        return false;
    }

    let sessid = extract_cookie_value(&cook, COOKIE_PHPSESSID);
    let pass_cookie = extract_cookie_value(&cook, COOKIE_PASS);
    let uid = extract_cookie_value(&cook, COOKIE_UID);
    let (Some(sessid), Some(pass_cookie), Some(uid)) = (
        sessid.filter(|s| !s.trim().is_empty()),
        pass_cookie.filter(|s| !s.trim().is_empty()),
        uid.filter(|s| !s.trim().is_empty()),
    ) else {
        plog::write(TRACKER_NAME, "Login FAILED - missing PHPSESSID/uid/pass");
        return false;
    };

    let cookie_str = format!("{COOKIE_PHPSESSID}={sessid}; {COOKIE_UID}={uid}; {COOKIE_PASS}={pass_cookie}");
    *COOKIE_CACHE.lock() = Some((cookie_str, Instant::now() + COOKIE_CACHE_DURATION));
    plog::write(TRACKER_NAME, "Login OK");
    true
}

/// Value of `name=` from the first Set-Cookie line containing it.
pub fn extract_cookie_value(cookie_headers: &[String], name: &str) -> Option<String> {
    let key = format!("{name}=");
    let candidate = cookie_headers.iter().find(|l| !l.trim().is_empty() && l.contains(&key))?;
    let idx = candidate.find(&key)? + key.len();
    let v = crab_core::rx::group(&candidate[idx..], "([^;]+)(;|$)", 1);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Browse page range: explicit `parse_from`/`parse_to` when either is non-zero, otherwise
/// `limit_page` means the first N pages (0..N-1), capped at [`MAX_LIMIT_PAGES`].
pub fn resolve_page_range(parse_from: i32, parse_to: i32, limit_page: i32) -> (i32, i32) {
    if limit_page > 0 && parse_from == 0 && parse_to == 0 {
        let n = limit_page.clamp(1, MAX_LIMIT_PAGES);
        return (0, n - 1);
    }
    let start = if parse_from >= 0 { parse_from } else { 0 };
    let end = if parse_to >= 0 { parse_to } else { start };
    if start > end {
        (end, start)
    } else {
        (start, end)
    }
}

pub async fn parse(parse_from: i32, parse_to: i32, limit_page: i32) -> String {
    if host().is_empty() {
        return trackers::DISABLED_RESULT.to_string();
    }
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        if !check_login().await {
            return "login error".to_string();
        }
        let sw = Instant::now();
        let (start_page, end_page) = resolve_page_range(parse_from, parse_to, limit_page);
        let formats: Vec<String> = parser::PREFERRED_VIDEO_FORMATS.iter().map(|v| v.to_string()).collect();
        plog::write_kv(
            TRACKER_NAME,
            "Starting parse",
            &kv![
                ("parseFrom", parse_from),
                ("parseTo", parse_to),
                ("limit_page", limit_page),
                ("startPage", start_page),
                ("endPage", end_page),
                ("pages", end_page - start_page + 1),
                ("videoformats", formats.join(",")),
                ("host", host())
            ],
        );

        let (mut t_parsed, mut t_added, mut t_updated, mut t_skipped, mut t_failed) = (0, 0, 0, 0, 0);
        let mut first_request = true;
        for video_format in parser::PREFERRED_VIDEO_FORMATS {
            for page in start_page..=end_page {
                let delay = conf().Rudub.parse_delay();
                if !first_request && delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay as u64)).await;
                }
                first_request = false;
                plog::write(TRACKER_NAME, format!("Page {page} videoformat={video_format}"));
                let r = parse_page(page, video_format).await;
                t_parsed += r.0;
                t_added += r.1;
                t_updated += r.2;
                t_skipped += r.3;
                t_failed += r.4;
            }
        }

        plog::write_kv(
            TRACKER_NAME,
            &format!("Parse completed successfully (took {}s)", common::secs(sw)),
            &kv![("parsed", t_parsed), ("added", t_added), ("updated", t_updated), ("skipped", t_skipped), ("failed", t_failed)],
        );
        "ok".to_string()
    })
    .await
}

async fn parse_page(page: i32, video_format: i32) -> (i32, i32, i32, i32, i32) {
    let host = host();
    let url = format!("{host}{ENDPOINT_BROWSE}?incldead=0&sort=4&type=desc&videoformat={video_format}&page={page}");
    let req = Req::new().cp1251().cookie_opt(cookie()).useproxy(conf().Rudub.useproxy);
    let html = net::get(&url, &req).await;
    let html = match html {
        Some(h) if h.contains(parser::VALIDATION_MARKER) => h,
        other => {
            let reason = if other.is_none() { "null response" } else { "invalid content" };
            plog::write_kv(TRACKER_NAME, "Page parse failed", &kv![("page", page), ("videoformat", video_format), ("reason", reason)]);
            return (0, 0, 0, 0, 0);
        }
    };

    let torrents = parser::parse_torrent_list_from_html(&html, &host);
    let parsed = torrents.len() as i32;
    let (mut added, mut updated, mut skipped, mut failed) = (0, 0, 0, 0);

    if !torrents.is_empty() {
        let referer = format!("{host}{ENDPOINT_BROWSE}");
        for (key, list) in common::group_by_bucket(torrents) {
            let w = fdb::open_write(&key);
            for mut d in list {
                let ck = cookie();
                let cached = common::cached_row(&w, &d.t.url);
                let write = match &cached {
                    Some(c) if c.title.trim().to_lowercase() == d.t.title.trim().to_lowercase() => {
                        let types_changed = !parser::types_equal(Some(&d.t.types), Some(&c.types));
                        if types_changed {
                            updated += 1;
                            let reason = format!("types updated: [{}] -> [{}]", c.types.join(", "), d.t.types.join(", "));
                            plog::write_updated(TRACKER_NAME, &d.t, Some(&reason));
                            true
                        } else {
                            match download_and_extract(&d.download_uri, ck.as_deref(), &referer).await {
                                Err(e) => {
                                    skipped += 1;
                                    plog::write_skipped(TRACKER_NAME, c, Some(&e));
                                    false
                                }
                                Ok((magnet, size_name)) => {
                                    let magnet_changed = !c.magnet.trim().eq_ignore_ascii_case(magnet.trim());
                                    let size_changed = !c.sizeName.trim().eq_ignore_ascii_case(size_name.trim());
                                    if !magnet_changed && !size_changed {
                                        skipped += 1;
                                        plog::write_skipped(TRACKER_NAME, c, Some("no changes"));
                                        false
                                    } else {
                                        d.t.magnet = magnet;
                                        d.t.sizeName = size_name;
                                        updated += 1;
                                        let reason = if magnet_changed && size_changed {
                                            "magnet and size updated"
                                        } else if magnet_changed {
                                            "magnet updated"
                                        } else {
                                            "size updated"
                                        };
                                        plog::write_updated(TRACKER_NAME, &d.t, Some(reason));
                                        true
                                    }
                                }
                            }
                        }
                    }
                    _ => match download_and_extract(&d.download_uri, ck.as_deref(), &referer).await {
                        Err(e) => {
                            failed += 1;
                            plog::write_failed(TRACKER_NAME, &d.t, Some(&e));
                            false
                        }
                        Ok((magnet, size_name)) => {
                            d.t.magnet = magnet;
                            d.t.sizeName = size_name;
                            if cached.is_some() {
                                updated += 1;
                                plog::write_updated(TRACKER_NAME, &d.t, Some("title changed or new data"));
                            } else {
                                added += 1;
                                plog::write_added(TRACKER_NAME, &d.t);
                            }
                            true
                        }
                    },
                };
                if write {
                    w.add_or_update(&d.t);
                }
            }
        }
    }

    if parsed > 0 {
        plog::write_kv(
            TRACKER_NAME,
            &format!("Page {page} vf={video_format} completed"),
            &kv![("parsed", parsed), ("added", added), ("updated", updated), ("skipped", skipped), ("failed", failed)],
        );
    }
    (parsed, added, updated, skipped, failed)
}

/// Download the `.torrent` and return (tracker-less magnet, size name) or an error description.
async fn download_and_extract(download_uri: &str, cookie: Option<&str>, referer: &str) -> Result<(String, String), String> {
    let req = Req::new().timeout(30).cookie_opt(cookie).referer(referer).useproxy(conf().Rudub.useproxy);
    let data = net::download(download_uri, &req).await;
    let Some(data) = data.filter(|d| !d.is_empty()) else {
        let status = if cookie.map(|c| c.trim().is_empty()).unwrap_or(true) { "no cookie" } else { "cookie present" };
        return Err(format!("failed to download torrent (null or empty), downloadUri={download_uri}, {status}"));
    };
    if !parser::is_valid_bencoded_torrent(&data) {
        return Err(format!("downloaded HTML instead of torrent file, downloadUri={download_uri}"));
    }
    let magnet = bencode::magnet_no_trackers(&data).unwrap_or_default();
    let size_name = bencode::size_name(&data).unwrap_or_default();
    if !magnet.trim().is_empty() && !size_name.trim().is_empty() {
        return Ok((magnet, size_name));
    }
    Err(format!(
        "failed to extract magnet or size: magnet={}, sizeName={}, torrentSize={}",
        if magnet.trim().is_empty() { "null" } else { "ok" },
        if size_name.trim().is_empty() { "null" } else { "ok" },
        data.len()
    ))
}

pub fn router() -> Router {
    Router::new().route(
        "/cron/rudub/parse",
        get(|Query(q): Query<QueryMap>| async move {
            parse(common::q_i32(&q, "parsefrom", 0), common::q_i32(&q, "parseto", 0), common::q_i32(&q, "limit_page", 0)).await
        }),
    )
}
