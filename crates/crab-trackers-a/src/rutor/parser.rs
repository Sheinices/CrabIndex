//! Rutor browse page parsing.

use crab_core::conf;
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::parsing::tparse;
use crab_core::rx;

use super::categories::{RutorTitleKind, MAP};
use crate::common::{g, match_row_nbsp as m, name_before_brackets, nb, parse_int, year};

const TRACKER_NAME: &str = "rutor";

const LAST_BROWSE_RE: &str = r#"<a href="/browse/([0-9]+)/[0-9]+/[0-9]+/[0-9]+"><b>[0-9]+&nbsp;-&nbsp;[0-9]+</b></a></p>"#;

/// Last 0-based `/browse/N/` index from the top pager (range link right before `</p>`).
pub fn last_page_from_html(html: &str) -> i32 {
    if html.trim().is_empty() {
        return 0;
    }
    match rx::captures(html, LAST_BROWSE_RE) {
        Some(c) => c.get(1).and_then(|x| x.as_str().parse::<i32>().ok()).filter(|n| *n >= 0).unwrap_or(0),
        None => 0,
    }
}

/// Real browse HTML (pager and/or gai/tum rows), not a CF interstitial / empty fetch.
pub fn looks_like_browse_listing(html: &str) -> bool {
    if html.trim().is_empty() {
        return false;
    }
    if rx::is_match(html, LAST_BROWSE_RE) {
        return true;
    }
    let l = html.to_lowercase();
    l.contains("<tr class=\"gai\">") || l.contains("<tr class=\"tum\">")
}

/// Drop map slots past the live 0-based last index (inclusive `page <= max_page`).
pub fn prune_pages_beyond_max(tasks: &mut Vec<TaskParse>, max_page: i32) -> i32 {
    crate::common::prune_pages_beyond_max(tasks, max_page)
}

pub fn parse_torrents_from_page(html: &str, cat: &str) -> Vec<TorrentDetails> {
    let mut torrents = Vec::new();
    let Some(meta) = MAP.get(cat) else { return torrents };

    let flat = rx::replace(html, "[\n\r\t]+", "");
    for row in rx::split(&flat, "<tr class=\"(gai|tum)\">").into_iter().skip(1) {
        if row.trim().is_empty() || !row.contains("magnet:?xt=urn") {
            continue;
        }

        let Some(create_time) = tparse::parse_create_time(&m(&row, "<td>([^<]+)</td><td([^>]+)?><a class=\"downgif\"", 1), "dd.MM.yy") else {
            continue;
        };

        let url = m(&row, "<a href=\"/(torrent/[^\"]+)\">", 1);
        let title = m(&row, "<a href=\"/torrent/[^\"]+\">([^<]+)</a>", 1);
        let sid = m(&row, "<span class=\"green\"><img [^>]+>&nbsp;([0-9]+)</span>", 1);
        let pir = m(&row, "<span class=\"red\">&nbsp;([0-9]+)</span>", 1);
        let size_name = m(&row, "<td align=\"right\">([^<]+)</td>", 1);
        let magnet = m(&row, "href=\"(magnet:\\?xt=[^\"]+)\"", 1);

        if !nb(&url) || !nb(&title) || title.to_lowercase().contains("трейлер") || !nb(&sid) || !nb(&pir) || !nb(&size_name) || !nb(&magnet) {
            continue;
        }
        if meta.require_ukr_in_title && !title.contains(" UKR") {
            continue;
        }
        if title.contains(" КПК") {
            continue;
        }

        let url = format!("{}/{}", conf().Rutor.host, url);
        let (mut name, originalname, relased) = parse_title_names(meta.title_kind, &title);
        if !nb(&name) {
            name = name_before_brackets(&title);
        }
        if !nb(&name) {
            continue;
        }

        let mut t = TorrentDetails::new(TRACKER_NAME, meta.types, url, title);
        t.sid = parse_int(&sid);
        t.pir = parse_int(&pir);
        t.sizeName = size_name;
        t.magnet = magnet;
        t.createTime = create_time;
        t.name = name;
        t.originalname = originalname;
        t.relased = relased;
        torrents.push(t);
    }
    torrents
}

fn parse_title_names(kind: RutorTitleKind, title: &str) -> (String, String, i32) {
    match kind {
        RutorTitleKind::ForeignMovie => parse_foreign_movie_title(title),
        RutorTitleKind::RuMovie => parse_ru_movie_title(title),
        RutorTitleKind::ForeignSerial => parse_foreign_serial_title(title),
        RutorTitleKind::RuSerial => parse_ru_serial_title(title),
        RutorTitleKind::ShowLike => parse_show_like_title(title),
    }
}

fn three_ok(x: &[String], a: usize, b: usize, c: usize) -> bool {
    nb(&x[a]) && nb(&x[b]) && nb(&x[c])
}

fn parse_foreign_movie_title(title: &str) -> (String, String, i32) {
    let x = g(title, r"^([^/]+) / ([^/]+) / ([^/\(]+) \(([0-9]{4})\)");
    if three_ok(&x, 1, 2, 3) {
        return (x[1].clone(), x[3].clone(), year(&x[4]));
    }
    let x = g(title, r"^([^/\(]+) / ([^/\(]+) \(([0-9]{4})\)");
    (x[1].clone(), x[2].clone(), year(&x[3]))
}

fn parse_ru_movie_title(title: &str) -> (String, String, i32) {
    let x = g(title, r"^([^/\(]+) \(([0-9]{4})\)");
    (x[1].clone(), String::new(), year(&x[2]))
}

fn parse_foreign_serial_title(title: &str) -> (String, String, i32) {
    let x = g(title, r"^([^/]+) / [^/]+ / [^/]+ / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
    if three_ok(&x, 1, 2, 3) {
        return (x[1].clone(), x[2].clone(), year(&x[3]));
    }
    let x = g(title, r"^([^/]+) / [^/]+ / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
    if three_ok(&x, 1, 2, 3) {
        return (x[1].clone(), x[2].clone(), year(&x[3]));
    }
    let x = g(title, r"^([^/]+) / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
    (x[1].clone(), x[2].clone(), year(&x[3]))
}

fn parse_ru_serial_title(title: &str) -> (String, String, i32) {
    let x = g(title, r"^([^/]+) \[[^\]]+\] \(([0-9]{4})(\)|-)");
    (x[1].clone(), String::new(), year(&x[2]))
}

fn parse_show_like_title(title: &str) -> (String, String, i32) {
    let brackets = title.contains('[') && title.contains(']');
    if title.contains(" / ") {
        if brackets {
            let x = g(title, r"^([^/]+) / ([^/]+) / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
            if three_ok(&x, 1, 2, 3) {
                return (x[1].clone(), x[3].clone(), year(&x[4]));
            }
            let x = g(title, r"^([^/]+) / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
            (x[1].clone(), x[2].clone(), year(&x[3]))
        } else {
            let x = g(title, r"^([^/]+) / ([^/]+) / ([^/\(]+) \(([0-9]{4})\)");
            if three_ok(&x, 1, 2, 3) {
                return (x[1].clone(), x[3].clone(), year(&x[4]));
            }
            let x = g(title, r"^([^/\(]+) / ([^/\(]+) \(([0-9]{4})\)");
            (x[1].clone(), x[2].clone(), year(&x[3]))
        }
    } else if brackets {
        let x = g(title, r"^([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
        (x[1].clone(), String::new(), year(&x[2]))
    } else {
        let x = g(title, r"^([^/\(]+) \(([0-9]{4})\)");
        (x[1].clone(), String::new(), year(&x[2]))
    }
}
