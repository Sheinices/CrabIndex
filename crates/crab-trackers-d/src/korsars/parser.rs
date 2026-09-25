//! korsars.pro - phpBB-mod tracker with inline magnets on forum listings.

use chrono::{DateTime, NaiveDateTime, Utc};

use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::{rx, time, util};

use super::categories;
use crate::common::{moscow_to_utc, try_int};

pub const TRACKER_NAME: &str = "korsars";
pub const TOPICS_PER_PAGE: i32 = 50;

const ROW_DATE_RE: &str = r"<p>([0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2})</p>";
const ROW_TOPIC_ID_RE: &str = r#"<a id="tt-([0-9]+)""#;
const ROW_TITLE_RE: &str = r#"<a id="tt-[0-9]+"[^>]+>\s*<b>([^<]+)</b>\s*</a>"#;
const ROW_SID_RE: &str = r#"<span class="seedmed"[^>]*><b>([0-9]+)</b>"#;
const ROW_PIR_RE: &str = r#"<span class="leechmed"[^>]*><b>([0-9]+)</b>"#;
const ROW_SIZE_RE: &str = r#"href="\./dl\.php\?id=[0-9]+"[^>]*>([^<]+)</a>"#;
const ROW_MAGNET_RE: &str = r#"href="(magnet:[^"]+)""#;
const PAGER_START_RE: &str = r"viewforum\.php\?f=[0-9]+(?:&amp;|&)start=([0-9]+)";
const YEAR_RE: &str = r"\(([0-9]{4})";
const TITLE_SERIAL3_RE: &str = r"^([^/\[\(]+) / [^/\[\(]+ / ([^/\[\(]+) \[S[0-9]";
const TITLE_SERIAL2_RE: &str = r"^([^/\[\(]+) / ([^/\[\(]+) \[S[0-9]";
const TITLE_SERIAL1_RE: &str = r"^([^/\[\(]+) \[S[0-9]";
const TITLE_MOVIE3_RE: &str = r"^([^/\(]+) / [^/\(]+ / ([^/\(]+) \(";
const TITLE_MOVIE2_RE: &str = r"^([^/\(]+) / ([^/\(]+) \(";
const TITLE_MOVIE1_RE: &str = r"^([^/\(]+) \(";
const FIRST_NAME_PART_RE: &str = r"(\[|/|\(|\|)";
const STRIP_TAGS_RE: &str = r"<[^>]+>";
const WHITESPACE_RE: &str = r"\s+";

pub fn forum_url(host: &str, cat: &str, page: i32) -> String {
    let mut url = format!("{}/viewforum.php?f={cat}", host.trim_end_matches('/'));
    if page > 0 {
        url.push_str(&format!("&start={}", page * TOPICS_PER_PAGE));
    }
    url
}

pub fn topic_url(host: &str, topic_id: &str) -> String {
    format!("{}/viewtopic.php?t={topic_id}", host.trim_end_matches('/'))
}

/// Zero-based last page from the largest `?start=N` pager link (steps of 50).
pub fn last_page_from_html(body: &str) -> i32 {
    if util::is_blank(body) {
        return 0;
    }
    let mut max_start = 0;
    for g in rx::all_groups(body, PAGER_START_RE) {
        if let Some(n) = try_int(&g[1]) {
            if n > max_start {
                max_start = n;
            }
        }
    }
    max_start / TOPICS_PER_PAGE
}

/// Drop map slots past the live 0-based last index (inclusive `page <= max_page`).
pub fn prune_pages_beyond_max(tasks: &mut Vec<TaskParse>, max_page: i32) -> i32 {
    crate::common::prune_pages_beyond_max(tasks, max_page)
}

/// Session expired: the page shows the login form and no topic rows.
pub fn looks_like_login_form(body: &str) -> bool {
    !body.is_empty() && body.contains("name=\"login_username\"") && !body.contains("id=\"tt-")
}

pub fn category_types(cat: &str) -> &'static [&'static str] {
    categories::types_for(cat)
}

fn g1(text: &str, pattern: &str) -> Option<(String, String)> {
    let g = rx::groups(text, pattern);
    if g[0].is_empty() {
        None
    } else {
        Some((g[1].trim().to_string(), g.get(2).map(|s| s.trim().to_string()).unwrap_or_default()))
    }
}

/// Peel «RUS [/ ALT / ] EN [Sxx] (YEAR) …» into (russian, original, year).
pub fn parse_title(title: &str) -> (String, String, i32) {
    let year = try_int(&rx::group(title, YEAR_RE, 1)).unwrap_or(0);
    for (pattern, has_orig) in [
        (TITLE_SERIAL3_RE, true),
        (TITLE_SERIAL2_RE, true),
        (TITLE_SERIAL1_RE, false),
        (TITLE_MOVIE3_RE, true),
        (TITLE_MOVIE2_RE, true),
        (TITLE_MOVIE1_RE, false),
    ] {
        if let Some((name, orig)) = g1(title, pattern) {
            return (name, if has_orig { orig } else { String::new() }, year);
        }
    }
    (String::new(), String::new(), year)
}

/// Text before the first `[`, `/`, `(` or `|`.
pub fn first_token_title(title: &str) -> String {
    if util::is_blank(title) {
        return String::new();
    }
    match rx::re(FIRST_NAME_PART_RE).find(title).ok().flatten() {
        Some(m) => title[..m.start()].trim().to_string(),
        None => title.trim().to_string(),
    }
}

/// Parse a forum listing page. `canonical_host` is stored in FileDB urls (never the alias).
pub fn parse_listing_html(body: &str, cat: &str, canonical_host: &str) -> Vec<TorrentDetails> {
    let mut out = Vec::new();
    if util::is_blank(body) {
        return out;
    }
    let types = category_types(cat);
    if types.is_empty() {
        return out;
    }
    let host = canonical_host.trim_end_matches('/');
    for (i, part) in body.split("id=\"tt-").enumerate() {
        if i == 0 {
            continue;
        }
        // Re-prefix so the topic-id pattern (expects the full marker) matches.
        let row = format!("<a id=\"tt-{part}");

        let id = match1(ROW_TOPIC_ID_RE, &row);
        let title = clean_text(&match1(ROW_TITLE_RE, &row));
        if util::is_blank(&id) || util::is_blank(&title) {
            continue;
        }
        let create_time = parse_listing_date(&match1(ROW_DATE_RE, &row));
        if time::is_min(&create_time) {
            continue;
        }
        let sid = try_int(&match1(ROW_SID_RE, &row)).unwrap_or(0);
        let pir = try_int(&match1(ROW_PIR_RE, &row)).unwrap_or(0);

        let size_name = clean_text(&match1(ROW_SIZE_RE, &row)).replace('\u{00A0}', " ").trim().to_string();
        let magnet = util::html_decode(&match1(ROW_MAGNET_RE, &row));
        if util::is_blank(&size_name) || util::is_blank(&magnet) {
            continue;
        }

        let (mut name, original, year) = parse_title(&title);
        if util::is_blank(&name) {
            name = first_token_title(&title);
        }
        if util::is_blank(&name) {
            continue;
        }

        let mut t = TorrentDetails::new(TRACKER_NAME, types, topic_url(host, &id), title);
        t.sid = sid;
        t.pir = pir;
        t.sizeName = size_name;
        t.magnet = magnet;
        t.createTime = create_time;
        t.updateTime = time::now();
        t.name = name;
        t.originalname = original;
        t.relased = year;
        out.push(t);
    }
    out
}

/// Read «YYYY-MM-DD HH:MM» as Europe/Moscow and return UTC (`time::min()` on failure).
pub fn parse_listing_date(s: &str) -> DateTime<Utc> {
    if util::is_blank(s) {
        return time::min();
    }
    match NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M") {
        Ok(local) => moscow_to_utc(local).unwrap_or_else(time::min),
        Err(_) => time::min(),
    }
}

fn match1(pattern: &str, s: &str) -> String {
    rx::group(s, pattern, 1).trim().to_string()
}

fn clean_text(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = rx::replace(s, STRIP_TAGS_RE, "");
    let s = util::html_decode(&s).replace('\u{00A0}', " ");
    rx::replace(&s, WHITESPACE_RE, " ").trim().to_string()
}
