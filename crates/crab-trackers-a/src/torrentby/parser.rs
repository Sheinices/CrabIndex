//! torrent.by listing parsing.

use chrono::{Duration, NaiveDate, TimeZone, Utc};
use tokio_util::sync::CancellationToken;

use crab_core::models::TorrentDetails;
use crab_core::net::{self, Req};
use crab_core::parsing::tparse;
use crab_core::rx;
use crab_core::trackers::{self, Cancelled};
use crab_core::{conf, fdb, time};

use super::categories::{TorrentByTitleKind, MAP};
use crate::common::{g, match_row as m, name_before_brackets, nb, parse_int, year};

const TRACKER_NAME: &str = "torrentby";

/// Fetch + parse + upsert one listing page. An empty listing (pager past the last row)
/// is still a fetched page (`Ok(true)`).
pub async fn parse_page(cat: String, page: i32, ct: CancellationToken) -> Result<bool, Cancelled> {
    let c = conf();
    let html = net::get(&format!("{}/{cat}/?page={page}", c.TorrentBy.rq_host()), &Req::new().useproxy(c.TorrentBy.useproxy).cancel(&ct)).await;
    trackers::check(&ct)?;
    let Some(html) = html.filter(|h| is_listing_page(h)) else { return Ok(false) };
    let torrents = parse_torrents_from_html(&html, &cat);
    fdb::add_or_update(&torrents);
    Ok(true)
}

/// Real listing chrome. Tiny WAF/error bodies are not listings.
pub fn is_listing_page(html: &str) -> bool {
    !html.is_empty() && (html.to_lowercase().contains("ttable_headinner") || html.contains("Страницы:"))
}

/// At least one torrent row. The pager can continue past an empty table.
pub fn has_listing_rows(html: &str) -> bool {
    !html.is_empty() && html.to_lowercase().contains("ttable_col")
}

pub fn parse_torrents_from_html(html: &str, cat: &str) -> Vec<TorrentDetails> {
    let mut torrents = Vec::new();
    let Some(meta) = MAP.get(cat) else { return torrents };

    let html = tparse::replace_bad_names(html);
    for row in html.split("<tr class=\"ttable_col").skip(1) {
        if row.trim().is_empty() || !row.contains("magnet:?xt=urn") {
            continue;
        }

        let create_time = if row.contains(">Сегодня</td>") {
            time::now()
        } else if row.contains(">Вчера</td>") {
            time::now() - Duration::days(1)
        } else {
            let raw = m(row, ">([0-9]{4}-[0-9]{2}-[0-9]{2})</td>", 1).replace('-', " ");
            match NaiveDate::parse_from_str(&raw, "%Y %m %d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)) {
                Some(n) => Utc.from_utc_datetime(&n),
                None => continue,
            }
        };
        if time::is_min(&create_time) {
            continue;
        }

        let url = m(row, "<a name=\"search_select\" [^>]+ href=\"/([0-9]+/[^\"]+)\"", 1);
        let title = m(row, "<a name=\"search_select\" [^>]+>([^<]+)</a>", 1);
        let sid = m(row, "<font color=\"green\">&uarr; ([0-9]+)</font>", 1);
        let pir = m(row, "<font color=\"red\">&darr; ([0-9]+)</font>", 1);
        let size_name = m(row, "</td><td style=\"white-space:nowrap;\">([^<]+)</td>", 1);
        let magnet = m(row, "href=\"(magnet:\\?xt=[^\"]+)\"", 1);

        if !nb(&url) || !nb(&title) || !nb(&sid) || !nb(&pir) || !nb(&size_name) || !nb(&magnet) {
            continue;
        }

        let url = format!("{}/{}", conf().TorrentBy.host, url);
        let (mut name, originalname, relased) = match meta.title_kind {
            TorrentByTitleKind::FilmsForeign => parse_films_foreign(&title),
            TorrentByTitleKind::FilmsRu => parse_films_ru(&title),
            TorrentByTitleKind::SerialForeign => parse_serial_foreign(&title),
            TorrentByTitleKind::SerialRu => parse_serial_ru(&title),
            TorrentByTitleKind::ShowLike => parse_show_like(&title),
            TorrentByTitleKind::Sport => parse_sport(&title),
        };
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

type Names = (String, String, i32);

fn none() -> Names {
    (String::new(), String::new(), 0)
}

/// name + original + year when all three groups are non-blank.
fn nameorig(title: &str, pattern: &str) -> Option<Names> {
    let x = g(title, pattern);
    (nb(&x[1]) && nb(&x[2]) && nb(&x[3])).then(|| (x[1].trim().to_string(), x[2].trim().to_string(), year(&x[3])))
}

/// name + year when both groups are non-blank.
fn nameyear(title: &str, pattern: &str) -> Option<Names> {
    let x = g(title, pattern);
    (nb(&x[1]) && nb(&x[2])).then(|| (x[1].trim().to_string(), String::new(), year(&x[2])))
}

/// Зарубежные фильмы: Name / Alt / Orig (year) … or Name / Orig (year) …
fn parse_films_foreign(title: &str) -> Names {
    nameorig(title, r"^([^/\(]+) / [^/]+ / ([^/\(]+) \(((?:19|20)[0-9]{2})\)")
        .or_else(|| nameorig(title, r"^([^/\(]+) / ([^/\(]+) \(((?:19|20)[0-9]{2})\)"))
        .unwrap_or_else(none)
}

/// Наши фильмы: Name (year) … or Name [year, genres…] …
fn parse_films_ru(title: &str) -> Names {
    nameyear(title, r"^([^/\(\[]+) \(((?:19|20)[0-9]{2})\)")
        .or_else(|| nameyear(title, r"^([^/\(\[]+)(?: / [^/\[]+)? \[((?:19|20)[0-9]{2})"))
        .unwrap_or_else(none)
}

/// Зарубежные сериалы: Name / Orig (year) … / Name / Alt / Orig [S01] (year) …
fn parse_serial_foreign(title: &str) -> Names {
    nameorig(title, r"^([^/\(\[]+) / [^/]+ / ([^/\[\(]+)(?: \[[^\]]+\])? \(((?:19|20)[0-9]{2})(?:\)|-)")
        .or_else(|| nameorig(title, r"^([^/\(\[]+) / ([^/\[\(]+)(?: \[[^\]]+\])? \(((?:19|20)[0-9]{2})(?:\)|-)"))
        .or_else(|| nameorig(title, r"^([^/\(\[]+) / ([^/\[\(]+) \[[^\]]+\] \(((?:19|20)[0-9]{2})(?:\)|-)"))
        .or_else(|| nameyear(title, r"^([^/\(\[]+)(?: \[[^\]]+\])? \(((?:19|20)[0-9]{2})(?:\)|-)"))
        .unwrap_or_else(none)
}

/// Наши сериалы: Name / Сезон: … / Серии: … [year…]  or Name [01x01…] (year)
fn parse_serial_ru(title: &str) -> Names {
    nameyear(title, r"^([^/\(\[]+) / Сезон:[^\[]+\[((?:19|20)[0-9]{2})")
        .or_else(|| nameyear(title, r"^([^/\(\[]+) \[[^\]]+\] \(((?:19|20)[0-9]{2})(?:\)|-)"))
        .or_else(|| nameyear(title, r"^([^/\(\[]+) \(((?:19|20)[0-9]{2})(?:\)|-)"))
        .unwrap_or_else(none)
}

/// tv / humor / cartoons / anime - slash+orig+(year), [Sxx] (year), or Name (year).
fn parse_show_like(title: &str) -> Names {
    if title.contains(" / ") {
        if let Some(r) = nameorig(title, r"^([^/\(\[]+) / [^/]+ / ([^/\[\(]+)(?: \[[^\]]+\])? \(((?:19|20)[0-9]{2})(?:\)|-)")
            .or_else(|| nameorig(title, r"^([^/\(\[]+) / ([^/\[\(]+)(?: \[[^\]]+\])? \(((?:19|20)[0-9]{2})(?:\)|-)"))
        {
            return r;
        }
    }
    nameyear(title, r"^([^/\(\[]+) \[[^\]]+\] \(((?:19|20)[0-9]{2})(?:\)|-)")
        .or_else(|| nameyear(title, r"^([^/\(\[]+) \(((?:19|20)[0-9]{2})(?:\)|-)"))
        .unwrap_or_else(none)
}

/// Sport titles often contain 1/2 path-like slashes; take the name before the year in parentheses
/// and drop a trailing `[dd.mm]` date marker.
fn parse_sport(title: &str) -> Names {
    let Some(c) = rx::captures(title, r"\(((?:19|20)[0-9]{2})\)") else { return none() };
    let (Some(whole), Some(y)) = (c.get(0), c.get(1)) else { return none() };
    let relased = year(y.as_str());
    let name = title[..whole.start()].trim().to_string();
    let name = rx::replace(&name, r"\s*\[[0-9]{1,2}\.[0-9]{1,2}\]\s*$", "").trim().to_string();
    (name, String::new(), relased)
}
