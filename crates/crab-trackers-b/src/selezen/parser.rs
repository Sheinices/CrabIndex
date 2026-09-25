//! Selezen release list (`/relizy-ot-selezen/`) and detail page parser.

use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::{rx, util};

use crate::common;

const TRACKER_NAME: &str = "selezen";

pub fn parse_torrents_from_list_page(html: &str) -> Vec<TorrentDetails> {
    let mut torrents = Vec::new();
    let html = tparse::replace_bad_names(html);

    for row in html.split("card overflow-hidden").skip(1) {
        if row.contains(">Аниме</a>") {
            continue;
        }
        let m = |pattern: &str| -> String {
            let res = util::html_decode(rx::group_i(row, pattern, 1).trim());
            rx::replace(&res, r"\s+", " ").trim().to_string()
        };
        if util::is_blank(row) {
            continue;
        }

        let Some(create_time) = tparse::parse_create_time(
            &m(r#"class="bx bx-calendar"></span>\s*([0-9]{2}\.[0-9]{2}\.[0-9]{4} [0-9]{2}:[0-9]{2})</a>"#),
            "dd.MM.yyyy HH:mm",
        ) else {
            continue;
        };

        let g = rx::groups(row, r#"<a href="(https?://[^"]+)"><h4 class="card-title">([^<]+)</h4>"#);
        let (url, title) = (g[1].clone(), g[2].clone());
        if util::is_blank(&url) || !url.to_lowercase().contains(".html") {
            continue;
        }

        let sid = m(r#"<i class="bx bx-chevrons-up"></i>([0-9 ]+)"#).trim().to_string();
        let pir = m(r#"<i class="bx bx-chevrons-down"></i>([0-9 ]+)"#).trim().to_string();
        let size_name = m(r#"<span class="bx bx-download"></span>([^<]+)</a>"#).trim().to_string();
        if [&title, &sid, &pir, &size_name].iter().any(|s| util::is_blank(s)) {
            continue;
        }

        let mut relased = 0;
        let (mut name, originalname);
        let g = rx::groups(&title, r"^([^/\(]+) / [^/]+ / ([^/\(]+) \(([0-9]{4})\)");
        if !util::is_blank(&g[1]) && !util::is_blank(&g[2]) && !util::is_blank(&g[3]) {
            name = g[1].clone();
            originalname = g[2].clone();
            relased = g[3].parse().unwrap_or(0);
        } else {
            let g = rx::groups(&title, r"^([^/\(]+) / ([^/\(]+) \(([0-9]{4})\)");
            name = g[1].clone();
            originalname = g[2].clone();
            if let Ok(y) = g[3].parse() {
                relased = y;
            }
        }
        if util::is_blank(&name) {
            name = common::first_title_segment(&title);
        }
        if util::is_blank(&name) {
            continue;
        }

        // Type: cartoon by genre link on the card; serial by [S01] / [01x01-02 из 09] or TVShows in title/url; else movie.
        let types: &[&str] = if row.contains(">Мульт") || row.contains(">мульт") {
            &["multfilm"]
        } else if title.to_lowercase().contains("tvshows")
            || rx::is_match(&title, r"\[S\d+\]")
            || rx::is_match(&title, r"\[\d+[xх]\d+")
            || url.to_lowercase().contains("tvshows")
        {
            &["serial"]
        } else {
            &["movie"]
        };

        let mut t = TorrentDetails::new(TRACKER_NAME, types, url, title);
        t.sid = common::parse_int(&sid);
        t.pir = common::parse_int(&pir);
        t.sizeName = size_name;
        t.createTime = create_time;
        t.name = name;
        t.originalname = originalname;
        t.relased = relased;
        torrents.push(t);
    }
    torrents
}

/// First `magnet:?xt=urn:btih:` link on a detail page ("" when missing).
pub fn extract_magnet_from_detail_page(fullnews: Option<&str>) -> Option<String> {
    let fullnews = fullnews?;
    Some(rx::group(fullnews, r#"href="(magnet:\?xt=urn:btih:[^"]+)""#, 1))
}
