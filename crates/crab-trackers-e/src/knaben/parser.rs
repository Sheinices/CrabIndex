//! Knaben hit → FileDB row mapping and title normalisation.

use chrono::{DateTime, Utc};
use crab_core::models::TorrentDetails;
use crab_core::rx;
use crab_core::util::html_decode;

use super::models::KnabenHit;

pub const TRACKER_NAME: &str = "knaben";

fn trim_end_chars<'a>(s: &'a str, chars: &[char]) -> &'a str {
    s.trim_end_matches(|c| chars.contains(&c))
}

/// Strip metadata for the search key. Series: text before `S01E05`. Movies: cut at year, drop tags.
pub fn clean_title_for_search(title: &str) -> String {
    if title.trim().is_empty() {
        return title.to_string();
    }
    let mut t = title.trim().to_string();

    t = rx::replace(&t, r"\[[^\]]*\]", " ");
    let series = crab_core::rx::captures_i(&t, r"^(.+?)\s+S\d{1,2}E\d{1,2}\b").and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    match series {
        Some(s) if !s.is_empty() => t = s.trim().to_string(),
        _ => {
            if let Some(idx) = rx::captures(&t, r"[\(\[](\d{4})[\)\]]").and_then(|c| c.get(0).map(|m| m.start())) {
                if idx > 0 {
                    t = t[..idx].to_string();
                }
            }
            t = crab_core::rx::replace_i(&t, r"\b(S\d{1,2}E\d{1,2}|S\d{1,2}E?\d{0,2}|E\d{1,2}|\d{1,2}x\d{1,2})\b", "");
            t = crab_core::rx::replace_i(&t, r"\b(Сезон|Season)\s*\d{1,2}(?!\d).*$", "");
        }
    }

    t = crab_core::rx::replace_i(&t, r"\b(2160p|1080p|720p|480p)\b", "");
    t = crab_core::rx::replace_i(&t, r"\b(HDR10?|DV|HDR|SDR|10bit)\b", "");
    t = crab_core::rx::replace_i(&t, r"\b(WEB[-\s]?DL|WEB[-\s]?Rip|WEB\b|BDRip|BDRemux|HDRip|BluRay|BRRip|DVDRip|HDTV)\b", "");
    t = crab_core::rx::replace_i(&t, r"\b(x264|x265|xvid|h\.?264|h\.?265|hevc|avc|aac|ac3|dts)\b", "");
    t = crab_core::rx::replace_i(&t, r"\b(AMZN|NF|DS4K|DD\s*5\s*1|DD5\.?1|DDPA|DDP5\.?1|Atmos|DDP?\s*5\.?1|playWEB)\b", "");
    t = crab_core::rx::replace_i(&t, r"\b(ESub|Sub)\b", "");
    t = rx::replace(&t, r"\.", " ");
    t = rx::replace(&t, r"[\[\]\|]", " ");
    t = trim_end_chars(rx::replace(&t, r"\s{2,}", " ").trim(), &[' ', '/', '-', '|']).to_string();
    t = crab_core::rx::replace_i(&t, r"[.\s]+-\s*[A-Za-z0-9][A-Za-z0-9.-]*$", "");
    t = trim_end_chars(t.trim(), &[' ', '-']).to_string();
    if t.trim().is_empty() {
        title.to_string()
    } else {
        t
    }
}

/// Name + year. Supports `(2026)`, `[2026, ...]` and a standalone `2026`.
pub fn parse_name_and_year(title: &str) -> (Option<String>, i32) {
    if title.trim().is_empty() {
        return (None, 0);
    }
    let mut name = rx::replace(title.trim(), r"\s+\|\s+[^|]+$", "").trim().to_string();
    if name.trim().is_empty() {
        return (None, 0);
    }
    let mut relased = 0;

    let m = rx::captures(&name, r"[\(\[](\d{4})[\)\],\s]").and_then(|c| {
        let y = c.get(1)?.as_str().parse::<i32>().ok()?;
        Some((y, c.get(0)?.start()))
    });
    if let Some((y, idx)) = m {
        relased = y;
        if idx > 0 {
            name = trim_end_chars(&name[..idx], &[' ', '/', '-', '|']).to_string();
        }
    } else {
        let ym = rx::group(&name, r"\b(19|20)\d{2}\b", 0);
        if let Ok(y2) = ym.parse::<i32>() {
            relased = y2;
            name = rx::replace(&name, r"\b(19|20)\d{2}\b", "").trim().to_string();
        }
    }
    let name = clean_title_for_search(&name);
    if name.trim().is_empty() {
        (Some(title.trim().to_string()), relased)
    } else {
        (Some(name), relased)
    }
}

/// Normalise a title for FileDB: lowercase resolution tags, `.HDR` → ` HDR`, Dolby Vision/10-bit → `HDR`.
pub fn build_title_for_filedb(original_title: &str) -> String {
    if original_title.trim().is_empty() {
        return original_title.to_string();
    }
    let mut t = original_title.trim().to_string();
    t = crab_core::rx::replace_i(&t, r"\b2160p\b", "2160p");
    t = crab_core::rx::replace_i(&t, r"\b1080p\b", "1080p");
    t = crab_core::rx::replace_i(&t, r"\b720p\b", "720p");
    t = crab_core::rx::replace_i(&t, r"\.(HDR10?)\b", " $1");
    if crab_core::rx::is_match_i(&t, r"(dolby\s*vision|10-?bit)") && !crab_core::rx::is_match_i(&t, r"(\.|\[|,| )hdr") {
        t.push_str(" HDR");
    }
    t
}

pub fn map_to_torrent_details(h: &KnabenHit) -> Option<TorrentDetails> {
    if h.title.trim().is_empty() {
        return None;
    }
    let types = types_from_category_id(h.category_id.as_deref());

    let mut url = if !h.details.trim().is_empty() { h.details.clone() } else { h.link.clone() };
    if url.trim().is_empty() && !h.id.trim().is_empty() {
        url = format!("https://knaben.xyz/?id={}", h.id);
    }
    if url.trim().is_empty() {
        return None;
    }

    let mut title = html_decode(h.title.trim());
    let create_time = parse_date(&h.date).or_else(|| parse_date(&h.last_seen)).unwrap_or_else(Utc::now);
    let update_time = parse_date(&h.last_seen).unwrap_or(create_time);
    let (name, relased) = parse_name_and_year(&title);
    let name = name.unwrap_or_default();

    title = build_title_for_filedb(&title);
    if !h.tracker.trim().is_empty() && !title.contains(&h.tracker) {
        title = format!("{title} | {}", h.tracker);
    }

    let mut t = TorrentDetails::new(TRACKER_NAME, types, url, title);
    t.sid = h.seeders;
    t.pir = h.peers;
    t.sizeName = format_size(h.bytes);
    t.createTime = create_time;
    t.updateTime = update_time;
    t.magnet = if h.magnet_url.trim().is_empty() { String::new() } else { h.magnet_url.clone() };
    t._sn = if h.magnet_url.trim().is_empty() && !h.link.trim().is_empty() { h.link.clone() } else { String::new() };
    t.originalname = name.clone();
    t.name = name;
    t.relased = relased;
    Some(t)
}

/// Resolution hint from Knaben category ids (2160 / 1080 / 720, else 480).
pub fn quality_from_category_id(ids: Option<&[i32]>) -> i32 {
    let Some(ids) = ids else { return 480 };
    for &id in ids {
        if id == 2003000 || id == 3003000 {
            return 2160;
        }
        if id == 2001000 || id == 3001000 {
            return 1080;
        }
        if id == 2002000 || id == 3002000 {
            return 720;
        }
    }
    480
}

pub fn types_from_category_id(ids: Option<&[i32]>) -> &'static [&'static str] {
    let ids = match ids {
        Some(i) if !i.is_empty() => i,
        _ => return &["movie", "serial"],
    };
    for &id in ids {
        if (2000000..3000000).contains(&id) {
            return &["serial"];
        }
        if (3000000..4000000).contains(&id) {
            return &["movie"];
        }
    }
    &["movie", "serial"]
}

fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&Utc));
    }
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(d.with_timezone(&Utc));
    }
    crab_core::time::parse_net(s)
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
