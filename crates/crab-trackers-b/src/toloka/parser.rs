//! Toloka forum listing parser (`/f{cat}` pages).

use chrono::{DateTime, Utc};
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::parsing::tparse;
use crab_core::{conf, rx, time, util};

use crate::common;

const TRACKER_NAME: &str = "toloka";
const NEXT_PAGER_RE: &str = r#">([0-9]+)</a>&nbsp;&nbsp;<a href="[^"]+">наступна</a>"#;

/// Listing row plus the `download.php?id=` value used to fetch the .torrent.
#[derive(Clone, Debug, Default)]
pub struct TolokaDetails {
    pub t: TorrentDetails,
    pub download_id: String,
}

impl AsRef<TorrentDetails> for TolokaDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for TolokaDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

/// 1-based last page number before «наступна». Task slots are 0..N-1 (`/f{cat}-{page*45}`).
pub fn last_page_from_html(html: &str) -> i32 {
    if util::is_blank(html) {
        return 0;
    }
    match rx::group(html, NEXT_PAGER_RE, 1).parse::<i32>() {
        Ok(n) if n >= 1 => n,
        _ => 0,
    }
}

/// Real forum chrome (`lang="uk"`), not a login wall / empty fetch.
pub fn looks_like_forum_listing(html: &str) -> bool {
    !html.is_empty() && html.contains("<html lang=\"uk\"")
}

/// Drop map slots at or past the live page count (exclusive `page < page_count`).
pub fn prune_pages_beyond_page_count(tasks: Option<&mut Vec<TaskParse>>, page_count: i32) -> i32 {
    common::prune_pages_beyond_page_count(tasks, page_count)
}

pub fn parse_torrents_from_page(html: &str, cat: &str) -> Vec<TolokaDetails> {
    let mut torrents = Vec::new();
    let html = tparse::replace_bad_names(html);

    for row in html.split("</tr>").skip(1) {
        if util::is_blank(row) || rx::is_match_i(row, "Збір коштів") {
            continue;
        }
        let Some(create_time) = try_parse_create_time(row) else { continue };
        let Some((url, title, sid, pir, size_name)) = try_parse_row_fields(row) else { continue };

        let (name, originalname, relased) = parse_title_names(cat, &title);
        let mut name = name.unwrap_or_default();
        if util::is_blank(&name) {
            name = common::first_title_segment(&title);
        }
        if util::is_blank(&name) {
            continue;
        }

        let Some(types) = get_types_for_category(cat) else { continue };

        let download_id = rx::group_i(row, r#"href="(?:https?://[^"]+/)?download\.php\?id=([0-9]+)""#, 1);
        if util::is_blank(&download_id) {
            continue;
        }

        let mut t = TorrentDetails::new(TRACKER_NAME, types, url, title);
        t.sid = common::parse_int(&sid);
        t.pir = common::parse_int(&pir);
        t.sizeName = size_name;
        t.createTime = create_time;
        t.name = name;
        t.originalname = originalname.unwrap_or_default();
        t.relased = relased;
        torrents.push(TolokaDetails { t, download_id });
    }
    torrents
}

fn match_row(row: &str, pattern: &str) -> String {
    let res = util::html_decode(rx::group_i(row, pattern, 1).trim()).replace('\u{00A0}', " ");
    rx::replace(&res, "[\n\r\t ]+", " ").trim().to_string()
}

fn try_parse_create_time(row: &str) -> Option<DateTime<Utc>> {
    let raw = match_row(row, r#"class="postdetails">([0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2})"#).replace('-', ".");
    let dt = common::parse_ymd_hm(&raw)?;
    if time::is_min(&dt) {
        return None;
    }
    Some(dt)
}

fn try_parse_row_fields(row: &str) -> Option<(String, String, String, String, String)> {
    let url = match_row(row, r#"<a href="(?:https?://[^"]+/)?(t[0-9]+)" class="topictitle""#);
    let title = match_row(row, r#"class="topictitle">([^<]+)</a>"#);
    let sid = match_row(row, r#"<span class="seedmed"[^>]*><b>([0-9]+)</b></span>"#);
    let pir = match_row(row, r#"<span class="leechmed"[^>]*><b>([0-9]+)</b></span>"#);
    let size_name = match_row(row, r#"<a href="(?:https?://[^"]+/)?download\.php[^"]+"[^>]*>([^<]+)</a>"#);

    if [&url, &title, &sid, &pir, &size_name].iter().any(|s| util::is_blank(s))
        || size_name == "0 B"
        || size_name.to_lowercase().contains(&"Завантажити".to_lowercase())
    {
        return None;
    }
    let url = format!("{}/{url}", conf().Toloka.host.trim_end_matches('/'));
    Some((url, title, sid, pir, size_name))
}

type Names = (Option<String>, Option<String>, i32);

fn parse_title_names(cat: &str, title: &str) -> Names {
    match cat {
        "16" | "96" | "19" | "139" | "12" | "131" | "84" | "42" | "140" => parse_movie_title(title),
        "32" | "173" | "174" | "44" | "230" | "226" | "227" | "228" | "229" | "127" | "124" | "125" | "132" => parse_serial_title(title),
        _ => (None, None, 0),
    }
}

/// (pattern, name group, original group (0 = none), year group, trim name)
type Layout = (&'static str, usize, usize, usize, bool);

fn first_layout(title: &str, layouts: &[Layout]) -> Names {
    for (p, n, o, y, trim) in layouts {
        let g = rx::groups(title, p);
        let get = |i: usize| g.get(i).cloned().unwrap_or_default();
        let (name, orig, year) = (get(*n), if *o > 0 { get(*o) } else { String::new() }, get(*y));
        if !util::is_blank(&name) && (*o == 0 || !util::is_blank(&orig)) && !util::is_blank(&year) {
            let name = if *trim { name.trim().to_string() } else { name };
            let orig = if *o > 0 { Some(orig.trim().to_string()) } else { None };
            return (Some(name), orig, year.parse().unwrap_or(0));
        }
    }
    (None, None, 0)
}

fn parse_movie_title(title: &str) -> Names {
    first_layout(
        title,
        &[
            (r"^([^/\(\[]+)/[^/\(\[]+/([^/\(\[]+) \(([0-9]{4})(\)|-)", 1, 2, 3, true),
            (r"^([^/\(\[]+)/([^/\(\[]+) \(([0-9]{4})(\)|-)", 1, 2, 3, true),
            (r"^([^/\(\[]+) \([^\)]+\) \(([0-9]{4})(\)|-)", 1, 0, 2, false),
            (r"^([^/\(\[]+) \(([0-9]{4})(\)|-)", 1, 0, 2, false),
        ],
    )
}

fn parse_serial_title(title: &str) -> Names {
    first_layout(
        title,
        &[
            (r"^([^/\(\[]+) \([^\)]+\) \([^\)]+\) ?/([^/\(\[]+) \([^\)]+\) \(([0-9]{4})(\)|-)", 1, 2, 3, true),
            (r"^([^/\(\[]+) \([^\)]+\) ?/([^/\(\[]+) \([^\)]+\) \(([0-9]{4})(\)|-)", 1, 2, 3, true),
            (r"^([^/\(\[]+) (\(|\[)[^\)\]]+(\)|\]) ?/([^/\(\[]+) \(([0-9]{4})(\)|-)", 1, 4, 5, true),
            (r"^([^/\(\[]+)/([^/\(\[]+) \([^\)]+\) \(([0-9]{4})(\)|-)", 1, 2, 3, true),
            (r"^([^/\(\[]+)/[^/\(\[]+/([^/\(\[]+) \([^\)]+\) \(([0-9]{4})(\)|-)", 1, 2, 3, true),
            (r"^([^/\(\[]+) \([^\)]+\) \(([0-9]{4})(\)|-)", 1, 0, 2, true),
        ],
    )
}

pub fn get_types_for_category(cat: &str) -> Option<&'static [&'static str]> {
    Some(match cat {
        "16" | "96" | "42" => &["movie"],
        "19" | "139" | "84" => &["multfilm"],
        "32" | "173" | "124" => &["serial"],
        "174" | "44" | "125" => &["multserial"],
        "226" | "227" | "228" | "229" | "230" | "12" | "131" | "140" => &["docuserial", "documovie"],
        "127" => &["anime"],
        "132" => &["tvshow"],
        _ => return None,
    })
}
