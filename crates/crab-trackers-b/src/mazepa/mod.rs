//! Mazepa: login cookie, walks every category page until an empty or repeated page.
//! Magnets are built from the downloaded .torrent without trackers (announce URLs carry a passkey).

pub mod parser;

use axum::routing::get;
use axum::Router;
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log};
use crab_core::trackers::{self, ParseLock};
use crab_core::{conf, time};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::common;

pub const TRACKER_NAME: &str = "mazepa";
const COOKIE_KEY: &str = "cron:MazepaController:Cookie";

pub const CATEGORIES: [(&str, &[&str]); 27] = [
    // Українські фільми
    ("37", &["movie"]),
    ("7", &["movie"]),
    // Фільми
    ("175", &["movie"]),
    ("147", &["movie"]),
    ("12", &["movie"]),
    ("13", &["movie"]),
    ("174", &["movie"]),
    // Українські серіали
    ("38", &["serial"]),
    ("8", &["serial"]),
    // Серіали
    ("152", &["serial"]),
    ("44", &["serial"]),
    ("14", &["serial"]),
    // Українські мультфільми
    ("35", &["multfilm"]),
    ("5", &["multfilm"]),
    // Мультфільми
    ("155", &["multfilm"]),
    ("41", &["multfilm"]),
    ("10", &["multfilm"]),
    // Українські мультсеріали
    ("36", &["multserial"]),
    ("6", &["multserial"]),
    // Мультсеріали
    ("43", &["multserial"]),
    ("11", &["multserial"]),
    // Аніме
    ("16", &["anime"]),
    // Українські документальні
    ("39", &["documovie"]),
    ("9", &["documovie"]),
    // Документальні
    ("157", &["documovie"]),
    ("42", &["documovie"]),
    ("15", &["documovie"]),
];

static PARSE_LOCK: ParseLock = ParseLock::new();

fn cookie() -> Option<String> {
    common::cache_get(COOKIE_KEY)
}

async fn check_login() -> bool {
    if cookie().is_some() {
        return true;
    }
    take_login().await
}

async fn take_login() -> bool {
    let c = conf();
    let host = c.Mazepa.host.clone();
    if host.is_empty() {
        return false;
    }
    let Some(client) = common::login_client(10) else { return false };
    let form = [
        ("login_username", c.Mazepa.login.u.clone().unwrap_or_default()),
        ("login_password", c.Mazepa.login.p.clone().unwrap_or_default()),
        ("autologin", "on".into()),
        ("redirect", "/index.php".into()),
        ("login", "Увійти".into()),
    ];
    match client.post(format!("{host}/login.php")).header("User-Agent", "Mozilla/5.0").form(&form).send().await {
        Ok(resp) => {
            let cookies = common::set_cookies(resp.headers());
            if !cookies.is_empty() {
                let cookie_str = cookies.iter().map(|c| c.split(';').next().unwrap_or("")).collect::<Vec<_>>().join("; ");
                if cookie_str.contains("bb_") {
                    common::cache_set(COOKIE_KEY, cookie_str, Duration::from_secs(2 * 3600));
                    parser_log::write(TRACKER_NAME, "Login OK");
                    return true;
                }
            }
        }
        Err(e) => parser_log::write(TRACKER_NAME, format!("Login error: {e}")),
    }
    false
}

pub async fn parse() -> String {
    if conf().Mazepa.host.is_empty() {
        return trackers::DISABLED_RESULT.into();
    }

    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async {
        if !check_login().await {
            return "login error".to_string();
        }

        let sw = Instant::now();
        let mut total = 0;
        let host = conf().Mazepa.host.clone();

        for (cat, types) in CATEGORIES {
            let mut start = 0;
            let mut page = 1;
            let mut last_signature: Option<String> = None;
            loop {
                let url = format!("{host}/viewforum.php?f={cat}&start={start}");
                parser_log::write(TRACKER_NAME, format!("Parsing forum {cat} (page {page})"));

                let (found, added, signature) = parse_category(&url, types, &host).await;
                parser_log::write(TRACKER_NAME, format!("Found {found} topics, added {added}"));
                if found == 0 {
                    break;
                }
                if signature == last_signature {
                    parser_log::write(TRACKER_NAME, format!("DUPLICATE PAGE → STOP at {page}"));
                    break;
                }
                last_signature = signature;
                total += added;
                start += 50;
                page += 1;
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
        }

        parser_log::write(TRACKER_NAME, format!("Finished: {total} in {}", common::fmt_elapsed(sw.elapsed())));
        format!("ok {total}")
    })
    .await
}

async fn parse_category(url: &str, types: &[&str], host: &str) -> (i32, i32, Option<String>) {
    let Some(html) = net::get(url, &Req::new().cookie_opt(cookie())).await.filter(|h| !h.is_empty()) else {
        return (0, 0, None);
    };
    let list = parser::parse_torrents_from_category_page(&html, types, host);
    if list.is_empty() {
        return (0, 0, None);
    }
    let signature = list.iter().take(5).map(|x| x.t.url.as_str()).collect::<Vec<_>>().join(",");
    let found = list.len() as i32;

    let added = AtomicI32::new(0);
    let added_ref = &added;
    common::add_or_update_async(list, common::by_url, |mut t, cached| async move {
        if let Some(c) = cached.as_ref() {
            if c.title == t.t.title && !c.magnet.is_empty() {
                t.t.magnet = c.magnet.clone();
                if !time::is_min(&c.createTime) {
                    t.t.createTime = c.createTime;
                }
                return Some(t);
            }
        }

        let req = Req::new().timeout(30).cookie_opt(cookie()).referer(host.to_string());
        let file = net::download(&format!("{host}/dl.php?id={}", t.download_id), &req).await;
        // Announce URLs embed the user's passkey, so trackers are stripped.
        let magnet = file.as_deref().and_then(bencode::magnet_no_trackers)?;
        t.t.magnet = magnet;

        match cached {
            Some(existing) => {
                if !time::is_min(&existing.createTime) {
                    t.t.createTime = existing.createTime;
                }
            }
            None => {
                added_ref.fetch_add(1, Ordering::SeqCst);
            }
        }
        Some(t)
    })
    .await;

    (found, added.load(Ordering::SeqCst), Some(signature))
}

async fn h_parse() -> String {
    parse().await
}

pub fn router() -> Router {
    Router::new().route("/cron/mazepa/parse", get(h_parse).post(h_parse))
}
