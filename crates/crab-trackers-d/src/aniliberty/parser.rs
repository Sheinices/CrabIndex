//! Maps aniliberty API torrents to FileDB rows.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

use crab_core::models::TorrentDetails;
use crab_core::{rx, time, util};

use super::models::{AnilibertyApiResponse, AnilibertyTorrent};

pub const TRACKER_NAME: &str = "aniliberty";

pub fn map_page_torrents(response: &AnilibertyApiResponse, host: &str) -> Vec<TorrentDetails> {
    match &response.data {
        Some(data) if !data.is_empty() => data.iter().filter_map(|t| map_api_torrent(t, host)).collect(),
        _ => Vec::new(),
    }
}

fn opt_trim(s: Option<&String>) -> String {
    s.map(|x| x.trim().to_string()).unwrap_or_default()
}

pub fn map_api_torrent(api: &AnilibertyTorrent, host: &str) -> Option<TorrentDetails> {
    let magnet = api.magnet.clone().unwrap_or_default();
    let release = api.release.as_ref()?;
    if util::is_blank(&magnet) {
        return None;
    }
    let name = opt_trim(release.name.as_ref().and_then(|n| n.main.as_ref()));
    let originalname = opt_trim(release.name.as_ref().and_then(|n| n.english.as_ref()));
    if util::is_blank(&name) && util::is_blank(&originalname) {
        return None;
    }

    let quality_info = extract_quality_info(api.label.as_deref().unwrap_or(""));

    let base_title = if !util::is_blank(&name) && !util::is_blank(&originalname) && name != originalname {
        format!("{name} / {originalname}")
    } else if !util::is_blank(&name) {
        name.clone()
    } else if !util::is_blank(&originalname) {
        originalname.clone()
    } else {
        "Unknown".to_string()
    };

    let mut title = base_title;
    if let Some(y) = release.year {
        title.push_str(&format!(" / {y}"));
    }
    if !util::is_blank(&quality_info) {
        title.push_str(&format!(" / {quality_info}"));
    }

    let types = determine_types(release.type_.as_ref().and_then(|t| t.value.as_deref()).unwrap_or(""));

    let mut create_time = api.created_at.as_deref().filter(|s| !util::is_blank(s)).and_then(parse_any_date).unwrap_or_else(time::min);
    if time::is_min(&create_time) {
        create_time = time::now();
    }
    let mut update_time = api.updated_at.as_deref().filter(|s| !util::is_blank(s)).and_then(parse_any_date).unwrap_or_else(time::min);
    if time::is_min(&update_time) {
        update_time = create_time;
    }

    let hash = api.hash.clone().unwrap_or_default();
    let base_url = match release.alias.as_deref().filter(|a| !util::is_blank(a)) {
        Some(alias) => format!("{host}/anime/releases/release/{alias}"),
        None => format!("{host}/api/v1/anime/torrents/{hash}"),
    };
    let torrent_url = format!("{base_url}?hash={hash}");

    let mut t = TorrentDetails::new(TRACKER_NAME, types, torrent_url, title);
    t.sid = api.seeders;
    t.pir = api.leechers;
    t.createTime = create_time;
    t.updateTime = update_time;
    t.name = name;
    t.originalname = originalname;
    t.relased = release.year.unwrap_or(0);
    t.magnet = magnet;
    t.sizeName = format_size(api.size);
    t.quality = parse_quality(api.quality.as_ref().and_then(|q| q.value.as_deref()).unwrap_or(""));
    t.videotype = api.type_.as_ref().and_then(|t| t.value.as_deref()).map(|v| v.to_lowercase()).unwrap_or_default();
    Some(t)
}

/// Parse an API timestamp. Strings with an offset are converted exactly; strings
/// without one are read as server-local time.
pub fn parse_any_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%:z", "%Y-%m-%dT%H:%M:%S%z"] {
        if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    let naive = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
        .or_else(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))?;
    chrono::Local.from_local_datetime(&naive).earliest().map(|d| d.with_timezone(&Utc))
}

pub fn determine_types(type_value: &str) -> &'static [&'static str] {
    if util::is_blank(type_value) {
        return &["anime"];
    }
    match type_value.to_uppercase().as_str() {
        "MOVIE" => &["anime", "movie"],
        "OVA" | "OAD" => &["anime", "ova"],
        "SPECIAL" => &["anime", "special"],
        "ONA" | "WEB" => &["anime", "ona"],
        "DORAMA" => &["dorama"],
        _ => &["anime", "serial"],
    }
}

pub fn format_size(bytes: i64) -> String {
    if bytes < 1_073_741_824 {
        format!("{:.2} Mb", bytes as f64 / 1_048_576.0)
    } else if bytes < 1_099_511_627_776 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.2} TB", bytes as f64 / 1_099_511_627_776.0)
    }
}

pub fn parse_quality(quality_value: &str) -> i32 {
    if util::is_blank(quality_value) {
        return 480;
    }
    let q = quality_value.to_lowercase();
    let q = q.trim();
    if q.contains("4k") || q.contains("2160p") || q.contains("uhd") {
        return 2160;
    }
    let g = rx::groups(q, r"(\d{3,4})p?");
    if !g[0].is_empty() {
        if let Ok(quality) = g[1].parse::<i32>() {
            return if quality >= 2160 {
                2160
            } else if quality >= 1080 {
                1080
            } else if quality >= 720 {
                720
            } else if quality >= 480 {
                480
            } else {
                quality
            };
        }
    }
    480
}

pub fn extract_quality_info(label: &str) -> String {
    if util::is_blank(label) {
        return String::new();
    }
    rx::group(label, r"(\[[^\]]+\](?:\s*\[[^\]]+\])*)\s*$", 1).trim().to_string()
}
