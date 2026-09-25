//! Mazepa forum listing parser (phpBB-style `tr-{id}` rows, Ukrainian dates).

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::{rx, util};
use indexmap::IndexSet;

use crate::common;

const TRACKER_NAME: &str = "mazepa";

/// Listing row plus the `dl.php?id=` value used to fetch the .torrent.
#[derive(Clone, Debug, Default)]
pub struct MazepaDetails {
    pub t: TorrentDetails,
    pub download_id: String,
}

impl AsRef<TorrentDetails> for MazepaDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for MazepaDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

/// Strip year, season/episode, resolution, source and codec noise from a title.
pub fn clean_title(title: &str) -> Option<String> {
    if util::is_blank(title) {
        return None;
    }
    let mut t = title.to_string();
    t = rx::replace_i(&t, r"\s*\((19|20)\d{2}(\-\d{4})?\)", "");
    t = rx::replace_i(&t, r"\b(Сезон|Season)\s*\d+.*$", "");
    t = rx::replace_i(&t, r"\b(S\d{1,2}|E\d{1,2}|S\d{1,2}E\d{1,2})\b", "");
    t = rx::replace_i(&t, r"\b(2160p|1080p|720p|480p)\b", "");
    t = rx::replace_i(&t, r"\b(WEB[-\s]?DL|WEB[-\s]?Rip|BDRip|BDRemux|HDRip|BluRay|BRRip|DVDRip|HDTV)\b", "");
    t = rx::replace_i(&t, r"\b(x264|x265|h\.?264|h\.?265|hevc|avc|aac|ac3|dts|ddp?\d\.\d|vc\-?1)\b", "");
    t = rx::replace(&t, r"[\[\]\|]", " ");
    t = rx::replace(&t, r"\s{2,}", " ").trim().to_string();
    Some(t)
}

/// (name, originalname, year) from a `Укр / Orig (2020)` style title.
pub fn parse_names_advanced(title: &str) -> (Option<String>, Option<String>, i32) {
    if util::is_blank(title) {
        return (None, None, 0);
    }
    let m = common::match_groups(title, r"^(.*?)\s*\((\d{4}|\d{4}-\d{4})\)", false);
    let before_year = match m {
        Some(g) => g[1].clone(),
        None => title.to_string(),
    };
    let yr = rx::group(title, r"\((\d{4})\)", 1);

    let parts: Vec<String> = rx::split(&before_year, r"\s*/\s*").into_iter().filter(|p| !util::is_blank(p)).map(|p| p.trim().to_string()).collect();
    if parts.is_empty() {
        return (None, None, 0);
    }

    let original = parts.iter().rev().find(|p| rx::is_match(p, "[A-Za-z]")).cloned();
    let name = parts.iter().find(|p| !rx::is_match(p, "[A-Za-z]")).cloned();
    let year = yr.parse::<i32>().unwrap_or(0);

    let name = name.unwrap_or_else(|| parts[0].clone());
    let original = original.unwrap_or_else(|| name.clone());

    let name = clean_title(&name).map(|n| tparse::replace_bad_names(&n));
    let original = clean_title(&original).map(|o| tparse::replace_bad_names(&o));
    (name, original, year)
}

/// «Сьогодні 12:21», «Вчора 18:05» or «4 Лис 2025, 13:00» → UTC. `None` when unparsable.
pub fn parse_mazepa_date(text: &str) -> Option<DateTime<Utc>> {
    if util::is_blank(text) {
        return None;
    }
    let text = util::html_decode(text).trim().to_string();
    let text = rx::replace(&text, r"\s+", " ");

    if let Some(rel) = common::match_groups(&text, r"^(Сьогодні|Вчора)\s+(\d{1,2}):(\d{2})$", true) {
        let h: u32 = rel[2].parse().ok()?;
        let mi: u32 = rel[3].parse().ok()?;
        let mut base = Utc::now().date_naive();
        if rel[1].to_lowercase() == "вчора" {
            base -= Duration::days(1);
        }
        let dt = base.and_hms_opt(h, mi, 0)?;
        return Some(Utc.from_utc_datetime(&dt));
    }

    let m = common::match_groups(&text, r"(\d{1,2})\s+([^\s]+)\s+(\d{4}),\s*(\d{1,2}):(\d{2})", true)?;
    let day: u32 = m[1].parse().ok()?;
    let month_raw = m[2].trim().to_lowercase();
    let year: i32 = m[3].parse().ok()?;
    let hour: u32 = m[4].parse().ok()?;
    let minute: u32 = m[5].parse().ok()?;
    let month = match month_raw.as_str() {
        "січ" | "сiч" => 1,
        "лют" => 2,
        "бер" => 3,
        "кві" | "квi" => 4,
        "тра" => 5,
        "чер" => 6,
        "лип" => 7,
        "сер" => 8,
        "вер" => 9,
        "жов" => 10,
        "лис" => 11,
        "гру" => 12,
        _ => return None,
    };
    let dt = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, 0)?;
    Some(Utc.from_utc_datetime(&dt))
}

/// `magnet:?xt=urn:btih:{hash}` without trackers, or `None`.
pub fn normalize_magnet(magnet: &str) -> Option<String> {
    if magnet.is_empty() {
        return None;
    }
    let magnet = util::html_decode(magnet);
    let h = rx::group(&magnet, r"btih:([A-Fa-f0-9]{40}|[A-Z2-7]{32})", 1);
    if h.is_empty() {
        return None;
    }
    Some(format!("magnet:?xt=urn:btih:{h}"))
}

pub fn parse_size_name(block: &str) -> Option<String> {
    if let Some(m) = common::match_groups(block, r">([\d\.,]+)\s*&nbsp;(GB|MB|TB)<", true).filter(|m| !util::is_blank(&m[1])) {
        return Some(format!("{} {}", m[1].trim(), m[2].trim()));
    }
    if let Some(m) = common::match_groups(block, r"([\d\.,]+)\s*(GB|MB|TB|ГБ|МБ|ТБ)\b", true).filter(|m| !util::is_blank(&m[1])) {
        return Some(format!("{} {}", m[1].trim(), m[2].trim()));
    }
    None
}

pub fn parse_size_bytes(size_name: &str) -> f64 {
    if util::is_blank(size_name) {
        return 0.0;
    }
    let g = rx::groups_i(size_name, r"([0-9\.,]+)\s*(Mb|МБ|GB|ГБ|TB|ТБ)");
    if g.len() < 3 || util::is_blank(&g[2]) {
        return 0.0;
    }
    let Ok(mut size) = g[1].replace(',', ".").parse::<f64>() else { return 0.0 };
    let u = g[2].to_lowercase();
    if u == "gb" || u == "гб" {
        size *= 1024.0;
    } else if u == "tb" || u == "тб" {
        size *= 1048576.0;
    }
    size * 1048576.0
}

pub fn parse_quality(title: &str) -> i32 {
    if util::is_blank(title) {
        return 480;
    }
    if title.contains("2160p") || rx::is_match_i(title, "(4k|uhd)") {
        return 2160;
    }
    if title.contains("1080p") {
        return 1080;
    }
    if title.contains("720p") {
        return 720;
    }
    480
}

pub fn parse_videotype(title: &str) -> String {
    if util::is_blank(title) {
        return "sdr".into();
    }
    if rx::is_match(&title.to_lowercase(), r"\bhdr\b|hdr10") {
        return "hdr".into();
    }
    "sdr".into()
}

pub fn parse_torrents_from_category_page(html: &str, types: &[&str], host: &str) -> Vec<MazepaDetails> {
    let mut list: Vec<MazepaDetails> = Vec::new();
    let rows = rx::all_groups(html, r#"(?s)<tr id="tr-(\d+)".*?>.*?</tr>"#);
    if rows.is_empty() {
        return list;
    }

    for row in rows {
        let block = row[0].as_str();
        let tid = rx::group(block, r"tr-(\d+)", 1);
        if tid.is_empty() {
            continue;
        }
        let title = rx::group(block, r#"class="torTopic[^"]*"><b>([^<]+)</b>"#, 1);
        let download_id = rx::group(block, r"dl\.php\?id=(\d+)", 1);
        let magnet = rx::group(block, r#"href="(magnet:\?[^"]+)""#, 1);
        if util::is_blank(&title) || util::is_blank(&download_id) {
            continue;
        }

        let size_name = parse_size_name(block).unwrap_or_default();
        let sid = rx::group(block, r"seedmed[^>]*><b>(\d+)</b>", 1).parse().unwrap_or(0);
        let pir = rx::group(block, r"leechmed[^>]*><b>(\d+)</b>", 1).parse().unwrap_or(0);
        let last_post_text = rx::group(block, r#"(?s)<ul class="last_post[^"]*">.*?<a[^>]*>([^<]+)</a>"#, 1);
        let last_post_time = parse_mazepa_date(&last_post_text).unwrap_or_else(Utc::now);

        let title_trim = title.trim().to_string();
        let (name, originalname, year) = parse_names_advanced(&title_trim);

        let mut t = TorrentDetails::new(TRACKER_NAME, types, format!("{host}/viewtopic.php?t={tid}"), title_trim.clone());
        t.name = name.unwrap_or_default();
        t.originalname = originalname.unwrap_or_default();
        t.magnet = normalize_magnet(&magnet).unwrap_or_default();
        t.size = parse_size_bytes(&size_name);
        t.sizeName = size_name;
        t.quality = parse_quality(&title_trim);
        t.videotype = parse_videotype(&title_trim);
        t.sid = sid;
        t.pir = pir;
        t.createTime = last_post_time;
        t.updateTime = last_post_time;
        t.relased = year;
        list.push(MazepaDetails { t, download_id });
    }

    let mut seen = IndexSet::new();
    list.retain(|x| seen.insert(x.t.url.clone()));
    list
}
