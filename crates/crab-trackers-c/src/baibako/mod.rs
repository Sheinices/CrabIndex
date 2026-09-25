//! Baibako: cookie (config or `takelogin.php`) → `browse.php?page=N` → `.torrent` download.
//!
//! Route: `/cron/baibako/parse?parsefrom&parseto` (page numbering starts at 0).

pub mod parser;

use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use once_cell::sync::Lazy;
use serde::Deserialize;

use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock, DISABLED_RESULT};
use crab_core::util::is_blank;
use crab_core::{conf, fdb, rx};

use crate::common::{cached, group_by_key, int_param, kv, login_client, secs, Expiring};
use parser::{BaibakoDetails, TRACKER};

const COOKIE_PHPSESSID: &str = "PHPSESSID";
const COOKIE_PASS: &str = "pass";
const COOKIE_UID: &str = "uid";
const ENDPOINT_LOGIN: &str = "/takelogin.php";
const ENDPOINT_BROWSE: &str = "/browse.php";
const COOKIE_TTL: Duration = Duration::from_secs(24 * 3600);

static PARSE_LOCK: ParseLock = ParseLock::new();
static COOKIE_CACHE: Expiring = Expiring::new();
static LOGIN_SEMAPHORE: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

type Counts = (i32, i32, i32, i32, i32);

fn host() -> String {
    conf().Baibako.host.clone()
}

fn log(msg: impl AsRef<str>) {
    parser_log::write(TRACKER, msg);
}

fn log_kv(msg: &str, data: Vec<(String, String)>) {
    parser_log::write_kv(TRACKER, msg, &data);
}

fn cookie() -> Option<String> {
    if let Some(c) = conf().Baibako.cookie.clone().filter(|c| !is_blank(c)) {
        return Some(c);
    }
    COOKIE_CACHE.get()
}

fn login_creds() -> Option<(String, String)> {
    let c = conf();
    let u = c.Baibako.login.u.clone().unwrap_or_default();
    let p = c.Baibako.login.p.clone().unwrap_or_default();
    if is_blank(&u) || is_blank(&p) {
        None
    } else {
        Some((u, p))
    }
}

async fn check_login() -> bool {
    if cookie().is_some() {
        return true;
    }
    if login_creds().is_some() {
        return take_login().await;
    }
    log("No cookie or login credentials available");
    false
}

/// Value of `name=` from the first Set-Cookie line that contains it.
pub fn extract_cookie_value(lines: &[String], name: &str) -> Option<String> {
    let key = format!("{name}=");
    let candidate = lines.iter().find(|l| !is_blank(l) && l.contains(&key))?;
    let idx = candidate.find(&key)? + key.len();
    let v = rx::group(&candidate[idx..], "([^;]+)(;|$)", 1);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

async fn take_login() -> bool {
    let Ok(_guard) = tokio::time::timeout(Duration::from_secs(15), LOGIN_SEMAPHORE.lock()).await else {
        log("TakeLogin skipped: login semaphore timeout (15s)");
        return false;
    };
    if cookie().is_some() {
        return true;
    }
    let Some((user, pass)) = login_creds() else { return false };
    let host = host();
    if host.is_empty() {
        return false;
    }
    let Some(client) = login_client(10) else { return false };
    let resp = client
        .post(format!("{host}{ENDPOINT_LOGIN}"))
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/75.0.3770.100 Safari/537.36",
        )
        .form(&[("username", user.as_str()), ("password", pass.as_str())])
        .send()
        .await;
    match resp {
        Ok(resp) => {
            let lines: Vec<String> =
                resp.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(|s| s.to_string())).collect();
            if !lines.is_empty() {
                let sessid = extract_cookie_value(&lines, COOKIE_PHPSESSID).unwrap_or_default();
                let pass_cookie = extract_cookie_value(&lines, COOKIE_PASS).unwrap_or_default();
                let uid = extract_cookie_value(&lines, COOKIE_UID).unwrap_or_default();
                if !is_blank(&sessid) && !is_blank(&uid) && !is_blank(&pass_cookie) {
                    COOKIE_CACHE.set(format!("{COOKIE_PHPSESSID}={sessid}; {COOKIE_UID}={uid}; {COOKIE_PASS}={pass_cookie}"), COOKIE_TTL);
                    log("Login OK");
                    return true;
                }
            }
        }
        Err(e) if e.is_timeout() => log(format!("Login cancelled: {e}")),
        Err(e) => log(format!("Login HTTP error: {e}")),
    }
    false
}

pub async fn parse(parse_from: i32, parse_to: i32) -> String {
    if host().is_empty() {
        return DISABLED_RESULT.into();
    }
    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        if !check_login().await {
            return "login error".to_string();
        }
        let sw = Instant::now();
        let base_url = format!("{}{ENDPOINT_BROWSE}", host());
        let mut start = parse_from.max(0);
        let mut end = if parse_to >= 0 { parse_to } else { parse_from.max(0) };
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
                tokio::time::sleep(Duration::from_millis(conf().Baibako.parse_delay().max(0) as u64)).await;
            }
            log(format!("Page {page}: {base_url}?page={page}"));
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

/// Download a .torrent and extract (magnet, sizeName); `Err(reason)` on failure.
async fn download_and_extract(download_uri: &str, cookie: &Option<String>, referer: &str) -> Result<(String, String), String> {
    let data = net::download(download_uri, &Req::new().cookie_opt(cookie.clone()).referer(referer).timeout(30)).await;
    let Some(data) = data.filter(|d| !d.is_empty()) else {
        let status = if cookie.as_deref().map(is_blank).unwrap_or(true) { "no cookie" } else { "cookie present" };
        return Err(format!("failed to download torrent (null or empty), downloadUri={download_uri}, {status}"));
    };
    if !parser::is_valid_bencoded_torrent(Some(&data)) {
        return Err(format!("downloaded HTML instead of torrent file, downloadUri={download_uri}"));
    }
    let magnet = bencode::magnet(&data).unwrap_or_default();
    let size_name = bencode::size_name(&data).unwrap_or_default();
    if !is_blank(&magnet) && !is_blank(&size_name) {
        return Ok((magnet, size_name));
    }
    Err(format!(
        "failed to extract magnet or size: magnet={}, sizeName={}, torrentSize={}",
        if is_blank(&magnet) { "null" } else { "ok" },
        if is_blank(&size_name) { "null" } else { "ok" },
        data.len()
    ))
}

#[derive(Default)]
struct Stats {
    added: i32,
    updated: i32,
    skipped: i32,
    failed: i32,
}

async fn process(t: &mut BaibakoDetails, existing: Option<crab_core::models::TorrentDetails>, st: &mut Stats) -> bool {
    let cookie = cookie();
    let referer = format!("{}{ENDPOINT_BROWSE}", host());

    if let Some(c) = existing.as_ref().filter(|c| c.title.trim().to_lowercase() == t.t.title.trim().to_lowercase()) {
        if !parser::types_equal(&t.t.types, &c.types) {
            st.updated += 1;
            let reason = format!("types updated: [{}] -> [{}]", c.types.join(", "), t.t.types.join(", "));
            parser_log::write_updated(TRACKER, &t.t, Some(&reason));
            return true;
        }
        let (magnet, size_name) = match download_and_extract(&t.download_uri, &cookie, &referer).await {
            Ok(v) => v,
            Err(e) => {
                st.skipped += 1;
                parser_log::write_skipped(TRACKER, c, Some(&e));
                return false;
            }
        };
        let magnet_changed = c.magnet.trim().to_lowercase() != magnet.trim().to_lowercase();
        let size_changed = c.sizeName.trim().to_lowercase() != size_name.trim().to_lowercase();
        if !magnet_changed && !size_changed {
            st.skipped += 1;
            parser_log::write_skipped(TRACKER, c, Some("no changes"));
            return false;
        }
        t.t.magnet = magnet;
        t.t.sizeName = size_name;
        st.updated += 1;
        let reason = if magnet_changed && size_changed {
            "magnet and size updated"
        } else if magnet_changed {
            "magnet updated"
        } else {
            "size updated"
        };
        parser_log::write_updated(TRACKER, &t.t, Some(reason));
        return true;
    }

    match download_and_extract(&t.download_uri, &cookie, &referer).await {
        Err(e) => {
            st.failed += 1;
            parser_log::write_failed(TRACKER, &t.t, Some(&e));
            false
        }
        Ok((magnet, size_name)) => {
            t.t.magnet = magnet;
            t.t.sizeName = size_name;
            if existing.is_some() {
                st.updated += 1;
                parser_log::write_updated(TRACKER, &t.t, Some("title changed or new data"));
            } else {
                st.added += 1;
                parser_log::write_added(TRACKER, &t.t);
            }
            true
        }
    }
}

async fn parse_page(page: i32) -> Counts {
    let h = host();
    let html = net::get(&format!("{h}{ENDPOINT_BROWSE}?page={page}"), &Req::new().cp1251().cookie_opt(cookie())).await;
    let html = match html {
        Some(x) if x.contains(parser::VALIDATION_NAV_TOP) => x,
        other => {
            log_kv(
                "Page parse failed",
                kv!("page" => page, "reason" => if other.is_none() { "null response" } else { "invalid content" }),
            );
            return (0, 0, 0, 0, 0);
        }
    };

    let torrents = parser::parse_torrent_list_from_html(&html, &h, page);
    let parsed = torrents.len() as i32;
    let mut st = Stats::default();
    for (key, group) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in group {
            let existing = cached(&w, &t.t.url);
            if process(&mut t, existing, &mut st).await {
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
    Router::new().route("/cron/baibako/parse", get(h_parse).post(h_parse))
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(int_param(&q.parsefrom, 0), int_param(&q.parseto, 0)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_values() {
        let lines = vec![
            "PHPSESSID=abc123; path=/".to_string(),
            "uid=42; expires=Thu, 01 Jan 2030 00:00:00 GMT".to_string(),
            "pass=deadbeef".to_string(),
        ];
        assert_eq!(extract_cookie_value(&lines, "PHPSESSID").as_deref(), Some("abc123"));
        assert_eq!(extract_cookie_value(&lines, "uid").as_deref(), Some("42"));
        assert_eq!(extract_cookie_value(&lines, "pass").as_deref(), Some("deadbeef"));
        assert_eq!(extract_cookie_value(&lines, "none"), None);
    }
}
