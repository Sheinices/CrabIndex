//! anibelka.com - anime-only phpBB tracker.
//! Stays anonymous on purpose: a logged-in .torrent embeds a personal passkey.

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashSet;

use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::{rx, time, util};

use crate::common::{has_cyrillic, has_latin, moscow_to_utc, try_int};

pub const TRACKER_NAME: &str = "anibelka";
pub const TOPICS_PER_PAGE: i32 = 15;

const ROW_TOPIC_RE: &str = r#"(?is)href="\./viewtopic\.php\?t=(\d+)[^"]*"\s+class="topictitle">(.*?)</a>"#;
const PAGE_START_RE: &str = r#"viewforum\.php\?f=[0-9]+[^"'\s>]*?start=([0-9]+)"#;
const TORRENT_LINK_RE: &str = r#"(?is)href="\./download/file\.php\?id=(\d+)[^"]*"[^>]*tooltip="Скачать торрент""#;
const SIZE_RE: &str = r"(?is)Размер:\s*<b>([0-9.,]+)&nbsp;(КБ|МБ|ГБ|ТБ)</b>";
const ADDED_RE: &str = r"(?is)Добавлен:\s*<b>\s*<span[^>]*>([^<]+)</span>";
const SEED_RE: &str = r#"(?is)Сидеров:\s*<span class="seed">\s*<b>(\d+)</b>"#;
const LEECH_RE: &str = r#"(?is)Личеров:\s*<span class="leech">\s*<b>(\d+)</b>"#;
const TAG_RE: &str = r"^\[(\w+)\]\s*";
const YEAR_RE: &str = r"\[(\d{4})";
const RU_DATE_RE: &str = r"^(\d{1,2})\s+([А-Яа-яЁё]+)\s+(\d{4})(?:,\s*(\d{2}):(\d{2}))?";
const STRIP_TAGS_RE: &str = r"<[^>]+>";
const WHITESPACE_RE: &str = r"\s+";

/// Anonymous `.torrent` attachment id for `download/file.php?id=…` rides along with the row.
#[derive(Clone, Debug, Default)]
pub struct AnibelkaDetails {
    pub t: TorrentDetails,
    pub download_id: String,
}

impl AsRef<TorrentDetails> for AnibelkaDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for AnibelkaDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnibelkaListingItem {
    pub topic_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnibelkaTopicInfo {
    pub torrent_id: String,
    pub size_name: String,
    pub sid: i32,
    pub pir: i32,
    pub create_time: DateTime<Utc>,
}

impl Default for AnibelkaTopicInfo {
    fn default() -> Self {
        AnibelkaTopicInfo { torrent_id: String::new(), size_name: String::new(), sid: 0, pir: 0, create_time: time::min() }
    }
}

pub fn forum_url(host: &str, section_id: &str, page: i32) -> String {
    let host = host.trim_end_matches('/');
    if page <= 0 {
        format!("{host}/viewforum.php?f={section_id}")
    } else {
        format!("{host}/viewforum.php?f={section_id}&start={}", page * TOPICS_PER_PAGE)
    }
}

pub fn topic_url(host: &str, topic_id: &str) -> String {
    format!("{}/viewtopic.php?t={topic_id}", host.trim_end_matches('/'))
}

pub fn torrent_download_url(host: &str, torrent_id: &str) -> String {
    format!("{}/download/file.php?id={torrent_id}", host.trim_end_matches('/'))
}

/// Zero-based last page from the largest `viewforum.php?f=…&start=N` pager link.
/// Bare `start=N` (scripts, jumpto) is ignored so the map cannot inflate.
pub fn last_page_from_html(body: &str) -> i32 {
    if util::is_blank(body) {
        return 0;
    }
    let mut max_start = 0;
    for g in rx::all_groups(body, PAGE_START_RE) {
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

pub fn parse_listing_html(body: &str) -> Vec<AnibelkaListingItem> {
    let mut out = Vec::new();
    if util::is_blank(body) {
        return out;
    }
    let mut seen = HashSet::new();
    for g in rx::all_groups(body, ROW_TOPIC_RE) {
        let title = clean_text(&g[2]);
        // Pinned service topics hold no torrent and have no [tag] prefix.
        if util::is_blank(&title) || !title.starts_with('[') || !seen.insert(g[1].clone()) {
            continue;
        }
        out.push(AnibelkaListingItem { topic_id: g[1].clone(), title });
    }
    out
}

pub fn try_parse_topic_html(body: &str) -> Option<AnibelkaTopicInfo> {
    if util::is_blank(body) {
        return None;
    }
    let tm = rx::groups(body, TORRENT_LINK_RE);
    if tm[0].is_empty() {
        return None;
    }
    let mut info = AnibelkaTopicInfo { torrent_id: tm[1].clone(), ..Default::default() };

    let sm = rx::groups(body, SIZE_RE);
    if !sm[0].is_empty() {
        info.size_name = format!("{} {}", sm[1].trim(), sm[2]);
    }
    let seed = rx::groups(body, SEED_RE);
    if !seed[0].is_empty() {
        if let Some(n) = try_int(&seed[1]) {
            info.sid = n;
        }
    }
    let leech = rx::groups(body, LEECH_RE);
    if !leech[0].is_empty() {
        if let Some(n) = try_int(&leech[1]) {
            info.pir = n;
        }
    }
    let added = rx::groups(body, ADDED_RE);
    if !added[0].is_empty() {
        info.create_time = parse_ru_date(&util::html_decode(&added[1]));
    }
    if time::is_min(&info.create_time) {
        info.create_time = time::now();
    }
    Some(info)
}

/// Split a listing title into (name, original, year).
/// Original is the first Latin-only slash part (not simply the second).
pub fn parse_title(title: &str) -> (String, String, i32) {
    let year = try_int(&rx::group(title, YEAR_RE, 1)).unwrap_or(0);

    let mut body = rx::re(TAG_RE).replace(title, "").into_owned();
    if let Some(b) = body.find('[') {
        body.truncate(b);
    }
    let parts: Vec<&str> = body.split(" / ").map(|p| p.trim()).collect();
    if parts.is_empty() || util::is_blank(parts[0]) {
        return (String::new(), String::new(), year);
    }
    let name = parts[0];
    let original = parts.iter().skip(1).find(|p| has_latin(p) && !has_cyrillic(p)).copied().unwrap_or("");
    (name.trim().to_string(), original.trim().to_string(), year)
}

pub fn category_tag(title: &str) -> String {
    rx::group(title, TAG_RE, 1)
}

pub fn build_torrent(host: &str, item: &AnibelkaListingItem, info: &AnibelkaTopicInfo, magnet: &str) -> Option<AnibelkaDetails> {
    let (name, original, year) = parse_title(&item.title);
    if util::is_blank(&name) || util::is_blank(magnet) {
        return None;
    }
    let now = time::now();
    let mut t = TorrentDetails::new(TRACKER_NAME, &["anime"], topic_url(host, &item.topic_id), item.title.clone());
    t.sid = info.sid;
    t.pir = info.pir;
    t.sizeName = info.size_name.clone();
    t.magnet = magnet.to_string();
    t.createTime = if time::is_min(&info.create_time) { now } else { info.create_time };
    t.updateTime = now;
    t.name = name;
    t.originalname = original;
    t.relased = year;
    Some(AnibelkaDetails { t, download_id: info.torrent_id.clone() })
}

fn ru_month(key: &str) -> Option<u32> {
    Some(match key {
        "янв" => 1,
        "фев" => 2,
        "мар" => 3,
        "апр" => 4,
        "май" => 5,
        "июн" => 6,
        "июл" => 7,
        "авг" => 8,
        "сен" => 9,
        "окт" => 10,
        "ноя" => 11,
        "дек" => 12,
        _ => return None,
    })
}

/// Read «23 июл 2026, 08:56» as Europe/Moscow and return UTC (`time::min()` on failure).
pub fn parse_ru_date(s: &str) -> DateTime<Utc> {
    if util::is_blank(s) {
        return time::min();
    }
    let s = rx::replace(s.replace('\u{00A0}', " ").trim(), WHITESPACE_RE, " ");
    let Some(c) = rx::captures(&s, RU_DATE_RE) else { return time::min() };
    let g = |i: usize| c.get(i).map(|m| m.as_str()).unwrap_or("");
    let (Some(day), Some(year)) = (try_int(g(1)), try_int(g(3))) else { return time::min() };
    let key: String = g(2).to_lowercase().chars().take(3).collect();
    let Some(month) = ru_month(&key) else { return time::min() };
    let (mut hour, mut minute) = (0u32, 0u32);
    if !g(4).is_empty() {
        hour = g(4).parse().unwrap_or(0);
        minute = g(5).parse().unwrap_or(0);
    }
    let Some(local) = u32::try_from(day)
        .ok()
        .and_then(|d| NaiveDate::from_ymd_opt(year, month, d))
        .and_then(|d| d.and_hms_opt(hour, minute, 0))
    else {
        return time::min();
    };
    moscow_to_utc(local).unwrap_or_else(time::min)
}

fn clean_text(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = rx::replace(s, STRIP_TAGS_RE, "");
    let s = util::html_decode(&s);
    rx::replace(&s, WHITESPACE_RE, " ").trim().to_string()
}
