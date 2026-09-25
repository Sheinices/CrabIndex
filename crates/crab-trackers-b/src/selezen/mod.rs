//! Selezen: DLE site behind a login; list pages `/relizy-ot-selezen/page/{n}/`, magnet from each detail page.

pub mod parser;

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use crab_core::fdb::ShardMap;
use crab_core::models::TorrentDetails;
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log;
use crab_core::trackers::{self, ParseLock};
use crab_core::{conf, rx, util};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::common;

pub const TRACKER_NAME: &str = "selezen";
const COOKIE_KEY: &str = "selezen:cookie";
const AUTH_KEY: &str = "selezen:TakeLogin()";
const SELEZEN_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const ID_RE: &str = r"/relizy-ot-selezen/(\d+)-";

static PARSE_LOCK: ParseLock = ParseLock::new();
static LOGIN_BUSY: AtomicBool = AtomicBool::new(false);

fn kv(pairs: &[(&str, String)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Minimal GET headers (Origin / Sec-Fetch-* can trigger the WAF).
fn selezen_headers() -> Vec<(String, String)> {
    vec![
        ("Accept".into(), "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".into()),
        ("Accept-Language".into(), "en-US,en;q=0.9".into()),
    ]
}

fn cookie() -> Option<String> {
    common::cache_get(COOKIE_KEY)
}

struct LoginRelease;
impl Drop for LoginRelease {
    fn drop(&mut self) {
        LOGIN_BUSY.store(false, Ordering::SeqCst);
    }
}

/// Log in and cache the PHPSESSID cookie; concurrent calls are rejected, not queued.
async fn take_login() -> bool {
    if LOGIN_BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        parser_log::write_kv(TRACKER_NAME, "TakeLogin skipped", &kv(&[("reason", "login already in progress".into())]));
        return false;
    }
    let _release = LoginRelease;

    if common::cache_has(AUTH_KEY) {
        return false;
    }
    common::cache_set(AUTH_KEY, "0", Duration::from_secs(2 * 60));

    let c = conf();
    let host = c.Selezen.host.trim_end_matches('/').to_string();
    let (u, p) = (c.Selezen.login.u.clone().unwrap_or_default(), c.Selezen.login.p.clone().unwrap_or_default());
    if util::is_blank(&u) || util::is_blank(&p) {
        parser_log::write_kv(TRACKER_NAME, "TakeLogin failed", &kv(&[("reason", "credentials not configured".into())]));
        return false;
    }

    let form = [("login_name", u.clone()), ("login_password", p), ("login_not_save", "1".into()), ("login", "submit".into())];
    // Behind Cloudflare the plain POST gets 403: skip it once the host is known to be guarded.
    let guarded = url::Url::parse(&host).ok().and_then(|x| x.host_str().map(net::cf::is_guarded)).unwrap_or(false);
    if guarded || !take_login_direct(&host, &form).await {
        if let Some(cookie) = take_login_browser(&host, &u, &form).await {
            common::cache_set(COOKIE_KEY, cookie, Duration::from_secs(24 * 3600));
            parser_log::write_kv(TRACKER_NAME, "TakeLogin success", &kv(&[("host", host), ("via", "browser".into())]));
            return true;
        }
        return false;
    }
    true
}

/// Plain POST login. `true` = logged in (cookie cached); `false` = blocked (403/503 or network
/// error, worth retrying in the browser) or rejected.
async fn take_login_direct(host: &str, form: &[(&str, String); 4]) -> bool {
    let Some(client) = common::login_client(15) else { return false };
    let resp = client
        .post(host)
        .header("User-Agent", SELEZEN_USER_AGENT)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Referer", format!("{host}/"))
        .header("Origin", host)
        .form(form)
        .send()
        .await;

    match resp {
        Ok(resp) => {
            let sess = common::set_cookies(resp.headers())
                .into_iter()
                .filter(|l| !util::is_blank(l) && l.contains("PHPSESSID="))
                .map(|l| rx::group(&l, "PHPSESSID=([^;]+)(;|$)", 1))
                .last()
                .unwrap_or_default();
            if !util::is_blank(&sess) {
                common::cache_set(COOKIE_KEY, format!("PHPSESSID={sess}; _ym_isad=2;"), Duration::from_secs(24 * 3600));
                parser_log::write_kv(TRACKER_NAME, "TakeLogin success", &kv(&[("host", host.to_string())]));
                return true;
            }
            parser_log::write_kv(
                TRACKER_NAME,
                "TakeLogin failed",
                &kv(&[("reason", "no PHPSESSID in response".into()), ("statusCode", resp.status().as_u16().to_string())]),
            );
        }
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "request" };
            parser_log::write_kv(TRACKER_NAME, "TakeLogin error", &kv(&[("message", e.to_string()), ("type", kind.into())]));
        }
    }
    false
}

/// DLE login form submitted from the FlareSolverr session of the host (passes Cloudflare).
/// Success = the returned page shows `>{login}<`; the session cookies come from the browser jar.
async fn take_login_browser(host: &str, login: &str, form: &[(&str, String); 4]) -> Option<String> {
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form.iter().map(|(k, v)| (*k, v.as_str()))).finish();
    let Some(r) = net::cf::post_form(&format!("{host}/"), &body).await else {
        parser_log::write_kv(TRACKER_NAME, "TakeLogin failed", &kv(&[("reason", "browser login: FlareSolverr disabled or unavailable".into())]));
        return None;
    };
    let cookie = r
        .cookies
        .iter()
        .filter(|(k, _)| matches!(k.as_str(), "PHPSESSID" | "dle_user_id" | "dle_password" | "dle_newpm"))
        .map(|(k, v)| format!("{k}={v}; "))
        .collect::<String>();
    if r.body.contains(&format!(">{login}<")) && cookie.contains("PHPSESSID=") {
        return Some(cookie.trim_end().to_string());
    }
    let reason = if r.body.contains("Ошибка авторизации") || r.body.contains("login_name") {
        "browser login: site rejected login/password (login.u is the site nickname)"
    } else {
        "browser login: login not found in response"
    };
    parser_log::write_kv(TRACKER_NAME, "TakeLogin failed", &kv(&[("reason", reason.into()), ("statusCode", r.status.to_string())]));
    None
}

/// Parse list pages `parse_from..=parse_to` (both 0 → page 1 only).
pub async fn parse(parse_from: i32, parse_to: i32) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, true, || async move {
        let sw = Instant::now();
        let c = conf();
        let base_url = format!("{}/relizy-ot-selezen/", c.Selezen.host);
        let mut start_page = if parse_from > 0 { parse_from } else { 1 };
        let mut end_page = if parse_to > 0 { parse_to } else if parse_from > 0 { parse_from } else { 1 };
        if start_page > end_page {
            std::mem::swap(&mut start_page, &mut end_page);
        }

        parser_log::write_kv(
            TRACKER_NAME,
            "Starting parse",
            &kv(&[
                ("parseFrom", parse_from.to_string()),
                ("parseTo", parse_to.to_string()),
                ("startPage", start_page.to_string()),
                ("endPage", end_page.to_string()),
                ("baseUrl", base_url.clone()),
            ]),
        );

        let (mut tp, mut ta, mut tu, mut ts, mut tf) = (0, 0, 0, 0, 0);
        for page in start_page..=end_page {
            if page > start_page {
                tokio::time::sleep(Duration::from_millis(conf().Selezen.parse_delay().max(0) as u64)).await;
            }
            if page > 1 {
                parser_log::write_kv(
                    TRACKER_NAME,
                    "Parsing page",
                    &kv(&[("page", page.to_string()), ("url", format!("{}/relizy-ot-selezen/page/{page}/", conf().Selezen.host))]),
                );
            }
            let (p, a, u, s, f) = parse_page(page).await;
            tp += p;
            ta += a;
            tu += u;
            ts += s;
            tf += f;
        }

        parser_log::write_kv(
            TRACKER_NAME,
            "Parse completed successfully",
            &kv(&[
                ("tookSec", sw.elapsed().as_secs_f64().to_string()),
                ("parsed", tp.to_string()),
                ("added", ta.to_string()),
                ("updated", tu.to_string()),
                ("skipped", ts.to_string()),
                ("failed", tf.to_string()),
            ]),
        );
        "ok".to_string()
    })
    .await
}

/// Cached row by exact url, else by the numeric release id among this tracker's rows.
fn lookup(db: &ShardMap, t: &TorrentDetails) -> Option<TorrentDetails> {
    if let Some(c) = db.get(&t.url) {
        return Some(c.clone());
    }
    let id = rx::group(&t.url, ID_RE, 1);
    if id.is_empty() {
        return None;
    }
    db.iter()
        .filter(|(_, v)| v.trackerName.eq_ignore_ascii_case(TRACKER_NAME))
        .find(|(k, _)| {
            let m = rx::group(k, ID_RE, 1);
            !m.is_empty() && m == id
        })
        .map(|(_, v)| v.clone())
}

async fn parse_page(page: i32) -> (i32, i32, i32, i32, i32) {
    let c = conf();
    if cookie().is_none() && c.Selezen.cookie.as_deref().unwrap_or("").is_empty() && !take_login().await {
        return (0, 0, 0, 0, 0);
    }

    // A fresh login beats the pasted cookie (it may be stale or bound to another IP).
    let cookie = cookie().or_else(|| c.Selezen.cookie.clone().filter(|s| !s.trim().is_empty()));
    let host = c.Selezen.host.trim_end_matches('/').to_string();
    let list_url = if page <= 1 { format!("{host}/relizy-ot-selezen/") } else { format!("{host}/relizy-ot-selezen/page/{page}/") };
    let req = Req::new()
        .cookie_opt(cookie.clone())
        .referer(format!("{host}/"))
        .headers(selezen_headers())
        .timeout(15)
        .useproxy(c.Selezen.useproxy);

    let (html, resp) = net::http::base_get(&list_url, &req).await;
    let html = match html {
        Some(h) if h.contains("dle_root") => h,
        other => {
            let reason = if other.is_some() {
                "invalid content".to_string()
            } else if resp.status == 500 && resp.headers.is_empty() {
                "null response".to_string()
            } else {
                let phrase = reqwest::StatusCode::from_u16(resp.status).ok().and_then(|s| s.canonical_reason()).unwrap_or("");
                format!("HTTP {} {phrase}", resp.status)
            };
            parser_log::write_kv(TRACKER_NAME, "Page parse failed", &kv(&[("page", page.to_string()), ("url", list_url), ("reason", reason)]));
            return (0, 0, 0, 0, 0);
        }
    };

    if !html.contains(&format!(">{}<", c.Selezen.login_u())) {
        // Not logged in (expired session or a pasted cookie that does not work): log in again,
        // the next run uses the fresh cookie.
        common::cache_remove(COOKIE_KEY);
        take_login().await;
        parser_log::write_kv(TRACKER_NAME, "Page parse failed", &kv(&[("page", page.to_string()), ("reason", "login not found in response".into())]));
        return (0, 0, 0, 0, 0);
    }

    let torrents = parser::parse_torrents_from_list_page(&html);
    let parsed = torrents.len() as i32;
    let (added, updated, skipped, failed) = (AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0));

    if !torrents.is_empty() {
        let counters = (&added, &updated, &skipped, &failed);
        let req = &req;
        common::add_or_update_async(torrents, lookup, |mut t, cached| async move {
            let (added, updated, skipped, failed) = counters;
            let fullnews = net::get(&t.url, req).await;
            if let Some(magnet) = parser::extract_magnet_from_detail_page(fullnews.as_deref()).filter(|m| !util::is_blank(m)) {
                t.magnet = magnet.clone();
                match cached {
                    Some(tc) => {
                        if tc.magnet.is_empty() || !tc.magnet.eq_ignore_ascii_case(&magnet) {
                            updated.fetch_add(1, Ordering::SeqCst);
                            parser_log::write_updated(TRACKER_NAME, &t, Some("magnet"));
                        } else {
                            skipped.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    None => {
                        added.fetch_add(1, Ordering::SeqCst);
                        parser_log::write_added(TRACKER_NAME, &t);
                    }
                }
                return Some(t);
            }
            failed.fetch_add(1, Ordering::SeqCst);
            parser_log::write_failed(TRACKER_NAME, &t, Some("no magnet"));
            None
        })
        .await;
    }

    let res = (parsed, added.load(Ordering::SeqCst), updated.load(Ordering::SeqCst), skipped.load(Ordering::SeqCst), failed.load(Ordering::SeqCst));
    if parsed > 0 {
        parser_log::write_kv(
            TRACKER_NAME,
            "Page completed",
            &kv(&[
                ("page", page.to_string()),
                ("parsed", res.0.to_string()),
                ("added", res.1.to_string()),
                ("updated", res.2.to_string()),
                ("skipped", res.3.to_string()),
                ("failed", res.4.to_string()),
            ]),
        );
    }
    res
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ParseQ {
    parsefrom: Option<String>,
    parseto: Option<String>,
}

async fn h_parse(Query(q): Query<ParseQ>) -> String {
    parse(common::q_int(&q.parsefrom, 0), common::q_int(&q.parseto, 0)).await
}

pub fn router() -> Router {
    Router::new().route("/cron/selezen/parse", get(h_parse).post(h_parse))
}
