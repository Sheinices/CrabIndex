//! AnimeLayer listing parser (`/torrents/anime/` pages).

use chrono::Datelike;

use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::util::{html_decode, is_blank};
use crab_core::{rx, time};

pub const TRACKER: &str = "animelayer";

fn m(row: &str, pattern: &str, index: usize) -> String {
    let res = rx::group_i(row, pattern, index);
    let res = rx::replace(res.trim(), "[\n\r\t\u{a0}]+", " ");
    res.trim().to_string()
}

/// Parse the listing into rows (magnet/size are filled later from the .torrent download).
pub fn parse_torrent_list_from_html(html: &str, base_host: &str, page: i32) -> Vec<TorrentDetails> {
    let mut torrents = Vec::new();
    let decoded = tparse::replace_bad_names(&html_decode(&html.replace("&nbsp;", "")));
    for row in decoded.split("class=\"torrent-item torrent-item-medium panel\"").skip(1) {
        if is_blank(row) {
            continue;
        }

        let parsed = if rx::is_match(row, "(Добавл|Обновл)[^<]+</span>[0-9]+ [^ ]+ [0-9]{4}") {
            tparse::parse_create_time(&m(row, ">(Добавл|Обновл)[^<]+</span>([0-9]+ [^ ]+ [0-9]{4})", 2), "dd.MM.yyyy")
        } else {
            let date = m(row, "(Добавл|Обновл)[^<]+</span>([^\n]+) в", 2);
            if is_blank(&date) {
                continue;
            }
            tparse::parse_create_time(&format!("{date} {}", chrono::Local::now().year()), "dd.MM.yyyy")
        };
        let create_time = match parsed {
            Some(t) => t,
            None => {
                if page != 1 {
                    continue;
                }
                time::now()
            }
        };

        let g = rx::groups(row, "<a href=\"/(torrent/[a-z0-9]+)/?\">([^<]+)</a>");
        let url_path = g[1].clone();
        let mut title = g[2].clone();

        let sid_s = m(row, "class=\"icon s-icons-upload\"></i>([0-9]+)", 1);
        let pir_s = m(row, "class=\"icon s-icons-download\"></i>([0-9]+)", 1);
        let size_name = m(
            row,
            r#"s-icons-download"></i>\s*[0-9]+\s*<span[^>]*>[\s\S]*?</span>\s*([0-9]+(?:[\.,][0-9]+)?\s*(?:[KMGT]B|[КМГТ]Б))"#,
            1,
        )
        .replace(',', ".");

        if is_blank(&url_path) || is_blank(&title) {
            continue;
        }

        if rx::is_match(row, "Разрешение: ?</strong>1920x1080") {
            title.push_str(" [1080p]");
        } else if rx::is_match(row, "Разрешение: ?</strong>1280x720") {
            title.push_str(" [720p]");
        }

        let full_url = format!("{base_host}/{url_path}/");

        let mut name = String::new();
        let mut originalname = String::new();
        let g = rx::groups(&title, r"([^/\[\(]+)\([0-9]{4}\)[^/]+/([^/\[\(]+)");
        if !is_blank(&g[1]) && !is_blank(&g[2]) {
            name = g[2].trim().to_string();
            originalname = g[1].trim().to_string();
        } else {
            let g = rx::groups(&title, r"^([^/\[\(]+)/([^/\[\(]+)");
            if !is_blank(&g[1]) && !is_blank(&g[2]) {
                name = g[2].trim().to_string();
                originalname = g[1].trim().to_string();
            }
        }

        let relased: i32 = match m(row, "Год выхода: ?</strong>([0-9]{4})", 1).parse() {
            Ok(y) if y != 0 => y,
            _ => continue,
        };

        if is_blank(&name) {
            name = rx::split_i(&title, r"(\[|\/|\(|\|)").into_iter().next().unwrap_or_default().trim().to_string();
        }

        if !is_blank(&name) {
            torrents.push(TorrentDetails {
                trackerName: TRACKER.into(),
                types: vec!["anime".into()],
                url: full_url,
                title,
                sid: sid_s.parse().unwrap_or(0),
                pir: pir_s.parse().unwrap_or(0),
                sizeName: size_name,
                createTime: create_time,
                name,
                originalname,
                relased,
                ..Default::default()
            });
        }
    }
    torrents
}
