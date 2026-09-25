//! ultradox.vip HTML parsing (listing rows, detail magnets, pager, titles).
//!
//! The site 307-redirects to numbered `00N.ultradox.vip` mirrors. Listing magnets have an
//! empty btih - real magnets live on detail pages. Stored urls stay on the configured host.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::rx;
use crab_core::time;
use crab_core::util::html_decode;

pub const TRACKER_NAME: &str = "ultradox";

/// Search-engine Referer required by the site's nginx gate (own origin → 503).
pub const SEARCH_ENGINE_REFERER: &str = "https://www.google.com/";

const ROW_SPLIT_RE: &str = r#"<tr>\s*<td class="torrent-table-date">"#;
const ROW_DATE_RE: &str = r"^([^<]+)</td>";
const ROW_TIME_RE: &str = r"([0-9]{2}):([0-9]{2})";
const ROW_DETAIL_LINK_RE: &str = r##"(?s)<td class="torrent-table-href">\s*<a[^>]+href="([^"#]+)"[^>]*>([\s\S]*?)</a>"##;
const ROW_IMDB_RE: &str = r#"(?s)<span\s+data-clipboard-text="https://www\.imdb\.com/title/(tt[0-9]+)/?""#;
const ROW_SPAN_QUALITY_RE: &str = r#"(?s)<span[^>]*style="font-weight:\s*bold;?"[^>]*>([\s\S]*?)</span>"#;
const TAGS_RE: &str = r"<[^>]+>";
const DETAIL_MAGNET_RE: &str = r#"magnet:\?xt=urn:btih:([A-Fa-f0-9]+)&xl=([0-9]+)&dn=([^&"<\s]+)"#;
const PAGE_NUM_RE: &str = r"/page/([0-9]+)/";
const PAGES_BLOCK_RE: &str = r#"<div\s+class="[^"]*\bpages\b[^"]*">([\s\S]*?)</div>"#;
const TITLE_YEAR_RE: &str = r"\(([0-9]{4})\)";
const TITLE_NAME_RE: &str = r"^([^(\[]+)";
const TITLE_SEASON_RE: &str = r"\(\s*\d+\s*(?:-\s*\d+\s*)?сезон";
const DETAIL_YEAR_RE: &str = r#"(?s)itemprop="copyrightYear"[^>]*>\s*<span>[^<]*</span>\s*([0-9]{4})"#;
const DN_YEAR_RE: &str = r"^(?:19|20)\d{2}";
const DN_SEASON_RE: &str = r"^[Ss]\d{1,2}(?:[Ee]\d{1,3})?$";
const DN_RES_RE: &str = r"^\d{3,4}[pP]$";
const QUALITY_RES_RE: &str = r"([0-9]{3,4})[pP]";
const WHITESPACE_RE: &str = r"\s+";

const DN_STOP_TOKENS: [&str; 23] = [
    "bdrip", "webrip", "webdl", "web-dl", "web-dlrip", "hdrip", "dvdrip", "camrip", "telecine", "ts", "hdtv", "bluray", "proper",
    "repack", "avi", "mkv", "mp4", "x264", "x265", "h264", "h265", "hevc", "avc",
];

const QUALITY_TAGS: [&str; 8] = ["BDRip", "DVDRip", "HDRip", "WEBRip", "WEB-DL", "CAMRip", "CamRip", "TS"];

/// Europe/Moscow offset (UTC+3, no DST).
const MOSCOW_OFFSET_HOURS: i64 = 3;

#[derive(Clone, Debug)]
pub struct UltradoxListingItem {
    /// `time::min()` when unknown.
    pub create_time: DateTime<Utc>,
    pub detail_url: String,
    pub title: String,
    pub imdb: String,
}

impl Default for UltradoxListingItem {
    fn default() -> Self {
        UltradoxListingItem { create_time: time::min(), detail_url: String::new(), title: String::new(), imdb: String::new() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct UltradoxDetailInfo {
    pub year: i32,
    pub original: String,
}

#[derive(Clone, Debug, Default)]
pub struct UltradoxMagnetVariant {
    pub hash: String,
    pub bytes: i64,
    pub dn: String,
    pub magnet: String,
    pub quality: String,
}

pub fn listing_url(host: &str, section_path: &str, page: i32) -> String {
    let host = host.trim_end_matches('/');
    let section = section_path.trim_matches('/');
    if page <= 0 {
        format!("{host}/{section}/")
    } else {
        format!("{host}/{section}/page/{page}/")
    }
}

/// Last listing page from `div.pages` blocks. Takes the **smallest** last-page among
/// section-matching pagers: the footer pager is inflated and in-table widgets inflate the
/// other way. Falls back to a whole-body scan, then 1.
pub fn last_page_from_html(body: &str, section_path: Option<&str>) -> i32 {
    if body.trim().is_empty() {
        return 1;
    }
    let section = section_path.unwrap_or("").trim_matches('/');
    let (pattern, ic) = if section.is_empty() {
        (PAGE_NUM_RE.to_string(), false)
    } else {
        (format!("/{}/page/([0-9]+)/", regex::escape(section)), true)
    };

    let mut chosen = 0;
    for block in crab_core::rx::all_groups_i(body, PAGES_BLOCK_RE) {
        let from_block = max_page_in(&block[1], &pattern, ic);
        if from_block <= 0 {
            continue;
        }
        chosen = if chosen == 0 { from_block } else { chosen.min(from_block) };
    }
    if chosen > 0 {
        return chosen;
    }
    let fallback = max_page_in(body, &pattern, ic);
    if fallback > 0 {
        fallback
    } else {
        1
    }
}

fn max_page_in(haystack: &str, pattern: &str, ic: bool) -> i32 {
    if haystack.is_empty() {
        return 0;
    }
    let all = if ic { crab_core::rx::all_groups_i(haystack, pattern) } else { rx::all_groups(haystack, pattern) };
    all.iter().filter_map(|g| g[1].parse::<i32>().ok()).fold(0, |m, n| m.max(n))
}

/// Drop map slots past the live pager (ghost tails from a polluted maxPage). Returns removed count.
pub fn prune_pages_beyond_max(tasks: Option<&mut Vec<TaskParse>>, max_page: i32) -> i32 {
    let Some(tasks) = tasks else { return 0 };
    if tasks.is_empty() {
        return 0;
    }
    let max_page = max_page.max(1);
    let before = tasks.len();
    tasks.retain(|t| t.page <= max_page);
    (before - tasks.len()) as i32
}

pub fn parse_listing_html(body: &str) -> Vec<UltradoxListingItem> {
    let mut out = Vec::new();
    if body.trim().is_empty() {
        return out;
    }
    let chunks = rx::split(body, ROW_SPLIT_RE);
    for chunk in chunks.iter().skip(1) {
        let row = chunk.trim();
        if row.is_empty() {
            continue;
        }
        let Some(date) = rx::captures(row, ROW_DATE_RE).and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string())) else {
            continue;
        };
        let create_time = parse_row_date(&date);
        if time::is_min(&create_time) {
            continue;
        }
        let Some(link) = crab_core::rx::captures_i(row, ROW_DETAIL_LINK_RE) else { continue };
        let href = link.get(1).map(|m| m.as_str()).unwrap_or("");
        let inner = link.get(2).map(|m| m.as_str()).unwrap_or("");
        let detail_url = html_decode(href).trim().to_string();
        if detail_url.is_empty() {
            continue;
        }
        let title = flatten_title(inner);
        if title.is_empty() {
            continue;
        }
        let imdb = crab_core::rx::group_i(row, ROW_IMDB_RE, 1);
        out.push(UltradoxListingItem { create_time, detail_url, title, imdb });
    }
    out
}

fn moscow_to_utc(local: NaiveDateTime) -> DateTime<Utc> {
    Utc.from_utc_datetime(&(local - Duration::hours(MOSCOW_OFFSET_HOURS)))
}

/// Absolute `DD-MM-YYYY, HH:MM`, `Сегодня, HH:MM` or `Вчера, HH:MM` (Moscow time → UTC).
/// Returns `time::min()` when unparsable.
pub fn parse_row_date(s: &str) -> DateTime<Utc> {
    let s = s.trim();
    if s.is_empty() {
        return time::min();
    }
    if let Ok(abs) = NaiveDateTime::parse_from_str(s, "%d-%m-%Y, %H:%M") {
        return moscow_to_utc(abs);
    }
    let relative_days = if s.starts_with("Сегодня") {
        0
    } else if s.starts_with("Вчера") {
        -1
    } else {
        return time::min();
    };
    let hm = rx::groups(s, ROW_TIME_RE);
    if hm[0].is_empty() {
        return time::min();
    }
    let hour: u32 = hm[1].parse().unwrap_or(0);
    let minute: u32 = hm[2].parse().unwrap_or(0);
    let now_local = Utc::now().naive_utc() + Duration::hours(MOSCOW_OFFSET_HOURS);
    let day: NaiveDate = now_local.date() + Duration::days(relative_days);
    match day.and_hms_opt(hour, minute, 0) {
        Some(local) => moscow_to_utc(local),
        None => time::min(),
    }
}

/// Magnet variants + year/original from a detail page. `None` when no magnets are present.
pub fn try_parse_detail_html(body: &str) -> Option<(Vec<UltradoxMagnetVariant>, UltradoxDetailInfo)> {
    let mut variants: Vec<UltradoxMagnetVariant> = Vec::new();
    let mut info = UltradoxDetailInfo::default();
    if body.trim().is_empty() {
        return None;
    }
    info.year = extract_detail_year(body);

    let mut seen = std::collections::HashSet::new();
    let re = crab_core::rx::re_i(DETAIL_MAGNET_RE);
    for c in re.captures_iter(body).filter_map(|c| c.ok()) {
        let Some(m0) = c.get(0) else { continue };
        let hash = c.get(1).map(|m| m.as_str().to_lowercase()).unwrap_or_default();
        if !seen.insert(hash.clone()) {
            continue;
        }
        let bytes = c.get(2).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(0);
        let dn = c.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
        let mut magnet = extract_full_magnet(body, m0.start());
        if magnet.is_empty() {
            magnet = m0.as_str().to_string();
        }
        let quality = extract_quality(&dn);
        variants.push(UltradoxMagnetVariant { hash, bytes, dn, magnet, quality });
    }

    for v in &variants {
        let original = original_from_filename(&v.dn);
        if !original.is_empty() {
            info.original = original;
            break;
        }
    }

    if variants.is_empty() {
        None
    } else {
        Some((variants, info))
    }
}

pub fn extract_detail_year(body: &str) -> i32 {
    if body.trim().is_empty() {
        return 0;
    }
    crab_core::rx::group_i(body, DETAIL_YEAR_RE, 1).parse().unwrap_or(0)
}

pub fn extract_quality(dn: &str) -> String {
    let clean = dn.replace('O', "0");
    let res = rx::captures(&clean, QUALITY_RES_RE).and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    if let Some(r) = res {
        return format!("{r}p");
    }
    QUALITY_TAGS.iter().find(|tag| dn.contains(**tag)).map(|t| t.to_string()).unwrap_or_default()
}

/// Split a listing title into (name, original, year).
pub fn parse_title(title: &str) -> (String, String, i32) {
    let mut year = 0;
    let cut: &str = match rx::captures(title, TITLE_YEAR_RE) {
        Some(c) => {
            year = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let idx = c.get(0).map(|m| m.start()).unwrap_or(title.len());
            &title[..idx]
        }
        None => trim_metadata_blocks(title),
    };
    let cut = cut.trim();
    if let Some(slash) = cut.find(" / ") {
        return (cut[..slash].trim().to_string(), cut[slash + 3..].trim().to_string(), year);
    }
    (cut.to_string(), String::new(), year)
}

/// Latin original title from a torrent file name (`Title.Words.2026.BDRip.avi.torrent` → `Title Words`).
pub fn original_from_filename(dn: &str) -> String {
    if dn.trim().is_empty() {
        return String::new();
    }
    let trimmed = if dn.len() >= 8 && dn[dn.len() - 8..].eq_ignore_ascii_case(".torrent") { &dn[..dn.len() - 8] } else { dn };
    let tokens: Vec<&str> = trimmed.split('.').collect();
    let end = tokens.iter().position(|t| is_dn_stop_token(t));
    let end = match end {
        Some(e) if e > 0 => e,
        _ => return String::new(),
    };
    let mut title_tokens: Vec<&str> = tokens[..end].to_vec();
    if title_tokens.len() > 1 {
        let last = title_tokens[title_tokens.len() - 1].to_uppercase();
        if last == "US" || last == "UK" {
            title_tokens.pop();
        }
    }
    let result = title_tokens.join(" ").trim().to_string();
    if !has_latin(&result) {
        return String::new();
    }
    result
}

/// Build one FileDB row for a listing item + magnet variant. `None` when unusable.
pub fn build_torrent(
    host: &str,
    section_path: &str,
    types: &[&str],
    item: &UltradoxListingItem,
    variant: &UltradoxMagnetVariant,
    info: Option<&UltradoxDetailInfo>,
) -> Option<TorrentDetails> {
    if variant.hash.trim().is_empty() || variant.magnet.trim().is_empty() {
        return None;
    }
    let default_info = UltradoxDetailInfo::default();
    let info = info.unwrap_or(&default_info);
    let (mut name, mut original_name, mut year) = parse_title(&item.title);
    if year == 0 {
        year = info.year;
    }

    // rufilm filenames are transliterations, not foreign originals.
    if original_name.trim().is_empty() && !section_path.eq_ignore_ascii_case("rufilm") {
        original_name = info.original.clone();
    }

    let mut title = item.title.trim().to_string();
    if !variant.quality.is_empty() && !title.to_lowercase().contains(&variant.quality.to_lowercase()) {
        title = format!("{title} [{}]", variant.quality);
    }

    if name.trim().is_empty() {
        if let Some(n) = rx::captures(&title, TITLE_NAME_RE).and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string())) {
            name = n;
        }
    }
    if name.trim().is_empty() {
        return None;
    }

    let host = host.trim_end_matches('/');
    let detail_url = absolute_on_host(host, &item.detail_url);
    let hash_prefix: String = variant.hash.chars().take(8).collect();
    let unique_url = format!("{detail_url}#h={hash_prefix}");

    let now = Utc::now();
    let mut t = TorrentDetails::new(TRACKER_NAME, types, unique_url, title);
    t.sid = 1;
    t.pir = 1;
    t.sizeName = human_size(variant.bytes);
    t.magnet = variant.magnet.clone();
    t.createTime = if time::is_min(&item.create_time) { now } else { item.create_time };
    t.updateTime = now;
    t.name = name;
    t.originalname = original_name;
    t.relased = year;
    Some(t)
}

pub fn human_size(bytes: i64) -> String {
    if bytes <= 0 {
        return String::new();
    }
    const KB: i64 = 1 << 10;
    const MB: i64 = 1 << 20;
    const GB: i64 = 1 << 30;
    const TB: i64 = 1 << 40;
    if bytes >= TB {
        return format!("{:.2} TB", bytes as f64 / TB as f64);
    }
    if bytes >= GB {
        return format!("{:.2} GB", bytes as f64 / GB as f64);
    }
    if bytes >= MB {
        return format!("{:.2} MB", bytes as f64 / MB as f64);
    }
    if bytes >= KB {
        return format!("{:.2} KB", bytes as f64 / KB as f64);
    }
    format!("{bytes} B")
}

pub fn listing_magnets_are_placeholders(body: &str) -> bool {
    body.contains("magnet:?xt=urn:btih:&")
}

fn http_url(s: &str) -> Option<url::Url> {
    url::Url::parse(s).ok().filter(|u| u.scheme() == "http" || u.scheme() == "https")
}

fn path_query_fragment(u: &url::Url) -> String {
    let mut s = u.path().to_string();
    if let Some(q) = u.query().filter(|q| !q.is_empty()) {
        s.push('?');
        s.push_str(q);
    }
    if let Some(f) = u.fragment().filter(|f| !f.is_empty()) {
        s.push('#');
        s.push_str(f);
    }
    s
}

/// Put a listing/detail href on `host`, so stored urls stay on ultradox.vip, not a numbered mirror.
pub fn absolute_on_host(host: &str, href: &str) -> String {
    let host = host.trim_end_matches('/');
    let href = href.trim();
    if href.is_empty() {
        return host.to_string();
    }
    if let Some(u) = http_url(href) {
        return format!("{host}{}", path_query_fragment(&u));
    }
    format!("{host}/{}", href.trim_start_matches('/'))
}

/// Host-independent path + query + fragment, lowercase.
pub fn canonical_path_and_fragment(url: &str) -> String {
    if url.trim().is_empty() {
        return String::new();
    }
    let url = url.trim();
    if let Some(u) = http_url(url) {
        return path_query_fragment(&u).to_lowercase();
    }
    if url.starts_with('/') {
        url.to_lowercase()
    } else {
        format!("/{url}").to_lowercase()
    }
}

pub fn canonical_torrent_url(host: &str, url: &str) -> String {
    let host = host.trim_end_matches('/');
    let path = canonical_path_and_fragment(url);
    if path.is_empty() {
        return String::new();
    }
    if path.starts_with('/') {
        format!("{host}{path}")
    } else {
        format!("{host}/{path}")
    }
}

fn flatten_title(raw: &str) -> String {
    let span = crab_core::rx::group_i(raw, ROW_SPAN_QUALITY_RE, 1);
    let mut main_text = html_decode(&rx::replace(raw, TAGS_RE, " "));
    if !span.is_empty() {
        let span_plain = html_decode(&rx::replace(&span, TAGS_RE, ""));
        if !span_plain.is_empty() {
            main_text = main_text.replace(&span_plain, "");
        }
        main_text = format!("{} {span_plain}", main_text.trim());
    }
    collapse_spaces(&main_text)
}

fn trim_metadata_blocks(title: &str) -> &str {
    let mut end = title.len();
    if let Some(m) = rx::captures(title, TITLE_SEASON_RE).and_then(|c| c.get(0).map(|m| m.start())) {
        end = m;
    }
    if let Some(bracket) = title.find('[') {
        if bracket < end {
            end = bracket;
        }
    }
    &title[..end]
}

fn is_dn_stop_token(tok: &str) -> bool {
    let norm = tok.replace('O', "0");
    if rx::is_match(&norm, DN_YEAR_RE) || rx::is_match(&norm, DN_RES_RE) {
        return true;
    }
    if rx::is_match(tok, DN_SEASON_RE) {
        return true;
    }
    DN_STOP_TOKENS.iter().any(|s| s.eq_ignore_ascii_case(tok))
}

fn extract_full_magnet(body: &str, start: usize) -> String {
    if start >= body.len() {
        return String::new();
    }
    let rest = &body[start..];
    match rest.find(['"', '<']) {
        Some(end) => html_decode(&rest[..end]),
        None => String::new(),
    }
}

fn collapse_spaces(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = html_decode(&rx::replace(s, TAGS_RE, " "));
    rx::replace(&s, WHITESPACE_RE, " ").trim().to_string()
}

fn has_latin(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}
