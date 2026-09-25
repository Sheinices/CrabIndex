//! viruseproject.tv listing / detail page parsing.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use std::collections::{HashMap, HashSet};

use crab_core::models::TorrentDetails;
use crab_core::{rx, time, util};

use super::categories;
use crate::common::{has_cyrillic, has_latin, try_int};

pub const TRACKER_NAME: &str = "viruseproject";

const ITEM_HREF_RE: &str = r#"(?is)<h3\s+class="catItemTitle">\s*<a\s+href="([^"]+)""#;
const PAGINATION_END_RE: &str = r#"(?is)<li\s+class="pagination-end">\s*<a[^>]+href="[^"]*?[?&]start=(\d+)""#;
const ITEM_TITLE_RE: &str = r#"(?is)<h2\s+class="itemTitle">\s*(.+?)\s*</h2>"#;
const ITEM_DATE_RE: &str = r#"(?is)<span\s+class="itemDateCreated">\s*(.+?)\s*</span>"#;
const EXTRA_FIELD_RE: &str =
    r#"(?is)<span\s+class="itemExtraFieldsLabel">\s*([^<]+?)\s*</span>\s*<span\s+class="itemExtraFieldsValue">\s*([^<]+?)\s*</span>"#;
const ATTACHMENT_RE: &str =
    r#"(?is)<a\s+title="([^"]+?\.torrent)"\s+href="([^"]+/download/(\d+)_[a-f0-9]+)"\s*>\s*([^<]+?)\s*</a>"#;
const YEAR_IN_TEXT_RE: &str = r"\b(19|20)\d{2}\b";
const RESOLUTION_RE: &str = r"(?i)\b(2160|1440|1080|720|480|400)p\b";
const SIZE_RE: &str = r"(?i)размер\s+([0-9]+(?:[.,][0-9]+)?)\s*(Гб|Мб|Тб|Кб|GB|MB|TB|KB)";
const PAREN_EN_RE: &str = r"^([^()]+?)\s*\(([^()]+)\)\s*$";
const SEASON_INFO_RE: &str = r"(?i)^сезон\s+\d+";
const EPISODE_INFO_RE: &str = r"(?i)^\d+(\s*[-,]\s*\d+)*\s+из\s+\d+$";
const WHITESPACE_RE: &str = r"[\s ]+";
const STRIP_TAGS_RE: &str = r"<[^>]+>";

const RUSSIAN_MONTHS: &[(&str, u32)] = &[
    ("янв", 1),
    ("фев", 2),
    ("мар", 3),
    ("апр", 4),
    ("май", 5),
    ("мая", 5),
    ("июн", 6),
    ("июл", 7),
    ("авг", 8),
    ("сен", 9),
    ("окт", 10),
    ("ноя", 11),
    ("дек", 12),
];

/// Row plus the absolute `.torrent` download link.
#[derive(Clone, Debug, Default)]
pub struct ViruseprojectDetails {
    pub t: TorrentDetails,
    pub download_uri: String,
}

impl AsRef<TorrentDetails> for ViruseprojectDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for ViruseprojectDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

pub fn try_get_types(cat: &str) -> Option<&'static [&'static str]> {
    categories::get(cat).map(|c| c.types)
}

pub fn get_page_step(cat: &str) -> i32 {
    match categories::get(cat) {
        Some(c) if c.page_step > 0 => c.page_step,
        _ => 10,
    }
}

pub fn detect_last_page(body: &str, step: i32) -> i32 {
    if step <= 0 || body.is_empty() {
        return 1;
    }
    let g = rx::groups(body, PAGINATION_END_RE);
    if !g[0].is_empty() {
        if let Some(last) = try_int(&g[1]).filter(|l| *l > 0) {
            return last / step + 1;
        }
    }
    1
}

pub fn extract_post_urls(body: &str, host: &str) -> Vec<String> {
    let mut out = Vec::new();
    if body.is_empty() || util::is_blank(host) {
        return out;
    }
    let host = host.trim_end_matches('/');
    let mut seen = HashSet::new();
    for g in rx::all_groups(body, ITEM_HREF_RE) {
        let mut u = util::html_decode(&g[1]).trim().to_string();
        if util::is_blank(&u) {
            continue;
        }
        if u.starts_with('/') {
            u = format!("{host}{u}");
        }
        if seen.insert(u.to_lowercase()) {
            out.push(u);
        }
    }
    out
}

pub fn parse_detail_html(dhtml: &str, post_url: &str, host: &str, types: &[&str]) -> Vec<ViruseprojectDetails> {
    let mut out = Vec::new();
    if util::is_blank(dhtml) || util::is_blank(post_url) || types.is_empty() {
        return out;
    }
    let host = host.trim_end_matches('/');
    let raw_title = clean_text(&extract_match(ITEM_TITLE_RE, dhtml));
    if util::is_blank(&raw_title) {
        return out;
    }

    let fields = extract_extra_fields(dhtml);
    let year = fields.get("Год выпуска").and_then(|y| try_int(y)).unwrap_or(0);
    let video_quality = fields.get("Качество видео").map(|v| v.trim().to_string()).unwrap_or_default();

    let create_time = parse_russian_date(&clean_text(&extract_match(ITEM_DATE_RE, dhtml)));
    let (name_ru, name_en) = parse_names(&raw_title);
    let title_has_year = rx::is_match(&raw_title, YEAR_IN_TEXT_RE);

    let mut base_title = raw_title.clone();
    if !title_has_year && year > 0 {
        base_title = format!("{base_title} ({year})");
    }
    if !util::is_blank(&video_quality) {
        base_title = format!("{base_title} [{video_quality}]");
    }

    for att in rx::all_groups(dhtml, ATTACHMENT_RE) {
        let file_title = att[1].trim().to_string();
        let mut download_url = util::html_decode(&att[2]).trim().to_string();
        if download_url.starts_with('/') {
            download_url = format!("{host}{download_url}");
        }
        let download_id = att[3].trim().to_string();
        let link_text = clean_text(&att[4]);

        let mut res_int = 400;
        let mut resolution = "400p".to_string();
        let rm = rx::groups(&file_title, RESOLUTION_RE);
        if !rm[0].is_empty() {
            if let Some(r) = try_int(&rm[1]) {
                res_int = r;
                resolution = format!("{r}p");
            }
        }

        let mut size_name = String::new();
        let mut size_bytes = 0.0;
        let sm = rx::groups(&link_text, SIZE_RE);
        if !sm[0].is_empty() {
            let num_raw = sm[1].replace(',', ".");
            let unit = &sm[2];
            size_name = format!("{num_raw} {unit}");
            if let Ok(mut num) = num_raw.parse::<f64>() {
                match unit.to_lowercase().as_str() {
                    "тб" | "tb" => num *= 1024.0 * 1024.0,
                    "гб" | "gb" => num *= 1024.0,
                    "кб" | "kb" => num /= 1024.0,
                    _ => {}
                }
                size_bytes = num * 1_048_576.0;
            }
        }

        let title = format!("{base_title} [{resolution}]");
        let record_url = format!("{post_url}#q={res_int}&id={download_id}");
        let original = if util::is_blank(&name_en) { name_ru.clone() } else { name_en.clone() };

        let mut t = TorrentDetails::new(TRACKER_NAME, types, record_url, title);
        // The site doesn't expose peer counts.
        t.sid = 1;
        t.sizeName = size_name;
        t.size = size_bytes;
        t.createTime = create_time;
        t.updateTime = create_time;
        t.name = name_ru.clone();
        t.originalname = original;
        t.relased = year;
        t.quality = res_int;
        t.videotype = video_quality.clone();
        out.push(ViruseprojectDetails { t, download_uri: download_url });
    }
    out
}

pub fn parse_names(raw_title: &str) -> (String, String) {
    let raw_title = raw_title.trim();
    if raw_title.is_empty() {
        return (String::new(), String::new());
    }

    let paren = rx::groups(raw_title, PAREN_EN_RE);
    if !paren[0].is_empty() {
        let ru = paren[1].trim();
        let en = paren[2].trim();
        if has_cyrillic(ru) && has_latin(en) && !en.contains('/') {
            return (ru.to_string(), en.to_string());
        }
    }

    if !raw_title.contains('/') {
        return (raw_title.to_string(), raw_title.to_string());
    }

    let clean: Vec<&str> = raw_title
        .split('/')
        .map(|p| p.trim())
        .filter(|pt| !pt.is_empty() && !is_year_only(pt) && !rx::is_match(pt, SEASON_INFO_RE) && !rx::is_match(pt, EPISODE_INFO_RE))
        .collect();
    if clean.is_empty() {
        return (raw_title.to_string(), raw_title.to_string());
    }

    let name_ru = clean[0].to_string();
    let name_en = clean.iter().skip(1).find(|p| has_latin(p) && !has_cyrillic(p)).map(|p| p.to_string()).unwrap_or_else(|| name_ru.clone());
    (name_ru, name_en)
}

/// Parse «Четверг, 13 Февраль 2025 00:00» (weekday optional) as UTC; now on failure.
pub fn parse_russian_date(s: &str) -> DateTime<Utc> {
    if util::is_blank(s) {
        return time::now();
    }
    let s = match s.find(',') {
        Some(i) => &s[i + 1..],
        None => s,
    };
    let parts = rx::split(s.trim(), WHITESPACE_RE);
    if parts.len() < 3 {
        return time::now();
    }
    let Some(day) = try_int(&parts[0]) else { return time::now() };
    let month = month_from_russian(&parts[1]);
    if month == 0 {
        return time::now();
    }
    let Some(year) = try_int(&parts[2]) else { return time::now() };

    let (mut hour, mut minute) = (0, 0);
    if parts.len() >= 4 {
        let hm: Vec<&str> = parts[3].splitn(2, ':').collect();
        if hm.len() == 2 {
            hour = try_int(hm[0]).unwrap_or(0);
            minute = try_int(hm[1]).unwrap_or(0);
        }
    }

    u32::try_from(day)
        .ok()
        .and_then(|d| NaiveDate::from_ymd_opt(year, month, d))
        .and_then(|d| d.and_hms_opt(u32::try_from(hour).ok()?, u32::try_from(minute).ok()?, 0))
        .map(|n| Utc.from_utc_datetime(&n))
        .unwrap_or_else(time::now)
}

fn month_from_russian(s: &str) -> u32 {
    let s = s.trim().to_lowercase();
    RUSSIAN_MONTHS.iter().find(|(p, _)| s.starts_with(p)).map(|(_, m)| *m).unwrap_or(0)
}

fn extract_extra_fields(body: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if body.is_empty() {
        return out;
    }
    for g in rx::all_groups(body, EXTRA_FIELD_RE) {
        let mut label = clean_text(&g[1]);
        if label.ends_with(':') {
            label = label[..label.len() - 1].trim().to_string();
        }
        let value = clean_text(&g[2]);
        if !util::is_blank(&label) {
            out.insert(label, value);
        }
    }
    out
}

fn extract_match(pattern: &str, body: &str) -> String {
    let g = rx::groups(body, pattern);
    if g[0].is_empty() || g.len() < 2 {
        return String::new();
    }
    util::html_decode(&g[1]).trim().to_string()
}

fn clean_text(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = rx::replace(s, STRIP_TAGS_RE, "");
    let s = util::html_decode(&s);
    rx::replace(&s, WHITESPACE_RE, " ").trim().to_string()
}

fn is_year_only(s: &str) -> bool {
    let s = s.trim();
    if s.chars().count() != 4 {
        return false;
    }
    matches!(try_int(s), Some(n) if (1900..=2100).contains(&n))
}
