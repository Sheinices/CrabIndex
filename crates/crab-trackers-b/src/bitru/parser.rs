//! BitRu API JSON → FileDB rows.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use crab_core::models::TorrentDetails;
use crab_core::{rx, util};
use serde_json::Value;

use super::categories;
use super::models::{BitruApiItemInner, BitruApiResponse};

/// Strip season, episode, quality, codec etc. from a title for name/originalname
/// (search matches on the base name; the season is a separate parameter).
pub fn clean_title_for_search(title: &str) -> String {
    if util::is_blank(title) {
        return title.to_string();
    }
    let mut t = title.trim().to_string();

    if let Ok(Some(m)) = rx::re(r"[\(\[](\d{4})[\)\]]").find(&t) {
        if m.start() > 0 {
            t = t[..m.start()].to_string();
        }
    }

    t = rx::replace_i(&t, r"\b(S\d{1,2}E\d{1,2}|S\d{1,2}E?\d{0,2}|E\d{1,2}|\d{1,2}x\d{1,2})\b", "");
    t = rx::replace_i(&t, r"\s*\d{1,2}(-\d{1,2})?\s*сезон\s*.*$", "");
    t = rx::replace_i(&t, r"\b(Сезон|Season)\s*\d{1,2}(?!\d).*$", "");

    t = rx::replace_i(&t, r"\b(2160p|1080p|720p|480p)\b", "");
    t = rx::replace_i(&t, r"\b(WEB[-\s]?DL|WEB[-\s]?Rip|BDRip|BDRemux|HDRip|BluRay|BRRip|DVDRip|HDTV)\b", "");
    t = rx::replace_i(&t, r"\b(x264|x265|h\.?264|h\.?265|hevc|avc|aac|ac3|dts)\b", "");

    t = rx::replace(&t, r"[\[\]\|]", " ");
    t = rx::replace(&t, r"\s{2,}", " ").trim().trim_end_matches([' ', '/', '-', '|']).to_string();
    t = rx::replace_i(&t, r"[.\s]+-\s*[A-Za-z0-9][A-Za-z0-9.-]*$", "");
    t.trim().trim_end_matches([' ', '-']).to_string()
}

pub fn parse_response_json(json: &str) -> Option<BitruApiResponse> {
    if util::is_blank(json) {
        return None;
    }
    serde_json::from_str(json).ok()
}

pub fn parse_torrents_from_json(json: &str, host_url: &str) -> Vec<TorrentDetails> {
    match parse_response_json(json) {
        Some(r) => parse_torrents_from_response(Some(&r), host_url),
        None => Vec::new(),
    }
}

pub fn parse_torrents_from_response(response: Option<&BitruApiResponse>, host_url: &str) -> Vec<TorrentDetails> {
    let Some(response) = response else { return Vec::new() };
    if response.has_error() {
        return Vec::new();
    }
    let Some(items) = response.result.as_ref().and_then(|r| r.items.as_ref()) else { return Vec::new() };
    items.iter().filter_map(|w| w.item.as_ref()).filter_map(|i| map_to_torrent_details(i, host_url)).collect()
}

pub fn map_to_torrent_details(item: &BitruApiItemInner, host_url: &str) -> Option<TorrentDetails> {
    let (torrent, info, template) = (item.torrent.as_ref()?, item.info.as_ref()?, item.template.as_ref()?);
    let types = categories::try_get_types(template.category.as_deref(), template.subsection.as_deref())?;

    let info_name = info.name.clone().unwrap_or_default();
    let orig_name = template.orig_name.clone().unwrap_or_default();

    let mut name = clean_title_for_search(&info_name).trim().to_string();
    let mut originalname = clean_title_for_search(&orig_name).trim().to_string();
    if util::is_blank(&name) {
        name = info_name.trim().to_string();
    }
    if util::is_blank(&originalname) {
        originalname = orig_name.trim().to_string();
    }
    let year_display = year_to_display_string(&info.year);
    let relased = year_to_released(&info.year);

    let name_raw = info_name.trim();
    let orig_raw = orig_name.trim();
    let mut title_part = name_raw.to_string();
    if !util::is_blank(orig_raw) {
        title_part.push_str(" / ");
        title_part.push_str(orig_raw);
    }
    if !year_display.is_empty() {
        title_part.push_str(&format!(" ({year_display})"));
    }
    if let Some(q) = template.video.as_ref().and_then(|v| v.quality.as_ref()) {
        title_part.push(' ');
        title_part.push_str(q);
    }
    if let Some(o) = template.other.as_deref().filter(|o| !util::is_blank(o)) {
        title_part.push_str(" | ");
        title_part.push_str(o);
    }

    let host_url = host_url.trim_end_matches('/');
    let url = format!("{host_url}/details.php?id={}", torrent.id);
    let size_name = format_size(torrent.size);
    let create_time: DateTime<Utc> = Utc.timestamp_opt(torrent.added, 0).single().unwrap_or_else(crab_core::time::min);

    let download_url = match torrent.file.as_deref() {
        Some(f) if !util::is_blank(f) && f.to_lowercase().starts_with("http") => f.to_string(),
        _ => format!("{host_url}/api.php?download={}", torrent.id),
    };

    let mut t = TorrentDetails::new("bitru", types, url, util::html_decode(title_part.trim()));
    t.sid = torrent.seeders;
    t.pir = torrent.leechers;
    t.sizeName = size_name;
    t.createTime = create_time;
    t.name = name.trim().to_string();
    t.originalname = originalname.trim().to_string();
    t.relased = relased;
    t._sn = download_url;
    Some(t)
}

/// Unix seconds of the start of `from_date`'s calendar day (UTC); used as the older-than filter.
pub fn unix_from_date(from_date: NaiveDate) -> i64 {
    from_date.and_hms_opt(0, 0, 0).map(|d| Utc.from_utc_datetime(&d).timestamp()).unwrap_or(0)
}

fn year_to_display_string(year: &Value) -> String {
    match year {
        Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        other => other.to_string(),
    }
}

fn year_to_released(year: &Value) -> i32 {
    match year {
        Value::Null => 0,
        Value::Number(n) => n.as_i64().map(|v| v as i32).unwrap_or(0),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return 0;
            }
            let first = match s.find('-') {
                Some(d) if d > 0 => s[..d].trim(),
                _ => s,
            };
            if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
                first.parse().unwrap_or(0)
            } else {
                0
            }
        }
        _ => 0,
    }
}

fn format_size(bytes: i64) -> String {
    if bytes < 1000 * 1024 {
        format!("{:.2} КБ", bytes as f64 / 1024.0)
    } else if bytes < 1000 * 1_048_576 {
        format!("{:.2} МБ", bytes as f64 / 1_048_576.0)
    } else if bytes < 1000 * 1_073_741_824 {
        format!("{:.2} ГБ", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.2} ТБ", bytes as f64 / 1_099_511_627_776.0)
    }
}
