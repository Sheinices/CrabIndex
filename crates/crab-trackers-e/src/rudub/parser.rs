//! RuDub browse.php card parser. Keeps only HD 1080 / HD 2160 titles (drops XviD, x264-SD, HD 720).

use chrono::{DateTime, Datelike, NaiveDateTime, TimeZone, Utc};
use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::rx;
use crab_core::time;
use crab_core::util::html_decode;

pub const TRACKER_NAME: &str = "rudub";
pub const VALIDATION_MARKER: &str = "card__torlist__browse_2";
pub const ENDPOINT_DOWNLOAD: &str = "/download2.php";

/// Site `videoformat` query: 4 = HD 1080, 5 = HD 2160.
pub const PREFERRED_VIDEO_FORMATS: [i32; 2] = [4, 5];

const TYPE_SERIAL: &str = "serial";
const TYPE_MOVIE: &str = "movie";

const CARD_SPLIT_RE: &str = r#"<div\s+class="card__torlist__browse_2""#;
const DETAILS_RE: &str = r#"href=["']/?(details\.php\?id=([0-9]+))["'][^>]*>\s*<b>([\s\S]*?)</b>"#;
const DOWNLOAD_ID_RE: &str = r#"href=["']/?(?:download2\.php\?id=|download\.php\?id=)([0-9]+)["']"#;
const DATE_RE: &str = r#"li\s+title=["']Дата["'][^>]*>[\s\S]*?</i>\s*([0-9]{4}-[0-9]{2}-[0-9]{2}\s+[0-9]{2}:[0-9]{2}:[0-9]{2})"#;
const SIZE_RE: &str = r#"li\s+title=["']Размер["'][^>]*>[\s\S]*?</i>\s*([^<]+)"#;
const ACTIVITY_RE: &str = r#"li\s+title=["']Активность["'][^>]*>[\s\S]*?</i>\s*(\d+)\s*<[\s\S]*?</i>\s*(\d+)"#;
const GOOD_QUALITY_RE: &str = r"(?i)(?:\b|[^0-9])(?:HD|BD|HDR)?(?:1080p|2160p)\b";
const BAD_QUALITY_RE: &str = r"(?i)(?:\bWEBRip\s*XviD\b|\bWEBRip\s*x264\b|\bHD720p\b|(?<![0-9])720p\b)";
const YEAR_ONLY_RE: &str = r"^(?:19|20)\d{2}(?:\s*[-–]\s*(?:19|20)\d{2})?$";
const SERIAL_RE_1: &str = r"[CcСс]езон";
const SERIAL_RE_2: &str = r"[CcСс]ери";
const SERIAL_RE_3: &str = r"(?i)/\s*s\d+e\d+";
const WHITESPACE_RE: &str = r"[\n\r\t ]+";
const BR_RE: &str = r"(?i)<br\s*/?>";

/// Listing row plus the authenticated `.torrent` download url.
#[derive(Clone, Debug, Default)]
pub struct RudubDetails {
    pub t: TorrentDetails,
    pub download_uri: String,
}

impl AsRef<TorrentDetails> for RudubDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for RudubDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

pub fn is_preferred_quality_title(title: &str) -> bool {
    if title.trim().is_empty() {
        return false;
    }
    let good = rx::is_match(title, GOOD_QUALITY_RE);
    if rx::is_match(title, BAD_QUALITY_RE) && !good {
        return false;
    }
    good
}

pub fn parse_torrent_list_from_html(html: &str, host: &str) -> Vec<RudubDetails> {
    let mut torrents = Vec::new();
    if html.trim().is_empty() || host.trim().is_empty() {
        return torrents;
    }
    let host = host.trim_end_matches('/');
    let decoded = tparse::replace_bad_names(&html_decode(&html.replace("&nbsp;", " ")));

    for card in crab_core::rx::split_i(&decoded, CARD_SPLIT_RE).iter().skip(1) {
        let d = crab_core::rx::groups_i(card, DETAILS_RE);
        if d[0].is_empty() {
            continue;
        }
        let rel_url = &d[1];
        let id = &d[2];
        let title = normalize_title(&d[3]);
        if title.trim().is_empty() || id.trim().is_empty() {
            continue;
        }
        if !is_preferred_quality_title(&title) {
            continue;
        }
        let download_id = crab_core::rx::group_i(card, DOWNLOAD_ID_RE, 1);
        if download_id.trim().is_empty() {
            continue;
        }

        let mut create_time = parse_card_date(card);
        if time::is_min(&create_time) {
            create_time = Utc::now();
        }
        let (name, originalname, relased) = parse_title_fields(&title, create_time);
        let Some(name) = name.filter(|n| !n.trim().is_empty()) else { continue };

        let (mut sid, mut pir) = (1, 0);
        let act = crab_core::rx::groups_i(card, ACTIVITY_RE);
        if !act[0].is_empty() {
            sid = act[1].parse().unwrap_or(0);
            pir = act[2].parse().unwrap_or(0);
        }

        let size_raw = crab_core::rx::captures_i(card, SIZE_RE).and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
        let size_name = size_raw.map(|s| rx::replace(&s, WHITESPACE_RE, " ").trim().to_string()).unwrap_or_default();

        let mut t = TorrentDetails::new(TRACKER_NAME, detect_content_type(&title), format!("{host}/{rel_url}"), title.clone());
        t.name = name;
        t.originalname = originalname.unwrap_or_default();
        t.sid = sid;
        t.pir = pir;
        t.createTime = create_time;
        t.sizeName = size_name;
        t.quality = detect_quality(&title);
        t.relased = relased;
        torrents.push(RudubDetails { t, download_uri: format!("{host}{ENDPOINT_DOWNLOAD}?id={download_id}") });
    }
    torrents
}

pub fn types_equal(a: Option<&[String]>, b: Option<&[String]>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// A real `.torrent` starts with a bencoded dictionary (`d`).
pub fn is_valid_bencoded_torrent(data: &[u8]) -> bool {
    !data.is_empty() && data[0] == b'd'
}

fn normalize_title(raw: &str) -> String {
    let t = rx::replace(raw, BR_RE, " ");
    let t = rx::replace(&html_decode(&t), WHITESPACE_RE, " ").trim().to_string();
    let t = replace_ci(&t, "(Обновляемая)", "");
    let t = replace_ci(&t, "(Оновлюється)", "");
    let t = replace_ci(&t, "(Золото)", "");
    rx::replace(&t, WHITESPACE_RE, " ").trim().to_string()
}

fn replace_ci(s: &str, needle: &str, with: &str) -> String {
    let pattern = format!("(?i){}", regex::escape(needle));
    rx::re(&pattern).replace_all(s, regex_literal(with)).into_owned()
}

fn regex_literal(s: &str) -> fancy_regex::NoExpand<'_> {
    fancy_regex::NoExpand(s)
}

/// Split a listing title into name / originalname / year. The year comes from a `(YYYY)` group,
/// otherwise from `create_time`; year-only parens are never an originalname; nested parens stay balanced.
pub fn parse_title_fields(title: &str, create_time: DateTime<Utc>) -> (Option<String>, Option<String>, i32) {
    let mut name: Option<String> = None;
    let mut originalname: Option<String> = None;
    let mut relased = 0;

    if !title.trim().is_empty() {
        for (start, inner) in enumerate_paren_groups(title) {
            if let Some(year) = try_parse_year_group(inner) {
                if relased <= 0 {
                    relased = year;
                }
                continue;
            }
            if originalname.is_none() {
                name = Some(strip_trailing_year_parens(title[..start].trim()));
                originalname = Some(inner.trim().to_string());
            }
        }
        if name.as_deref().map(|n| n.trim().is_empty()).unwrap_or(true) {
            name = crab_core::rx::split_i(title, r"(\(|/|\|)").into_iter().next().map(|s| s.trim().to_string());
        }
    }

    if relased <= 0 && !time::is_min(&create_time) && create_time.year() > 1 {
        relased = create_time.year();
    }

    (name.filter(|n| !n.trim().is_empty()), originalname.filter(|o| !o.trim().is_empty()), relased)
}

fn enumerate_paren_groups(title: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let bytes = title.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(rel) = title[i..].find('(') else { break };
        let open = i + rel;
        let mut depth = 0;
        let mut close = None;
        for (j, &c) in bytes.iter().enumerate().skip(open) {
            if c == b'(' {
                depth += 1;
            } else if c == b')' {
                depth -= 1;
                if depth == 0 {
                    close = Some(j);
                    break;
                }
            }
        }
        let Some(close) = close else { break };
        out.push((open, &title[open + 1..close]));
        i = close + 1;
    }
    out
}

fn try_parse_year_group(inner: &str) -> Option<i32> {
    if inner.trim().is_empty() {
        return None;
    }
    let t = inner.trim();
    if !rx::is_match(t, YEAR_ONLY_RE) {
        return None;
    }
    let head: String = t.chars().take(4).collect();
    let year: i32 = head.parse().ok()?;
    (1900..=2100).contains(&year).then_some(year)
}

fn strip_trailing_year_parens(prefix: &str) -> String {
    let mut prefix = prefix.to_string();
    while !prefix.trim().is_empty() {
        prefix = prefix.trim_end().to_string();
        if !prefix.ends_with(')') {
            break;
        }
        let Some(open) = prefix.rfind('(') else { break };
        let inner = &prefix[open + 1..prefix.len() - 1];
        if try_parse_year_group(inner).is_none() {
            break;
        }
        prefix = prefix[..open].trim_end().to_string();
    }
    prefix
}

fn parse_card_date(card: &str) -> DateTime<Utc> {
    let s = crab_core::rx::group_i(card, DATE_RE, 1);
    if s.is_empty() {
        return time::min();
    }
    match NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S") {
        Ok(n) => Utc.from_utc_datetime(&n),
        Err(_) => time::min(),
    }
}

fn detect_quality(title: &str) -> i32 {
    if crab_core::rx::is_match_i(title, "2160p") {
        2160
    } else if crab_core::rx::is_match_i(title, "1080p") {
        1080
    } else {
        0
    }
}

fn detect_content_type(title: &str) -> &'static [&'static str] {
    let serial = rx::is_match(title, SERIAL_RE_1) || rx::is_match(title, SERIAL_RE_2) || rx::is_match(title, SERIAL_RE_3);
    if serial {
        &[TYPE_SERIAL]
    } else {
        &[TYPE_MOVIE]
    }
}
