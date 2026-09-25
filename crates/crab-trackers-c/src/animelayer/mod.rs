//! AnimeLayer: authorised listing parse (static cookie or login → `layer_hash`/`layer_id` cookie),
//! magnets from the `.torrent` download.
//!
//! Routes: `/cron/animelayer/takelogin`, `/cron/animelayer/parse?parsefrom&parseto`.

pub mod parser;

use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use once_cell::sync::Lazy;
use serde::Deserialize;

use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock};
use crab_core::util::is_blank;
use crab_core::{conf, fdb, rx};

use crate::common::{b, cached, group_by_key, int_param, kv, login_client, read_capped, secs, Expiring};
use parser::TRACKER;

static PARSE_LOCK: ParseLock = ParseLock::new();
static COOKIE_CACHE: Expiring = Expiring::new();
static LOGIN_SEMAPHORE: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
const COOKIE_TTL: Duration = Duration::from_secs(24 * 3600);

type Counts = (i32, i32, i32, i32, i32);

/// Force `https://` on the configured host.
pub fn ensure_https(host: &str) -> String {
    if is_blank(host) {
        return host.to_string();
    }
    let lower = host.to_ascii_lowercase();
    if lower.starts_with("http://") {
        return format!("https://{}", &host[7..]);
    }
    if !lower.starts_with("https://") {
        return format!("https://{host}");
    }
    host.to_string()
}

fn base_host() -> String {
    ensure_https(&conf().Animelayer.host)
}

fn useproxy() -> bool {
    conf().Animelayer.useproxy
}

fn login_creds() -> Option<(String, String)> {
    let c = conf();
    let u = c.Animelayer.login.u.clone().unwrap_or_default();
    let p = c.Animelayer.login.p.clone().unwrap_or_default();
    if is_blank(&u) || is_blank(&p) {
        None
    } else {
        Some((u, p))
    }
}

fn static_cookie() -> Option<String> {
    conf().Animelayer.cookie.clone().filter(|c| !is_blank(c))
}

fn log_kv(msg: &str, data: Vec<(String, String)>) {
    parser_log::write_kv(TRACKER, msg, &data);
}

fn invalidate_cookie() {
    COOKIE_CACHE.remove();
    log_kv("Cookie invalidated", kv!("reason" => "likely expired during parsing"));
}

fn has_anonymous_markers(html: &str) -> bool {
    !is_blank(html) && (html.contains("id=\"loginForm\"") || html.contains("/auth/login/") || html.contains("/auth/register/"))
}

/// First non-whitespace byte of the first 64 is `<`.
pub fn looks_like_html(data: Option<&[u8]>) -> bool {
    let Some(data) = data else { return false };
    for &b in data.iter().take(64) {
        if (b as char).is_whitespace() {
            continue;
        }
        return b == b'<';
    }
    false
}

async fn validate_cookie(cookie: &str) -> bool {
    if is_blank(cookie) {
        return false;
    }
    let test_url = format!("{}/torrents/anime/", base_host());
    let Some(html) = net::get(&test_url, &Req::new().cookie(cookie).useproxy(useproxy()).http2()).await else {
        log_kv("Cookie validation failed", kv!("reason" => "null response"));
        return false;
    };
    let has_wrapper = html.contains("id=\"wrapper\"");
    let is_valid = has_wrapper && !has_anonymous_markers(&html);
    log_kv(
        "Cookie validation",
        kv!("isValid" => b(is_valid), "hasWrapper" => b(has_wrapper), "hasLoginForm" => b(has_anonymous_markers(&html))),
    );
    is_valid
}

fn http_status_name(code: u16) -> String {
    reqwest::StatusCode::from_u16(code)
        .ok()
        .and_then(|s| s.canonical_reason().map(|r| r.replace(' ', "")))
        .unwrap_or_else(|| code.to_string())
}

/// POST `/auth/login/` and cache `layer_hash;layer_id[;PHPSESSID]` for a day.
pub async fn take_login() -> bool {
    let Ok(_guard) = LOGIN_SEMAPHORE.try_lock() else {
        log_kv("TakeLogin skipped", kv!("reason" => "login already in progress"));
        return false;
    };
    let Some((user, pass)) = login_creds() else {
        log_kv("TakeLogin failed", kv!("reason" => "credentials not configured"));
        return false;
    };
    match take_login_inner(&user, &pass).await {
        Ok(v) => v,
        Err((msg, ty)) => {
            log_kv("TakeLogin error", kv!("message" => msg, "type" => ty, "stackTrace" => ""));
            false
        }
    }
}

async fn take_login_inner(user: &str, pass: &str) -> Result<bool, (String, String)> {
    let client = login_client(10).ok_or_else(|| ("client build failed".to_string(), "http".to_string()))?;
    let config_host = conf().Animelayer.host.clone();
    let base = ensure_https(&config_host);
    let login_url = format!("{base}/auth/login/");
    log_kv("Attempting login", kv!("url" => login_url, "configHost" => config_host, "resolvedHost" => base, "user" => user));

    let resp = client
        .post(&login_url)
        .header("user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .header("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("accept-language", "en-US,en;q=0.5")
        .form(&[("login", user), ("password", pass)])
        .send()
        .await
        .map_err(|e| (e.to_string(), if e.is_timeout() { "timeout" } else { "http" }.to_string()))?;

    let status = resp.status().as_u16();
    log_kv("Login response received", kv!("statusCode" => status, "status" => http_status_name(status)));

    let all_cookies: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|h| h.split(", ").map(|p| p.trim().to_string()).collect::<Vec<_>>())
        .filter(|p| !is_blank(p))
        .collect();

    if (300..400).contains(&status) && all_cookies.is_empty() {
        let location = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("none").to_string();
        log_kv("Redirect response but no cookies found", kv!("statusCode" => status, "location" => location));
    }

    if !all_cookies.is_empty() {
        log_kv(
            "Cookies found in response",
            kv!("cookieCount" => all_cookies.len(), "cookies" => all_cookies.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")),
        );
        let (mut layer_hash, mut layer_id, mut phpsessid) = (String::new(), String::new(), String::new());
        for line in all_cookies.iter().filter(|c| !is_blank(c)) {
            if line.contains("layer_hash=") {
                let v = rx::group(line, "layer_hash=([^;]+)(;|$)", 1);
                if !v.is_empty() {
                    layer_hash = v;
                }
            }
            if line.contains("layer_id=") {
                let v = rx::group(line, "layer_id=([^;]+)(;|$)", 1);
                if !v.is_empty() {
                    layer_id = v;
                }
            }
            if line.contains("PHPSESSID=") {
                let v = rx::group(line, "PHPSESSID=([^;]+)(;|$)", 1);
                if !v.is_empty() {
                    phpsessid = v;
                }
            }
        }
        if !is_blank(&layer_hash) && !is_blank(&layer_id) {
            let mut value = format!("layer_hash={layer_hash};layer_id={layer_id}");
            if !is_blank(&phpsessid) {
                value.push_str(&format!(";PHPSESSID={phpsessid}"));
            }
            COOKIE_CACHE.set(value, COOKIE_TTL);
            log_kv(
                "TakeLogin successful",
                kv!("user" => user, "hasLayerHash" => b(true), "hasLayerId" => b(true), "hasPhpSessId" => b(!is_blank(&phpsessid))),
            );
            return Ok(true);
        }
        log_kv(
            "TakeLogin failed - missing required cookies",
            kv!("hasLayerHash" => b(!is_blank(&layer_hash)), "hasLayerId" => b(!is_blank(&layer_id)), "cookieLines" => all_cookies.join(" | ")),
        );
    } else {
        let body = match read_capped(resp, 2_000_000).await {
            Ok(mut s) => {
                if s.chars().count() > 500 {
                    s = format!("{}...", s.chars().take(500).collect::<String>());
                }
                s
            }
            Err(e) => {
                log_kv("Failed to read response body", kv!("statusCode" => status, "message" => e, "type" => "http"));
                String::new()
            }
        };
        log_kv(
            "TakeLogin failed - no cookies in response",
            kv!("statusCode" => status, "hasResponseBody" => b(!is_blank(&body)), "responsePreview" => body),
        );
    }
    Ok(false)
}

/// Authorise (static cookie → login) and parse listing pages `[parse_from, parse_to]`.
pub async fn parse(parse_from: i32, parse_to: i32) -> String {
    // authorization
    let mut need_login = false;
    match static_cookie() {
        None => need_login = true,
        Some(c) => {
            let c = c.trim().to_string();
            log_kv("Using static cookie from config", kv!());
            if validate_cookie(&c).await {
                COOKIE_CACHE.set(c, COOKIE_TTL);
            } else {
                log_kv("Static cookie is invalid", kv!("reason" => "anonymous page markers found"));
                need_login = true;
            }
        }
    }

    if need_login {
        if login_creds().is_some() {
            if take_login().await {
                let Some(cookie) = COOKIE_CACHE.get().filter(|c| !is_blank(c)) else {
                    log_kv("Authorization failed", kv!("reason" => "login succeeded but no cookie retrieved"));
                    return "work_login".into();
                };
                if !validate_cookie(&cookie).await {
                    log_kv("Authorization failed", kv!("reason" => "login cookie validation failed"));
                    invalidate_cookie();
                    return "work_login".into();
                }
            } else {
                log_kv("Authorization failed", kv!("reason" => "login failed"));
                return "work_login".into();
            }
        } else {
            log_kv("Authorization failed", kv!("reason" => "no cookie or credentials provided"));
            return "Failed to authorize, please provide either cookie or credentials".into();
        }
    }

    trackers::run_parse(TRACKER, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let base_url = base_host();
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
                tokio::time::sleep(Duration::from_millis(conf().Animelayer.parse_delay().max(0) as u64)).await;
            }
            if page > 1 {
                log_kv("Parsing page", kv!("page" => page, "url" => format!("{base_url}/torrents/anime/?page={page}")));
            }
            let r = parse_page_with_retry(page).await;
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

async fn parse_page_with_retry(page: i32) -> Counts {
    let Some(cookie) = COOKIE_CACHE.get().filter(|c| !is_blank(c)) else {
        log_kv("Page parse failed - no cookie", kv!("page" => page));
        return (0, 0, 0, 0, 0);
    };
    let result = parse_page(page, &cookie).await;
    if result.0 > 0 {
        return result;
    }
    log_kv("Page parse returned zeros, attempting cookie refresh", kv!("page" => page, "retryAttempt" => 1));
    invalidate_cookie();

    let new_cookie: Option<String>;
    if let Some(sc) = static_cookie() {
        log_kv("Using static cookie from config", kv!());
        let sc = sc.trim().to_string();
        if !validate_cookie(&sc).await {
            log_kv("Static cookie is invalid, aborting page parse", kv!("page" => page, "reason" => "anonymous page markers found"));
            return (0, 0, 0, 0, 0);
        }
        COOKIE_CACHE.set(sc.clone(), COOKIE_TTL);
        new_cookie = Some(sc);
    } else if login_creds().is_some() {
        if take_login().await {
            new_cookie = COOKIE_CACHE.get();
            log_kv("Re-login successful", kv!());
        } else {
            log_kv("Re-login failed, aborting page parse", kv!("page" => page));
            return (0, 0, 0, 0, 0);
        }
    } else {
        log_kv("No way to refresh cookie, aborting", kv!());
        return (0, 0, 0, 0, 0);
    }

    match new_cookie.filter(|c| !is_blank(c)) {
        Some(c) => parse_page(page, &c).await,
        None => (0, 0, 0, 0, 0),
    }
}

async fn parse_page(page: i32, cookie: &str) -> Counts {
    let base = base_host();
    let url = format!("{base}/torrents/anime/{}", if page > 1 { format!("?page={page}") } else { String::new() });
    let Some(html) = net::get(&url, &Req::new().cookie(cookie).useproxy(useproxy()).http2()).await else {
        log_kv("Page parse failed", kv!("page" => page, "url" => url, "reason" => "null response"));
        return (0, 0, 0, 0, 0);
    };
    if !html.contains("id=\"wrapper\"") {
        let is_login_form = html.contains("id=\"loginForm\"") || html.contains("/auth/login/");
        log_kv("Page parse failed", kv!("page" => page, "url" => url, "reason" => "invalid content", "likelyExpiredCookie" => b(is_login_form)));
        return (0, 0, 0, 0, 0);
    }

    let torrents = parser::parse_torrent_list_from_html(&html, &base, page);
    let parsed = torrents.len() as i32;
    let (mut added, mut updated, mut skipped, mut failed) = (0, 0, 0, 0);

    for (key, group) in group_by_key(torrents) {
        let w = fdb::open_write(&key);
        for mut t in group {
            let existing = cached(&w, &t.url);
            if let Some(c) = existing.as_ref().filter(|c| c.title == t.title) {
                skipped += 1;
                parser_log::write_skipped(TRACKER, c, Some("no changes"));
                w.add_or_update(&t);
                continue;
            }
            let r = Req::new()
                .cookie(cookie)
                .referer(t.url.clone())
                .header("accept", "application/x-bittorrent,application/octet-stream,*/*")
                .useproxy(useproxy())
                .timeout(30);
            let torrent = net::download(&format!("{}download/", t.url), &r).await;
            if looks_like_html(torrent.as_deref()) {
                failed += 1;
                parser_log::write_failed(TRACKER, &t, Some("download returned html instead of torrent; cookie is likely not authorized"));
                continue;
            }
            let data = torrent.unwrap_or_default();
            let magnet = bencode::magnet(&data).unwrap_or_default();
            let size_name = bencode::size_name(&data).unwrap_or_else(|| t.sizeName.clone());
            if !is_blank(&magnet) && !is_blank(&size_name) {
                t.magnet = magnet;
                t.sizeName = size_name;
                if existing.is_some() {
                    updated += 1;
                    parser_log::write_updated(TRACKER, &t, Some("magnet from download"));
                } else {
                    added += 1;
                    parser_log::write_added(TRACKER, &t);
                }
                w.add_or_update(&t);
                continue;
            }
            failed += 1;
            parser_log::write_failed(TRACKER, &t, Some("could not get magnet or size"));
        }
    }

    if parsed > 0 {
        log_kv(
            &format!("Page {page} completed"),
            kv!("parsed" => parsed, "added" => added, "updated" => updated, "skipped" => skipped, "failed" => failed),
        );
    }
    (parsed, added, updated, skipped, failed)
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
    Router::new()
        .route("/cron/animelayer/takelogin", get(h_take_login).post(h_take_login))
        .route("/cron/animelayer/parse", get(h_parse).post(h_parse))
}

async fn h_take_login() -> Json<bool> {
    Json(take_login().await)
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(int_param(&q.parsefrom, 0), int_param(&q.parseto, 0)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_rules() {
        assert_eq!(ensure_https("http://animelayer.ru"), "https://animelayer.ru");
        assert_eq!(ensure_https("animelayer.ru"), "https://animelayer.ru");
        assert_eq!(ensure_https("https://animelayer.ru"), "https://animelayer.ru");
    }

    #[test]
    fn html_sniff() {
        assert!(looks_like_html(Some(b"  \n<html>")));
        assert!(!looks_like_html(Some(b"d8:announce")));
        assert!(!looks_like_html(None));
    }
}
