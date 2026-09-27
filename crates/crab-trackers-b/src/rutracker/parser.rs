//! Rutracker HTML parsing: forum listing rows, pager, topic page magnet.

use chrono::{DateTime, Utc};
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::parsing::{rutracker_title, tparse};
use crab_core::{conf, rx, time, util};

use super::categories::{self, TitleKind};
use crate::common;

const TRACKER_NAME: &str = "rutracker";
const PAGE_OF_RE: &str = r"Страница <b>1</b> из <b>([0-9]+)</b>";
const TOR_TOPIC_CLASS: &str = "class=\"torTopic\"";

/// 1-based page count from «Страница 1 из N». Task slots are 0..N-1 (`start=page*50`).
pub fn last_page_from_html(html: &str) -> i32 {
    if util::is_blank(html) {
        return 0;
    }
    let g = rx::group(html, PAGE_OF_RE, 1);
    match g.parse::<i32>() {
        Ok(n) if n >= 1 => n,
        _ => 0,
    }
}

/// Real viewforum HTML (topics or pager), not a Cloudflare interstitial / empty fetch.
pub fn looks_like_forum_listing(html: &str) -> bool {
    if util::is_blank(html) {
        return false;
    }
    html.contains(TOR_TOPIC_CLASS) || rx::is_match(html, PAGE_OF_RE)
}

pub fn topic_row_count(html: &str) -> i32 {
    if util::is_blank(html) {
        return 0;
    }
    html.matches(TOR_TOPIC_CLASS).count() as i32
}

/// Live page count for UpdateTasksParse. 0 = failed fetch (do not prune).
/// Emptied archives still advertise «из 475» with zero rows - treated as 1 page.
pub fn effective_page_count(html: &str) -> i32 {
    if !looks_like_forum_listing(html) {
        return 0;
    }
    if topic_row_count(html) == 0 {
        return 1;
    }
    last_page_from_html(html).max(1)
}

/// Drop map slots at or past the live page count (exclusive `page < page_count`).
pub fn prune_pages_beyond_page_count(tasks: Option<&mut Vec<TaskParse>>, page_count: i32) -> i32 {
    common::prune_pages_beyond_page_count(tasks, page_count)
}

pub fn parse_torrents_from_page(html: &str, cat: &str) -> Vec<TorrentDetails> {
    let mut torrents = Vec::new();
    let Some(meta) = categories::get(cat) else { return torrents };

    let html = tparse::replace_bad_names(html);
    for row in html.split(TOR_TOPIC_CLASS).skip(1) {
        if util::is_blank(row) {
            continue;
        }
        let Some(create_time) = try_parse_create_time(row) else { continue };
        let Some((url, title, sid, pir, size_name)) = try_parse_row_fields(row) else { continue };

        let (name, originalname, relased, skip_row) = parse_title_names(meta.title_kind, &title);
        if skip_row {
            continue;
        }
        let mut name = name.unwrap_or_default();
        if util::is_blank(&name) {
            name = common::first_title_segment(&title);
        }
        if util::is_blank(&name) {
            continue;
        }

        let mut t = TorrentDetails::new(TRACKER_NAME, meta.types, url, title);
        t.sid = common::parse_int(&sid);
        t.pir = common::parse_int(&pir);
        t.sizeName = size_name;
        t.createTime = create_time;
        t.name = name;
        t.originalname = originalname.unwrap_or_default();
        t.relased = relased;
        torrents.push(t);
    }
    torrents
}

/// Fill createTime/magnet from the topic page. True when a magnet was found.
pub fn apply_topic_page_details(t: &mut TorrentDetails, full_news: Option<&str>) -> bool {
    let Some(full_news) = full_news else { return false };

    let time = rx::group(full_news, r#"<a class="p-link small" href="viewtopic.php\?t=[^"]+">([^<]+)</a>"#, 1);
    if let Some(ct) = tparse::parse_create_time(&time.replace('-', " "), "dd.MM.yy HH:mm") {
        if !time::is_min(&ct) {
            t.createTime = ct;
        }
    }

    let magnet = util::html_decode(&rx::group(full_news, r#"href="(magnet:[^"]+)" class="(med )?magnet-link""#, 1));
    if !util::is_blank(&magnet) {
        t.magnet = magnet;
        return true;
    }
    false
}

/// Skip the topic GET when listing title and size match a row that already has a magnet
/// written at or after the listing timestamp. A title-only check froze magnets when the
/// torrent was replaced in-place.
pub fn should_skip_topic_fetch(cached: Option<&TorrentDetails>, listing: &TorrentDetails) -> bool {
    let Some(cached) = cached else { return false };
    if util::is_blank(&cached.magnet) {
        return false;
    }
    if cached.title != listing.title {
        return false;
    }
    if !size_names_equal(&cached.sizeName, &listing.sizeName) {
        return false;
    }
    if time::is_min(&listing.createTime) || time::is_min(&cached.updateTime) {
        return false;
    }
    listing.createTime <= cached.updateTime
}

fn size_names_equal(left: &str, right: &str) -> bool {
    normalize_size_name(left).to_lowercase() == normalize_size_name(right).to_lowercase()
}

fn normalize_size_name(size_name: &str) -> String {
    if util::is_blank(size_name) {
        return String::new();
    }
    let decoded = util::html_decode(size_name).replace('\u{00a0}', " ");
    rx::replace(decoded.trim(), r"\s+", " ")
}

fn match_row(row: &str, pattern: &str) -> String {
    let res = util::html_decode(rx::group_i(row, pattern, 1).trim());
    rx::replace(&res, "[\n\r\t ]+", " ").trim().to_string()
}

fn try_parse_create_time(row: &str) -> Option<DateTime<Utc>> {
    let dt = common::parse_ymd_hm(&match_row(row, "<p>([0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2})</p>"))?;
    if time::is_min(&dt) {
        return None;
    }
    Some(dt)
}

fn try_parse_row_fields(row: &str) -> Option<(String, String, String, String, String)> {
    let url = match_row(row, "<a id=\"tt-([0-9]+)\"");
    let title = match_row(row, "<a id=\"tt-[0-9]+\"[^>]+>([^\n\r]+)</a>");
    let title = rx::replace(&title, "<[^>]+>", "");
    let sid = match_row(row, "<span class=\"seedmed\"[^>]+><b>([0-9]+)</b>");
    let pir = match_row(row, "<span class=\"leechmed\"[^>]+><b>([0-9]+)</b>");
    let size_name = match_row(row, "dl-stub\">([^<]+)</a>").replace("&nbsp;", " ").replace('\u{a0}', " ");

    if [&url, &title, &sid, &pir, &size_name].iter().any(|s| util::is_blank(s)) {
        return None;
    }
    // FileDB always stores the canonical tracker URL; requests go through rq_host (alias).
    let url = format!("{}/forum/viewtopic.php?t={url}", conf().Rutracker.host);
    Some((url, title, sid, pir, size_name))
}

type Names = (Option<String>, Option<String>, i32, bool);

/// `(name, originalname, relased, skip_row)` from the topic title; see [`rutracker_title`].
fn parse_title_names(kind: TitleKind, title: &str) -> Names {
    let k = match kind {
        TitleKind::Movie => rutracker_title::Kind::Movie,
        TitleKind::Serial => rutracker_title::Kind::Serial,
        TitleKind::NonStandard => rutracker_title::Kind::NonStandard,
    };
    rutracker_title::parse(k, title)
}
